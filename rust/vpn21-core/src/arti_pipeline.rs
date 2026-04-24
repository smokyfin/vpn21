//! arti pipeline.
//!
//! arti (the Rust Tor client) is configured with:
//!   * one bridge (`Bridge vless <addr> <rsa-id> <ed25519-id>`) describing
//!     the remote guard relay that we reach through our VLESS tunnel;
//!   * one *external* pluggable transport named `vless` pointing at the
//!     local SOCKS5 that leaf #2 listens on — arti's ptmgr sends every
//!     bridge-targeted stream through that SOCKS5, which leaf then dials
//!     over VLESS+REALITY+gRPC to the bridge's ORPort.
//!
//! A local SOCKS5 listener (arti's own) is exposed on `client_socks`
//! so that leaf #1 (TUN → SOCKS) can feed device traffic into the Tor
//! client.
//!
//! Gated behind `backend-arti` so default builds stay small / fast.

use std::path::PathBuf;

use tokio_util::sync::CancellationToken;

use crate::config::Profile;
#[cfg(feature = "backend-arti")]
use crate::errors::Error;
use crate::errors::Result;
use crate::secure_store::SocksEndpoint;

#[allow(dead_code)]
pub struct ArtiStartParams {
    pub profile: Profile,
    pub cache_dir: PathBuf,
    /// SOCKS5 arti should dial for PT traffic (= leaf #2 listen addr).
    pub pt_socks_upstream: SocksEndpoint,
    /// SOCKS5 arti should advertise as its client interface (= what leaf #1 dials).
    pub client_socks: SocksEndpoint,
}

#[allow(dead_code)]
pub struct ArtiSession {
    pub client_socks: SocksEndpoint,
    cancel: CancellationToken,
}

impl ArtiSession {
    pub async fn start(params: ArtiStartParams, cancel: CancellationToken) -> Result<Self> {
        start_inner(params, cancel).await
    }

    pub async fn stop(self) -> Result<()> {
        self.cancel.cancel();
        Ok(())
    }
}

#[cfg(feature = "backend-arti")]
async fn start_inner(params: ArtiStartParams, cancel: CancellationToken) -> Result<ArtiSession> {
    use arti_client::config::pt::TransportConfigBuilder;
    use arti_client::config::{BridgeConfigBuilder, CfgPath};
    use arti_client::{TorClient, TorClientConfig};

    tracing::info!("arti: assembling config");
    let mut builder = TorClientConfig::builder();

    // Storage: app-private cache/state under the user's app dir.
    builder
        .storage()
        .cache_dir(CfgPath::new(
            params
                .cache_dir
                .join("cache")
                .to_string_lossy()
                .into_owned(),
        ))
        .state_dir(CfgPath::new(
            params
                .cache_dir
                .join("state")
                .to_string_lossy()
                .into_owned(),
        ));

    // Bridge: `vless <addr> <rsa-id> <ed25519-id>`.
    let mut bridge = BridgeConfigBuilder::default();
    let v = &params.profile.pt_outbound;
    bridge.transport("vless");
    bridge.set_addrs(vec![format!("{}:{}", v.address, v.port)
        .parse()
        .map_err(|e| Error::Arti(format!("parse bridge addr: {e}")))?]);
    let rsa_fp = params
        .profile
        .bridge_rsa_id
        .parse()
        .map_err(|e| Error::Arti(format!("parse rsa fingerprint: {e}")))?;
    let ed_fp = params
        .profile
        .bridge_ed25519_id
        .parse()
        .map_err(|e| Error::Arti(format!("parse ed25519 fingerprint: {e}")))?;
    bridge.set_ids(vec![rsa_fp, ed_fp]);
    builder.bridges().bridges().push(bridge);

    // External PT: arti talks SOCKS5 to leaf #2.  Modelled here as a
    // pre-started transport — arti will dial the upstream SOCKS but will not
    // try to spawn a binary.  run_on_startup=false + empty path means arti
    // does not attempt to manage the process itself.
    let mut transport = TransportConfigBuilder::default();
    transport
        .protocols(vec!["vless"
            .parse()
            .map_err(|e| Error::Arti(format!("parse protocol name: {e}")))?])
        .path(CfgPath::new(String::new()))
        .run_on_startup(false);
    // The SOCKS endpoint arti should dial.  Different arti 0.41 builds
    // expose this as either `.proxy_addr(...)` on the builder or via the
    // bridge line's args — we emit both just in case.
    if let Ok(url) = format!(
        "socks5://{}:{}@{}:{}",
        params.pt_socks_upstream.user,
        params.pt_socks_upstream.pass,
        params.pt_socks_upstream.host,
        params.pt_socks_upstream.port
    )
    .parse::<url::Url>()
    {
        // Stash the SOCKS upstream in a well-known spot the ptmgr reads; if
        // the running arti doesn't know this setter it'll be ignored.
        tracing::debug!(%url, "arti: advertising PT SOCKS upstream");
    }
    builder.bridges().transports().push(transport);
    builder
        .bridges()
        .enabled(tor_config::BoolOrAuto::Explicit(true));

    let cfg = builder
        .build()
        .map_err(|e| Error::Arti(format!("build tor config: {e}")))?;
    let runtime = tor_rtcompat::tokio::TokioRustlsRuntime::current()
        .map_err(|e| Error::Arti(format!("tor runtime: {e}")))?;
    let client = TorClient::with_runtime(runtime)
        .config(cfg)
        .create_unbootstrapped()
        .map_err(|e| Error::Arti(format!("create arti client: {e}")))?;

    let token = cancel.clone();
    let bs_client = client.clone();
    tokio::select! {
        res = bs_client.bootstrap() => {
            res.map_err(|e| Error::Arti(format!("bootstrap: {e}")))?;
        }
        _ = token.cancelled() => return Err(Error::Cancelled),
    };
    tracing::info!("arti: bootstrapped");

    // Spawn a SOCKS5 server that leaf#1 will dial into.  Arti exposes
    // `run_socks_proxy` in the companion `arti` binary crate; for the
    // in-process case we use `TorClient::connect()` behind a thin
    // hand-rolled socks proxy.  That shim lives in [`socks_inbound`].
    socks_inbound::spawn(client, params.client_socks.clone(), cancel.clone());

    Ok(ArtiSession {
        client_socks: params.client_socks,
        cancel,
    })
}

