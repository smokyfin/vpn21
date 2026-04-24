//! Pluggable transport abstraction.
//!
//! See [`docs/PLUGINS.md`] for the rationale.  Today only the [`leaf`] based
//! provider is wired in; tomorrow we can drop in xray-core or lyrebird with
//! no change to the orchestrator.

use std::sync::Arc;

use futures::future::BoxFuture;
use tokio_util::sync::CancellationToken;

use crate::config::Profile;
use crate::errors::Result;
use crate::secure_store::SocksEndpoint;

pub mod leaf_impl;
pub mod registry;

/// Capabilities a provider self-advertises so that the orchestrator can
/// decide whether it is suitable for a given profile.
#[derive(Debug, Clone, Default)]
pub struct TransportCapabilities {
    pub supports_vless: bool,
    pub supports_trojan: bool,
    pub supports_reality: bool,
    pub supports_grpc: bool,
    pub supports_ws: bool,
    pub supports_tcp: bool,
}

/// The configuration handed to a provider when the orchestrator asks it to
/// open a SOCKS5 tunnel.
#[derive(Debug, Clone)]
pub struct TransportConfig {
    pub profile: Profile,
    /// Where the provider must listen for incoming SOCKS5 traffic.  The
    /// orchestrator picks the port/credentials so every session is unique.
    pub listen: SocksEndpoint,
    /// A stable, per-session tag that the provider may use for logging.
    pub session_tag: String,
    /// When Some, the provider should also own a TUN inbound on this fd.
    /// Used by leaf #1 on Android/desktop, where the platform has handed us
    /// a file descriptor for the tunnel interface.  On iOS the packet-flow
    /// is pumped by the extension itself and this is always `None`.
    pub tun_fd: Option<i32>,
    /// IPv4 address assigned to the TUN interface (CIDR), when applicable.
    /// Example: `"10.19.21.1/24"`.
    pub tun_address: Option<String>,
}

/// An opaque handle returned by [`TransportProvider::start`].
pub struct TransportHandle {
    pub id: &'static str,
    pub listen: SocksEndpoint,
    stop: Box<dyn FnOnce() -> BoxFuture<'static, Result<()>> + Send>,
}

impl TransportHandle {
    pub fn new<F>(id: &'static str, listen: SocksEndpoint, stop: F) -> Self
    where
        F: FnOnce() -> BoxFuture<'static, Result<()>> + Send + 'static,
    {
        Self {
            id,
            listen,
            stop: Box::new(stop),
        }
    }

    pub async fn stop(self) -> Result<()> {
        (self.stop)().await
    }
}

pub trait TransportProvider: Send + Sync + 'static {
    fn id(&self) -> &'static str;
    fn capabilities(&self) -> TransportCapabilities;
    fn start(
        self: Arc<Self>,
        cfg: TransportConfig,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<TransportHandle>>;
}
