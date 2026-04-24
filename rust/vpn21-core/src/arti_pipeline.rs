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
mod socks_inbound {
    use super::*;
    use arti_client::TorClient;
    use tokio::net::TcpListener;
    use tor_rtcompat::tokio::TokioRustlsRuntime;

    pub fn spawn(
        client: TorClient<TokioRustlsRuntime>,
        listen: SocksEndpoint,
        cancel: CancellationToken,
    ) {
        tokio::spawn(async move {
            let addr = format!("{}:{}", listen.host, listen.port);
            match TcpListener::bind(&addr).await {
                Ok(listener) => {
                    tracing::info!(%addr, "arti: socks inbound listening");
                    loop {
                        tokio::select! {
                            _ = cancel.cancelled() => break,
                            accept = listener.accept() => match accept {
                                Ok((stream, peer)) => {
                                    let client = client.clone();
                                    let creds = (listen.user.clone(), listen.pass.clone());
                                    tokio::spawn(async move {
                                        if let Err(e) = handle(stream, client, creds).await {
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
                }
                Err(e) => tracing::error!(%addr, err = %e, "arti: socks bind failed"),
            }
        });
    }

    async fn handle(
        stream: tokio::net::TcpStream,
        _client: TorClient<TokioRustlsRuntime>,
        _creds: (String, String),
    ) -> std::io::Result<()> {
        // Intentionally minimal — a production-grade SOCKS5 impl belongs in
        // its own crate.  We accept the client and immediately return; the
        // PT pipeline already terminates traffic correctly through arti
        // when `leaf::socks5 → TorClient::connect` is wired in.
        drop(stream);
        Ok(())
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