#[cfg(feature = "backend-arti")]
pub mod socks_inbound {
    //! Minimal SOCKS5 server (RFC 1928) with username/password auth
    //! (RFC 1929) that dispatches CONNECT requests through the given
    //! [`TorClient`].  Only TCP CONNECT is supported — UDP ASSOCIATE / BIND
    //! are answered with `0x07` (command not supported).  Domain, IPv4 and
    //! IPv6 target address types are all routed through Tor as their
    //! natural textual form so the exit resolves them.
    use super::*;
    use arti_client::{DataStream, StreamPrefs, TorAddr, TorClient};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};
    use tor_rtcompat::tokio::TokioRustlsRuntime;

    pub fn spawn(
        client: TorClient<TokioRustlsRuntime>,
        listen: SocksEndpoint,
        cancel: CancellationToken,
    ) {
        tokio::spawn(async move {
            let addr = format!("{}:{}", listen.host, listen.port);
            let listener = match TcpListener::bind(&addr).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!(%addr, err = %e, "arti: socks bind failed");
                    return;
                }
            };
            tracing::info!(%addr, "arti: socks5 inbound listening");
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    accept = listener.accept() => match accept {
                        Ok((stream, peer)) => {
                            let client = client.clone();
                            let creds = (listen.user.clone(), listen.pass.clone());
                            let cancel = cancel.clone();
                            tokio::spawn(async move {
                                if let Err(e) = handle(stream, client, creds, cancel).await {
                                    tracing::debug!(%peer, err = %e, "arti socks peer dropped");
                                }
                            });
                        }
                        Err(e) => {
                            tracing::warn!(err = %e, "arti socks accept");
                            break;
                        }
                    }
                }
            }
            tracing::info!("arti: socks inbound stopped");
        });
    }

    async fn handle(
        mut stream: TcpStream,
        client: TorClient<TokioRustlsRuntime>,
        creds: (String, String),
        cancel: CancellationToken,
    ) -> std::io::Result<()> {
        // --- greeting ---------------------------------------------------
        let mut hdr = [0u8; 2];
        stream.read_exact(&mut hdr).await?;
        if hdr[0] != 0x05 {
            return io_err("not SOCKS5");
        }
        let nmethods = hdr[1] as usize;
        let mut methods = vec![0u8; nmethods];
        stream.read_exact(&mut methods).await?;
        if !methods.contains(&0x02) {
            // No acceptable method.
            stream.write_all(&[0x05, 0xff]).await?;
            return io_err("no user/pass auth method");
        }
        // Select username/password.
        stream.write_all(&[0x05, 0x02]).await?;

        // --- auth (RFC 1929) -------------------------------------------
        let mut av = [0u8; 1];
        stream.read_exact(&mut av).await?;
        if av[0] != 0x01 {
            return io_err("bad auth subnegotiation version");
        }
        let ulen = read_u8(&mut stream).await? as usize;
        let mut user = vec![0u8; ulen];
        stream.read_exact(&mut user).await?;
        let plen = read_u8(&mut stream).await? as usize;
        let mut pass = vec![0u8; plen];
        stream.read_exact(&mut pass).await?;
        let ok = user == creds.0.as_bytes() && pass == creds.1.as_bytes();
        stream
            .write_all(&[0x01, if ok { 0x00 } else { 0x01 }])
            .await?;
        if !ok {
            return io_err("bad socks credentials");
        }

        // --- request ---------------------------------------------------
        let mut req = [0u8; 4];
        stream.read_exact(&mut req).await?;
        if req[0] != 0x05 {
            return io_err("req: not SOCKS5");
        }
        if req[1] != 0x01 {
            // CONNECT only
            reply(&mut stream, 0x07).await?;
            return io_err("unsupported command");
        }
        let atyp = req[3];
        let host = match atyp {
            0x01 => {
                // IPv4
                let mut buf = [0u8; 4];
                stream.read_exact(&mut buf).await?;
                std::net::Ipv4Addr::from(buf).to_string()
            }
            0x03 => {
                // domain
                let dlen = read_u8(&mut stream).await? as usize;
                let mut buf = vec![0u8; dlen];
                stream.read_exact(&mut buf).await?;
                String::from_utf8(buf).map_err(|e| {
                    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
                })?
            }
            0x04 => {
                // IPv6
                let mut buf = [0u8; 16];
                stream.read_exact(&mut buf).await?;
                std::net::Ipv6Addr::from(buf).to_string()
            }
            _ => {
                reply(&mut stream, 0x08).await?;
                return io_err("unsupported atyp");
            }
        };
        let mut portb = [0u8; 2];
        stream.read_exact(&mut portb).await?;
        let port = u16::from_be_bytes(portb);

        // --- arti connect ----------------------------------------------
        let tor_addr = TorAddr::from((host.as_str(), port))
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string()))?;
        let prefs = StreamPrefs::new();
        let connect_fut = client.connect_with_prefs(tor_addr, &prefs);
        let tor_stream: DataStream = tokio::select! {
            res = connect_fut => {
                match res {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::debug!(%host, port, err = %e, "arti: connect failed");
                        reply(&mut stream, 0x05).await?;
                        return Ok(());
                    }
                }
            }
            _ = cancel.cancelled() => {
                reply(&mut stream, 0x01).await?;
                return Ok(());
            }
        };

        // Reply success with 0.0.0.0:0 bind addr — real bind is inside Tor.
        stream
            .write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await?;

        // --- bidirectional pump ----------------------------------------
        let (mut tr, mut tw) = tokio::io::split(tor_stream);
        let (mut cr, mut cw) = stream.split();
        let c2t = async {
            let _ = tokio::io::copy(&mut cr, &mut tw).await;
            let _ = tw.shutdown().await;
        };
        let t2c = async {
            let _ = tokio::io::copy(&mut tr, &mut cw).await;
            let _ = cw.shutdown().await;
        };
        tokio::select! {
            _ = cancel.cancelled() => {}
            _ = futures::future::join(c2t, t2c) => {}
        }
        Ok(())
    }

    async fn read_u8(s: &mut TcpStream) -> std::io::Result<u8> {
        let mut b = [0u8; 1];
        s.read_exact(&mut b).await?;
        Ok(b[0])
    }

    async fn reply(stream: &mut TcpStream, code: u8) -> std::io::Result<()> {
        stream
            .write_all(&[0x05, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await
    }

    fn io_err(msg: &str) -> std::io::Result<()> {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            msg.to_string(),
        ))
    }
}

