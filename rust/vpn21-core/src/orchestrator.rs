//! Session orchestrator.
//!
//! Owns the state machine that starts / stops the whole pipeline, and
//! translates external commands (from the FFI layer, i.e. the Flutter UI)
//! into coordinated calls into `transport`, `arti_pipeline`, `dns`.
//!
//! Cancellation is cooperative: every subsystem gets a
//! [`CancellationToken`].  `stop()` cancels the token and awaits all
//! subsystems with a 5 s budget; past that, the runtime forcefully drops
//! handles so the UI can never hang on "Disconnecting".

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

use crate::arti_pipeline::{ArtiSession, ArtiStartParams};
use crate::config::Profile;
use crate::dns::{DnsServer, DnsSettings};
use crate::errors::{Error, Result};
use crate::secure_store::{self, SocksEndpoint};
use crate::transport::{leaf_impl, registry, TransportConfig, TransportHandle};
use crate::tun::TunConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    Idle,
    Bootstrapping,
    Connecting,
    Connected,
    Disconnecting,
    Error,
}

#[derive(Debug, Clone)]
pub struct SessionStatus {
    pub state: SessionState,
    pub detail: String,
    /// 0..=100 — shown on the UI's progress ring.
    pub progress: u8,
}

struct Running {
    cancel: CancellationToken,
    arti: Option<ArtiSession>,
    leaf_chain: Option<TransportHandle>, // leaf #2 (PT driver)
    leaf_tun: Option<TransportHandle>,   // leaf #1 (TUN → SOCKS)
    dns: Option<DnsServer>,
}

pub struct Orchestrator {
    app_dir: PathBuf,
    status: Arc<Mutex<SessionStatus>>,
    running: Mutex<Option<Running>>,
}

impl Orchestrator {
    pub fn new(app_dir: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            app_dir,
            status: Arc::new(Mutex::new(SessionStatus {
                state: SessionState::Idle,
                detail: String::new(),
                progress: 0,
            })),
            running: Mutex::new(None),
        })
    }

    pub fn status(&self) -> SessionStatus {
        self.status.lock().clone()
    }

    fn set(&self, state: SessionState, detail: impl Into<String>, progress: u8) {
        let detail = detail.into();
        tracing::info!(?state, %detail, progress, "state transition");
        let mut s = self.status.lock();
        s.state = state;
        s.detail = detail;
        s.progress = progress;
    }

    /// Starts the full pipeline for `profile`, adopting `tun`.  Returns as
    /// soon as the pipeline is wired up; actual arti bootstrap progress is
    /// reported asynchronously via [`status`].
    pub async fn start(self: Arc<Self>, profile: Profile, tun: TunConfig) -> Result<()> {
        if self.running.lock().is_some() {
            return Err(Error::Tunnel("already running".into()));
        }

        self.set(SessionState::Bootstrapping, "preparing", 5);

        let cancel = CancellationToken::new();
        let session_tag = format!("s{}", rand::random::<u32>());

        // --- leaf #2 (PT driver): listens on SOCKS, outbound = VLESS ----
        let leaf_chain_listen = SocksEndpoint::new_loopback()?;
        let providers = registry::default_registry();
        let picked = registry::pick(
            &providers,
            "vless",
            &profile.pt_outbound.network,
            profile.pt_outbound.security == "reality",
        )
        .ok_or_else(|| Error::Transport("no provider supports profile".into()))?;
        let leaf_chain_cfg = TransportConfig {
            profile: profile.clone(),
            listen: leaf_chain_listen.clone(),
            session_tag: format!("{session_tag}-pt"),
            tun_fd: None,
            tun_address: None,
            tun_auto: false,
            wintun_path: None,
        };
        self.set(SessionState::Bootstrapping, "starting transport", 15);
        let leaf_chain = picked.clone().start(leaf_chain_cfg, cancel.clone()).await?;

        // --- arti -------------------------------------------------------
        let arti_client_socks = SocksEndpoint::new_loopback()?;
        let arti_params = ArtiStartParams {
            profile: profile.clone(),
            cache_dir: secure_store::arti_cache_dir(&self.app_dir),
            pt_socks_upstream: leaf_chain_listen.clone(),
            client_socks: arti_client_socks.clone(),
        };
        self.set(SessionState::Bootstrapping, "starting arti", 35);
        let arti = ArtiSession::start(arti_params, cancel.clone()).await?;

        // --- leaf #1 (TUN → SOCKS), upstream = arti --------------------
        let leaf_tun_listen = SocksEndpoint::new_loopback()?;
        let tun_fd_opt = if tun.fd >= 0 { Some(tun.fd) } else { None };
        // macOS + Windows: no fd, let leaf auto-provision utun/wintun.
        let tun_auto =
            tun_fd_opt.is_none() && (cfg!(target_os = "macos") || cfg!(target_os = "windows"));
        let wintun_path = locate_wintun();
        let leaf_tun_cfg = TransportConfig {
            profile: profile.clone(),
            listen: leaf_tun_listen.clone(),
            session_tag: format!("{session_tag}-tun"),
            tun_fd: tun_fd_opt,
            tun_address: Some(format!("{}/{}", tun.ipv4, tun.ipv4_mask)),
            tun_auto,
            wintun_path,
        };
        self.set(SessionState::Connecting, "starting tunnel", 60);
        let leaf_tun =
            leaf_impl::spawn_chained(leaf_tun_cfg, arti_client_socks.clone(), cancel.clone())
                .await?;

        // --- DNS --------------------------------------------------------
        self.set(SessionState::Connecting, "starting dns", 80);
        let dns_settings = DnsSettings {
            doh_url: profile.doh_server.clone(),
            doh_server_ip: profile.doh_server_ip,
            socks_upstream: Some(leaf_tun_listen.as_url()),
        };
        let dns = DnsServer::start(tun.dns_listener_addr(), dns_settings, cancel.clone()).await?;

        // --- TUN adoption ----------------------------------------------
        adopt_tun(&tun, &leaf_tun_listen)?;

        *self.running.lock() = Some(Running {
            cancel,
            arti: Some(arti),
            leaf_chain: Some(leaf_chain),
            leaf_tun: Some(leaf_tun),
            dns: Some(dns),
        });
        self.set(SessionState::Connected, "connected", 100);
        Ok(())
    }

    /// Stops the running session.  Cooperative cancel with a 5 s budget.
    pub async fn stop(self: Arc<Self>) -> Result<()> {
        let running = match self.running.lock().take() {
            Some(r) => r,
            None => return Ok(()),
        };
        self.set(SessionState::Disconnecting, "disconnecting", 50);
        running.cancel.cancel();
        if let Some(d) = &running.dns {
            d.stop();
        }

        // Drain subsystems in reverse startup order, each with its own budget.
        let deadline = Duration::from_secs(5);

        let (tx, rx) = oneshot::channel::<()>();
        let jh = tokio::spawn(async move {
            if let Some(h) = running.leaf_tun {
                let _ = h.stop().await;
            }
            if let Some(a) = running.arti {
                let _ = a.stop().await;
            }
            if let Some(h) = running.leaf_chain {
                let _ = h.stop().await;
            }
            let _ = tx.send(());
        });

        if tokio::time::timeout(deadline, rx).await.is_err() {
            tracing::warn!("stop: subsystems exceeded 5 s budget, aborting");
            jh.abort();
        }

        self.set(SessionState::Idle, "", 0);
        Ok(())
    }
}

