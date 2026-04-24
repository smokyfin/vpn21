//! TUN-bound DNS resolver + DoH forwarder.
//!
//! All device DNS traffic is captured by the TUN interface and delivered to
//! a UDP listener bound to the TUN's address on port 53.  We parse each
//! query with [`hickory_proto`], ignore the cache (simplicity — the resolver
//! we delegate to has its own), and forward the raw wire-format question to
//! the user-configured DoH endpoint.  The response bytes are piped back
//! verbatim.
//!
//! Requirements:
//!
//! * If `doh_server_ip` is set in the profile, connect to that IP literally
//!   and use the hostname only for SNI.  The OS resolver is NOT consulted.
//! * Otherwise, resolve the DoH hostname *through the VPN tunnel* by pointing
//!   reqwest at our own SOCKS5 (= leaf #1) — this prevents DNS bootstrap
//!   leaks.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use hickory_proto::op::Message;
use tokio::net::UdpSocket;
use tokio_util::sync::CancellationToken;

use crate::errors::{Error, Result};

pub struct DnsServer {
    pub bind: SocketAddr,
    cancel: CancellationToken,
    _runner: tokio::task::JoinHandle<()>,
}

#[derive(Debug, Clone)]
pub struct DnsSettings {
    pub doh_url: String,
    pub doh_server_ip: Option<IpAddr>,
    /// When `Some`, DoH traffic is tunneled through this SOCKS5 endpoint.
    pub socks_upstream: Option<String>,
}

impl DnsServer {
    pub async fn start(
        bind: SocketAddr,
        settings: DnsSettings,
        cancel: CancellationToken,
    ) -> Result<Self> {
        let sock = UdpSocket::bind(bind)
            .await
            .map_err(|e| Error::Dns(format!("bind dns listener on {bind}: {e}")))?;
        tracing::info!(%bind, "dns server listening");
        let sock = Arc::new(sock);

        let client = build_client(&settings)?;
        let settings = Arc::new(settings);
        let cancel_loop = cancel.clone();
        let runner = tokio::spawn(async move {
            serve_loop(sock, client, settings, cancel_loop).await;
        });

        Ok(Self {
            bind,
            cancel,
            _runner: runner,
        })
    }

    pub fn stop(&self) {
        self.cancel.cancel();
    }
}

fn build_client(settings: &DnsSettings) -> Result<reqwest::Client> {
    let mut b = reqwest::Client::builder()
        .http2_prior_knowledge()
        .timeout(Duration::from_secs(5))
        .pool_idle_timeout(Duration::from_secs(30))
        .user_agent("vpn21/1.0");
    if let Some(ip) = settings.doh_server_ip {
        // Parse the URL once so we can pin the host → IP.
        let url: url::Url = settings
            .doh_url
            .parse()
            .map_err(|e| Error::Dns(format!("doh_url parse: {e}")))?;
        let host = url
            .host_str()
            .ok_or_else(|| Error::Dns("doh_url: missing host".into()))?
            .to_owned();
        let port = url.port_or_known_default().unwrap_or(443);
        b = b.resolve(&host, SocketAddr::new(ip, port));
    } else if let Some(socks) = &settings.socks_upstream {
        let proxy =
            reqwest::Proxy::all(socks).map_err(|e| Error::Dns(format!("doh proxy: {e}")))?;
        b = b.proxy(proxy);
    }
    b.build()
        .map_err(|e| Error::Dns(format!("build client: {e}")))
}

async fn serve_loop(
    sock: Arc<UdpSocket>,
    client: reqwest::Client,
    settings: Arc<DnsSettings>,
    cancel: CancellationToken,
) {
    let mut buf = vec![0u8; 4096];
    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            r = sock.recv_from(&mut buf) => {
                let (n, peer) = match r {
                    Ok(x) => x,
                    Err(e) => { tracing::warn!(%e, "dns recv"); continue; }
                };
                let query = Bytes::copy_from_slice(&buf[..n]);
                let client = client.clone();
                let settings = settings.clone();
                let sock = sock.clone();
                tokio::spawn(async move {
                    match forward(query, &client, &settings.doh_url).await {
                        Ok(resp) => {
                            if let Err(e) = sock.send_to(&resp, peer).await {
                                tracing::warn!(%e, %peer, "dns reply send");
                            }
                        }
                        Err(e) => tracing::warn!(%e, %peer, "dns forward"),
                    }
                });
            }
        }
    }
    tracing::info!("dns server stopped");
}

async fn forward(query: Bytes, client: &reqwest::Client, doh_url: &str) -> Result<Vec<u8>> {
    // Sanity-check the question so we log meaningful diagnostics.
    if let Ok(msg) = Message::from_vec(&query) {
        if let Some(q) = msg.queries().first() {
            tracing::debug!(
                name = %q.name(),
                ty = ?q.query_type(),
                "dns forwarding"
            );
        }
    }
    let resp = client
        .post(doh_url)
        .header("Content-Type", "application/dns-message")
        .header("Accept", "application/dns-message")
        .body(query.to_vec())
        .send()
        .await
        .map_err(|e| Error::Dns(format!("doh post: {e}")))?;
    if !resp.status().is_success() {
        return Err(Error::Dns(format!("doh http status {}", resp.status())));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| Error::Dns(format!("doh body: {e}")))?;
    Ok(bytes.to_vec())
}