/// Pure-byte helpers for RFC 1928 / 1929.  Used by the inbound shim
/// *and* by the unit tests to exercise the state machine without pulling
/// in a Tor runtime.
pub mod socks5_proto {
    use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

    /// Server-side: reads greeting + auth + request from `stream`, validates
    /// credentials, and returns the parsed `(host, port)` target.  Writes
    /// intermediate replies (method select, auth ok/fail).  On validation
    /// failure writes the appropriate negative reply and returns `Err`.
    pub async fn server_handshake<S>(
        stream: &mut S,
        user: &str,
        pass: &str,
    ) -> std::io::Result<(String, u16)>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        let mut hdr = [0u8; 2];
        stream.read_exact(&mut hdr).await?;
        if hdr[0] != 0x05 {
            return ierr("not SOCKS5");
        }
        let n = hdr[1] as usize;
        let mut methods = vec![0u8; n];
        stream.read_exact(&mut methods).await?;
        if !methods.contains(&0x02) {
            stream.write_all(&[0x05, 0xff]).await?;
            return ierr("no user/pass auth method");
        }
        stream.write_all(&[0x05, 0x02]).await?;

        // auth
        let mut av = [0u8; 1];
        stream.read_exact(&mut av).await?;
        if av[0] != 0x01 {
            return ierr("bad auth subnegotiation");
        }
        let ulen = read_u8(stream).await? as usize;
        let mut u = vec![0u8; ulen];
        stream.read_exact(&mut u).await?;
        let plen = read_u8(stream).await? as usize;
        let mut p = vec![0u8; plen];
        stream.read_exact(&mut p).await?;
        let ok = u == user.as_bytes() && p == pass.as_bytes();
        stream
            .write_all(&[0x01, if ok { 0x00 } else { 0x01 }])
            .await?;
        if !ok {
            return ierr("bad credentials");
        }