fn adopt_tun(tun: &TunConfig, _upstream: &SocksEndpoint) -> Result<()> {
    // iOS owns the packet flow inside the PacketTunnelExtension, so a -1
    // here is explicitly expected; the datapath runs under `embedded-tun`.
    // macOS/Windows also accept -1 — leaf auto-provisions utun/wintun.
    let fd_ok = tun.fd >= 0
        || cfg!(feature = "embedded-tun")
        || cfg!(target_os = "macos")
        || cfg!(target_os = "windows");
    if !fd_ok {
        return Err(Error::Tunnel(format!("invalid tun fd {}", tun.fd)));
    }
    tracing::info!(
        fd = tun.fd,
        mtu = tun.mtu,
        ipv4 = %tun.ipv4,
        mask = tun.ipv4_mask,
        "adopting tun fd"
    );
    // leaf's inbound config consumes the `tun_fd` JSON key directly — we
    // emit it when building the leaf config for inbound=#1 and let the
    // runtime pick it up at start().  See `transport::leaf_impl::build_config`.
    Ok(())
}

/// Looks up a bundled `wintun.dll`.  Search order:
///   1) `VPN21_WINTUN_PATH` environment variable (user override)
///   2) next to the running binary
///   3) current working directory
///   4) system32 (bundled-with-system case)
fn locate_wintun() -> Option<String> {
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(p) = std::env::var("VPN21_WINTUN_PATH") {
            if std::path::Path::new(&p).exists() {
                return Some(p);
            }
        }
        let candidates = [
            std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.join("wintun.dll"))),
            Some(std::path::PathBuf::from("wintun.dll")),
            Some(std::path::PathBuf::from(
                "C:\\Windows\\System32\\wintun.dll",
            )),
        ];
        for c in candidates.into_iter().flatten() {
            if c.exists() {
                return Some(c.to_string_lossy().into_owned());
            }
        }
        tracing::warn!("wintun.dll not found; leaf tun auto-mode will fail on Windows");
        None
    }
}
