//! vpn21 core library.
//!
//! This crate is the single source of truth for the VPN datapath. It is
//! consumed from Flutter via a small C-ABI FFI layer (see [`ffi`]), and
//! from Kotlin / Swift via the same FFI.
//!
//! The public surface here is intentionally tiny:
//!
//! * [`init`] — called once at application start-up.
//! * [`orchestrator::Orchestrator`] — session management facade used by
//!   platform code.
//! * [`config::Profile`] — parsed profile loaded from URL / QR / paste.
//! * [`logging`] — ring-buffered log tail exposed to the UI.

#![deny(clippy::dbg_macro)]
#![allow(clippy::needless_return)]

pub mod config;
pub mod dns;
pub mod errors;
pub mod ffi;
pub mod logging;
pub mod orchestrator;
pub mod secure_store;
pub mod transport;

mod arti_pipeline;
mod tun;
pub mod tun_desktop;

pub use errors::{Error, Result};

use once_cell::sync::OnceCell;
use std::path::PathBuf;

/// Application-wide initialisation parameters supplied by the platform code.
#[derive(Debug, Clone)]
pub struct InitOptions {
    /// App-private directory where we may create `profiles/`, `runtime/`,
    /// `cache/` and `logs/`.  Must survive across launches and must not be
    /// readable by other apps (enforced at platform level).
    pub app_dir: PathBuf,
    /// Verbose logging toggle.  The Flutter settings screen controls this.
    pub verbose: bool,
}

static INIT: OnceCell<()> = OnceCell::new();

/// One-shot initialisation: installs logging, default rustls provider and
/// ensures the on-disk layout under `app_dir` exists.
pub fn init(opts: InitOptions) -> Result<()> {
    INIT.get_or_try_init::<_, Error>(|| {
        logging::install(opts.verbose)?;
        // Install a process-wide rustls provider early so subsequent libraries
        // (arti, reqwest, leaf) do not race on their own default.
        let _ = rustls::crypto::ring::default_provider().install_default();
        secure_store::ensure_layout(&opts.app_dir)?;
        tracing::info!(app_dir = %opts.app_dir.display(), "vpn21 core initialised");
        Ok(())
    })
    .map(|_| ())
}
