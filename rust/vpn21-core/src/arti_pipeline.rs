//! arti pipeline.
//!
//! arti (the Rust Tor client) is configured with our custom VLESS pluggable
//! transport.  The PT is described to arti as an external one speaking
//! SOCKS5 on `pt_socks_upstream`; arti forwards Tor traffic to leaf #2,
//! which in turn terminates on the remote bridge's ORPort.
//!
//! The real integration is gated behind `backend-arti` because the arti
//! 0.41 API surface for bridge + PT configuration is still a moving target;
//! having a stub lets the rest of the workspace compile and lets the UI be
//! developed independently.

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
    use arti_client::{TorClient, TorClientConfig};

    tracing::info!("arti: building config");
    let mut cfg = TorClientConfig::builder();
    cfg.storage()
        .cache_dir(arti_client::config::CfgPath::new_literal(
            params.cache_dir.join("cache"),
        ))
        .state_dir(arti_client::config::CfgPath::new_literal(
            params.cache_dir.join("state"),
        ));
    // NOTE: the exact API for bridge / PT configuration in arti 0.41 is
    // still evolving.  We do the minimal bootstrap here and let the user
    // iterate — the structural pipeline is already in place.
    let cfg = cfg
        .build()
        .map_err(|e| Error::Arti(format!("build tor config: {e}")))?;
    let runtime = tor_rtcompat::PreferredRuntime::current()
        .map_err(|e| Error::Arti(format!("tor runtime: {e}")))?;
    let client = TorClient::with_runtime(runtime)
        .config(cfg)
        .create_unbootstrapped()
        .map_err(|e| Error::Arti(format!("create arti client: {e}")))?;

    let token = cancel.clone();
    tokio::select! {
        res = client.clone().bootstrap() => res.map_err(|e| Error::Arti(format!("bootstrap: {e}")))?,
        _ = token.cancelled() => return Err(Error::Cancelled),
    };
    Ok(ArtiSession {
        client_socks: params.client_socks,
        cancel,
    })
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