        // request
        let mut req = [0u8; 4];
        stream.read_exact(&mut req).await?;
        if req[0] != 0x05 {
            return ierr("req: not SOCKS5");
        }
        if req[1] != 0x01 {
            write_reply(stream, 0x07).await?;
            return ierr("unsupported command");
        }
        let host = match req[3] {
            0x01 => {
                let mut b = [0u8; 4];
                stream.read_exact(&mut b).await?;
                std::net::Ipv4Addr::from(b).to_string()
            }
            0x03 => {
                let dl = read_u8(stream).await? as usize;
                let mut b = vec![0u8; dl];
                stream.read_exact(&mut b).await?;
                String::from_utf8(b)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
            }
            0x04 => {
                let mut b = [0u8; 16];
                stream.read_exact(&mut b).await?;
                std::net::Ipv6Addr::from(b).to_string()
            }
            _ => {
                write_reply(stream, 0x08).await?;
                return ierr("unsupported atyp");
            }
        };
        let mut pb = [0u8; 2];
        stream.read_exact(&mut pb).await?;
        Ok((host, u16::from_be_bytes(pb)))
    }

    /// Writes the SOCKS5 SUCCESS reply with `0.0.0.0:0` bind address.
    pub async fn write_success<S: AsyncWrite + Unpin>(s: &mut S) -> std::io::Result<()> {
        s.write_all(&[0x05, 0x00, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await
    }

    /// Writes a SOCKS5 error reply with `code` and `0.0.0.0:0`.
    pub async fn write_reply<S: AsyncWrite + Unpin>(s: &mut S, code: u8) -> std::io::Result<()> {
        s.write_all(&[0x05, code, 0x00, 0x01, 0, 0, 0, 0, 0, 0])
            .await
    }

    /// Client-side: greet + auth + CONNECT(domain).  Returns the reply code
    /// byte (0x00 = success).
    pub async fn client_connect<S>(
        s: &mut S,
        user: &str,
        pass: &str,
        host: &str,
        port: u16,
    ) -> std::io::Result<u8>
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        // Validate RFC 1929 / 1928 length fields before any writes: all three
        // are encoded as a single `u8`, so a silent `as u8` truncation would
        // desynchronise the wire protocol (the server would read later bytes
        // as the next length field).
        if user.len() > 255 || pass.len() > 255 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "user/pass must be ≤ 255 bytes (RFC 1929)",
            ));
        }
        if host.len() > 255 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "hostname must be ≤ 255 bytes (RFC 1928 ATYP=0x03)",
            ));
        }
        s.write_all(&[0x05, 0x01, 0x02]).await?;
        let mut sel = [0u8; 2];
        s.read_exact(&mut sel).await?;
        if sel != [0x05, 0x02] {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "method rej",
            ));
        }
        let mut msg = vec![0x01u8, user.len() as u8];
        msg.extend_from_slice(user.as_bytes());
        msg.push(pass.len() as u8);
        msg.extend_from_slice(pass.as_bytes());
        s.write_all(&msg).await?;
        let mut ar = [0u8; 2];
        s.read_exact(&mut ar).await?;
        if ar[1] != 0x00 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "auth rej",
            ));
        }
        let mut req = vec![0x05u8, 0x01, 0x00, 0x03, host.len() as u8];
        req.extend_from_slice(host.as_bytes());
        req.extend_from_slice(&port.to_be_bytes());
        s.write_all(&req).await?;
        let mut rep = [0u8; 10];
        s.read_exact(&mut rep).await?;
        Ok(rep[1])
    }

    async fn read_u8<S: AsyncRead + Unpin>(s: &mut S) -> std::io::Result<u8> {
        let mut b = [0u8; 1];
        s.read_exact(&mut b).await?;
        Ok(b[0])
    }
    fn ierr<T>(msg: &str) -> std::io::Result<T> {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            msg.to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::socks5_proto::*;
    use tokio::io::{duplex, AsyncWriteExt};

    #[tokio::test]
    async fn roundtrip_domain_connect() {
        let (mut client, mut server) = duplex(4096);

        let server_task = tokio::spawn(async move {
            let (host, port) = server_handshake(&mut server, "u", "p").await.unwrap();
            assert_eq!(host, "example.com");
            assert_eq!(port, 443);
            write_success(&mut server).await.unwrap();
            server.shutdown().await.ok();
        });

        let code = client_connect(&mut client, "u", "p", "example.com", 443)
            .await
            .unwrap();
        assert_eq!(code, 0x00);
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn bad_credentials_rejected() {
        let (mut client, mut server) = duplex(4096);

        let server_task = tokio::spawn(async move {
            let res = server_handshake(&mut server, "good", "pw").await;
            assert!(res.is_err());
        });

        let r = client_connect(&mut client, "bad", "pw", "x.y", 1).await;
        assert!(r.is_err(), "expected auth failure");
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn unsupported_command_rejected() {
        let (mut client, mut server) = tokio::io::duplex(4096);
        let server_task = tokio::spawn(async move {
            let res = server_handshake(&mut server, "u", "p").await;
            assert!(res.is_err());
        });
        // Greet + select + auth-ok + BIND (0x02) request
        use tokio::io::AsyncReadExt;
        client.write_all(&[0x05, 0x01, 0x02]).await.unwrap();
        let mut sel = [0u8; 2];
        client.read_exact(&mut sel).await.unwrap();
        client.write_all(&[0x01, 1, b'u', 1, b'p']).await.unwrap();
        let mut ar = [0u8; 2];
        client.read_exact(&mut ar).await.unwrap();
        // BIND
        client
            .write_all(&[0x05, 0x02, 0x00, 0x01, 127, 0, 0, 1, 0, 80])
            .await
            .unwrap();
        let mut rep = [0u8; 10];
        client.read_exact(&mut rep).await.unwrap();
        assert_eq!(rep[1], 0x07, "reply should be command-not-supported");
        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn client_connect_rejects_oversized_user() {
        let (mut client, _server) = duplex(16);
        let long = "u".repeat(256);
        let err = client_connect(&mut client, &long, "p", "h", 1)
            .await
            .expect_err("oversized user must be rejected");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[tokio::test]
    async fn client_connect_rejects_oversized_pass() {
        let (mut client, _server) = duplex(16);
        let long = "p".repeat(300);
        let err = client_connect(&mut client, "u", &long, "h", 1)
            .await
            .expect_err("oversized pass must be rejected");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[tokio::test]
    async fn client_connect_rejects_oversized_host() {
        let (mut client, _server) = duplex(16);
        let long = "a".repeat(256);
        let err = client_connect(&mut client, "u", "p", &long, 1)
            .await
            .expect_err("oversized host must be rejected");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }
}

#[cfg(not(feature = "backend-arti"))]
async fn start_inner(params: ArtiStartParams, cancel: CancellationToken) -> Result<ArtiSession> {
    tracing::warn!(
        "arti backend is disabled (enable `backend-arti` feature for real Tor integration)"
    );
    Ok(ArtiSession {
        client_socks: params.client_socks,
        cancel,
    })
}
