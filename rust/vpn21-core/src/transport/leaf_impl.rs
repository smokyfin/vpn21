//! leaf-based provider.
//!
//! Given a parsed [`Profile`], we synthesise a leaf JSON config that exposes
//! a local SOCKS5 inbound (authenticated) and routes everything through a
//! VLESS outbound with the user's REALITY / gRPC settings, then hand that
//! config to [`leaf`] to run in-process.
//!
//! leaf is pulled from git with its *default* feature set; we do not cherry
//! pick individual protocols because the user wants room to later swap
//! trojan / shadowsocks / vmess in without rebuilding.
//!
//! This file always builds, but the real leaf integration is gated behind
//! the `backend-leaf` cargo feature — the upstream leaf API is not yet
//! stable and still moves between git revisions, so we isolate it here.

use std::sync::Arc;

use futures::future::BoxFuture;
use futures::FutureExt;
use serde_json::json;
use tokio_util::sync::CancellationToken;

use crate::config::Profile;
use crate::errors::{Error, Result};
use crate::secure_store::SocksEndpoint;

use super::{TransportCapabilities, TransportConfig, TransportHandle, TransportProvider};

#[derive(Default)]
pub struct LeafProvider {
    _priv: (),
}

impl LeafProvider {
    pub fn new() -> Self {
        Self::default()
    }

    /// Produces the leaf JSON config for a given session.  When `upstream`
    /// is `Some`, the outbound is a SOCKS5 chain; otherwise it is the
    /// profile's VLESS outbound.
    pub(crate) fn build_config(cfg: &TransportConfig, upstream: Option<&SocksEndpoint>) -> String {
        let mut inbounds = Vec::with_capacity(2);
        // SOCKS5 always listens so the local DNS server / apps can dial in.
        inbounds.push(json!({
            "protocol": "socks",
            "address": cfg.listen.host,
            "port": cfg.listen.port,
            "settings": {
                "auth": "password",
                "username": cfg.listen.user,
                "password": cfg.listen.pass,
                "udp": true
            }
        }));
        // TUN inbound: three possible provisioning modes.
        //   1) we already have an fd (Android, Linux self-opened)
        //   2) leaf should auto-create the TUN (macOS utun, Windows wintun)
        //   3) no TUN at all (iOS: packet-flow is pumped outside of leaf)
        let (addr_ip, addr_mask) =
            split_cidr(cfg.tun_address.as_deref().unwrap_or("10.19.21.1/24"));
        let netmask = mask_to_netmask(addr_mask);
        if let Some(fd) = cfg.tun_fd {
            inbounds.push(json!({
                "protocol": "tun",
                "settings": {
                    "fd": fd,
                    "address": addr_ip,
                    "netmask": netmask,
                    "gateway": addr_ip,
                    "mtu": 1500,
                    "tun2socks": "smoltcp",
                    "dnsServers": ["1.1.1.1", "1.0.0.1"]
                }
            }));
        } else if cfg.tun_auto {
            let mut settings = json!({
                "auto": true,
                "name": "vpn21",
                "address": addr_ip,
                "netmask": netmask,
                "gateway": addr_ip,
                "mtu": 1500,
                "tun2socks": "smoltcp",
                "dnsServers": ["1.1.1.1", "1.0.0.1"]
            });
            if let Some(w) = &cfg.wintun_path {
                settings["wintun"] = json!(w);
            }
            inbounds.push(json!({
                "protocol": "tun",
                "settings": settings
            }));
        }

        let outbound = match upstream {
            Some(up) => json!({
                "protocol": "socks",
                "tag": "socks-out",
                "settings": {
                    "address": up.host,
                    "port": up.port,
                    "username": up.user,
                    "password": up.pass
                }
            }),
            None => Self::build_vless_outbound(&cfg.profile),
        };

        json!({
            "log": { "level": "warn" },
            "inbounds": inbounds,
            "outbounds": [outbound, {"protocol": "direct", "tag": "direct"}],
            "router": { "rules": [] },
            "dns": { "servers": ["1.1.1.1", "1.0.0.1"] }
        })
        .to_string()
    }

    fn build_vless_outbound(profile: &Profile) -> serde_json::Value {
        let v = &profile.pt_outbound;
        let mut stream = json!({ "network": v.network });
        if v.network == "grpc" {
            stream["grpcSettings"] = json!({
                "serviceName": v.grpc_service_name,
                "multiMode": v.grpc_multi_mode
            });
        }
        match v.security.as_str() {
            "reality" => {
                if let Some(r) = v.reality.as_ref() {
                    stream["security"] = json!("reality");
                    stream["realitySettings"] = json!({
                        "serverName": r.server_name,
                        "publicKey": r.public_key,
                        "shortId": r.short_id,
                        "fingerprint": r.fingerprint
                    });
                } else {
                    stream["security"] = json!("none");
                }
            }
            "tls" => stream["security"] = json!("tls"),
            _ => stream["security"] = json!("none"),
        }
        json!({
            "protocol": "vless",
            "tag": "proxy",
            "settings": {
                "address": v.address,
                "port": v.port,
                "uuid": v.user_id,
                "flow": v.flow
            },
            "streamSettings": stream
        })
    }
}

fn split_cidr(cidr: &str) -> (String, u8) {
    match cidr.split_once('/') {
        Some((ip, mask)) => (ip.to_string(), mask.parse().unwrap_or(24)),
        None => (cidr.to_string(), 24),
    }
}

fn mask_to_netmask(bits: u8) -> String {
    let bits = bits.min(32);
    let mask: u32 = if bits == 0 {
        0
    } else {
        u32::MAX << (32 - bits)
    };
    std::net::Ipv4Addr::from(mask).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Profile;

    fn sample_cfg(tun_fd: Option<i32>, tun_auto: bool) -> TransportConfig {
        let raw = r#"{
            "bridge_rsa_id": "715213AEA5BBE71AB2E9E1AFEE02D0170206021F",
            "bridge_ed25519_id": "Rq4fdFNepS2oTFnyNrQon9FDWi46m5OFZKAGVFmMe9I",
            "doh_server": "https://dns.google/dns-query",
            "outbounds": [{"protocol":"vless","settings":{"vnext":[{"address":"1.2.3.4","port":443,"users":[{"id":"u"}]}]},"streamSettings":{"network":"grpc","grpcSettings":{"serviceName":"g"},"security":"reality","realitySettings":{"serverName":"s","publicKey":"p","shortId":"d","fingerprint":"f"}}}]
        }"#;
        let profile = Profile::from_json("id", "label", raw).unwrap();
        TransportConfig {
            profile,
            listen: crate::secure_store::SocksEndpoint {
                host: "127.0.0.1".into(),
                port: 1080,
                user: "u".into(),
                pass: "p".into(),
            },
            session_tag: "t".into(),
            tun_fd,
            tun_address: Some("10.19.21.1/24".into()),
            tun_auto,
            wintun_path: None,
        }
    }

    #[test]
    fn config_has_vless_outbound_with_reality() {
        let cfg = sample_cfg(None, false);
        let cfg_str = LeafProvider::build_config(&cfg, None);
        let v: serde_json::Value = serde_json::from_str(&cfg_str).unwrap();
        let ob = &v["outbounds"][0];
        assert_eq!(ob["protocol"], "vless");
        assert_eq!(ob["streamSettings"]["network"], "grpc");
        assert_eq!(ob["streamSettings"]["security"], "reality");
        assert_eq!(ob["streamSettings"]["realitySettings"]["serverName"], "s");
        assert_eq!(ob["streamSettings"]["grpcSettings"]["serviceName"], "g");
    }

    #[test]
    fn chained_config_uses_socks_outbound() {
        let cfg = sample_cfg(None, false);
        let up = crate::secure_store::SocksEndpoint {
            host: "127.0.0.1".into(),
            port: 2222,
            user: "uu".into(),
            pass: "pp".into(),
        };
        let cfg_str = LeafProvider::build_config(&cfg, Some(&up));
        let v: serde_json::Value = serde_json::from_str(&cfg_str).unwrap();
        let ob = &v["outbounds"][0];
        assert_eq!(ob["protocol"], "socks");
        assert_eq!(ob["settings"]["port"], 2222);
        assert_eq!(ob["settings"]["username"], "uu");
    }

    #[test]
    fn tun_inbound_fd_mode() {
        let cfg = sample_cfg(Some(7), false);
        let cfg_str = LeafProvider::build_config(&cfg, None);
        let v: serde_json::Value = serde_json::from_str(&cfg_str).unwrap();
        let tun = &v["inbounds"][1];
        assert_eq!(tun["protocol"], "tun");
        assert_eq!(tun["settings"]["fd"], 7);
        assert_eq!(tun["settings"]["netmask"], "255.255.255.0");
        assert!(tun["settings"].get("auto").is_none());
    }

    #[test]
    fn tun_inbound_auto_mode() {
        let mut cfg = sample_cfg(None, true);
        cfg.wintun_path = Some("C:\\wintun.dll".into());
        let cfg_str = LeafProvider::build_config(&cfg, None);
        let v: serde_json::Value = serde_json::from_str(&cfg_str).unwrap();
        let tun = &v["inbounds"][1];
        assert_eq!(tun["settings"]["auto"], true);
        assert_eq!(tun["settings"]["wintun"], "C:\\wintun.dll");
    }

    #[test]
    fn tun_inbound_absent_when_ios_style() {
        let cfg = sample_cfg(None, false);
        let cfg_str = LeafProvider::build_config(&cfg, None);
        let v: serde_json::Value = serde_json::from_str(&cfg_str).unwrap();
        assert!(v["inbounds"].as_array().unwrap().len() == 1);
    }

    #[test]
    fn mask_to_netmask_is_correct() {
        assert_eq!(mask_to_netmask(24), "255.255.255.0");
        assert_eq!(mask_to_netmask(16), "255.255.0.0");
        assert_eq!(mask_to_netmask(32), "255.255.255.255");
        assert_eq!(mask_to_netmask(0), "0.0.0.0");
    }
}

impl TransportProvider for LeafProvider {
    fn id(&self) -> &'static str {
        "leaf"
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            supports_vless: true,
            supports_trojan: true,
            supports_reality: true,
            supports_grpc: true,
            supports_ws: true,
            supports_tcp: true,
        }
    }

    fn start(
        self: Arc<Self>,
        cfg: TransportConfig,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<TransportHandle>> {
        async move {
            let config = Self::build_config(&cfg, None);
            spawn_leaf_runtime(config, cfg.listen.clone(), cancel).await
        }
        .boxed()
    }
}

/// Spawns a *second* leaf runtime whose outbound is a SOCKS5 chain to
/// `upstream` — used to route arti's PT SOCKS through the real VLESS tunnel.
pub async fn spawn_chained(
    cfg: TransportConfig,
    upstream: SocksEndpoint,
    cancel: CancellationToken,
) -> Result<TransportHandle> {
    let config = LeafProvider::build_config(&cfg, Some(&upstream));
    spawn_leaf_runtime(config, cfg.listen.clone(), cancel).await
}

#[cfg(feature = "backend-leaf")]
async fn spawn_leaf_runtime(
    config: String,
    listen: SocksEndpoint,
    cancel: CancellationToken,
) -> Result<TransportHandle> {
    use tokio::sync::oneshot;

    // Runtime ids in leaf are u16.  We derive one from a tiny counter so the
    // two leaf runtimes (TUN and PT) cannot collide within a process.
    use std::sync::atomic::{AtomicU16, Ordering};
    static NEXT_ID: AtomicU16 = AtomicU16::new(1);
    let rt_id = NEXT_ID.fetch_add(1, Ordering::SeqCst);

    let (stopped_tx, stopped_rx) = oneshot::channel::<Result<()>>();
    let config_clone = config.clone();
    std::thread::Builder::new()
        .name(format!("leaf-{rt_id}"))
        .spawn(move || {
            let opts = leaf::StartOptions {
                config: leaf::Config::Str(config_clone),
                runtime_opt: leaf::RuntimeOption::SingleThread,
                #[cfg(feature = "auto-reload")]
                auto_reload: false,
            };
            let res = leaf::start(rt_id, opts).map_err(|e| Error::Transport(e.to_string()));
            let _ = stopped_tx.send(res);
        })
        .map_err(|e| Error::Transport(format!("spawn leaf thread: {e}")))?;

    let cancel_for_task = cancel.clone();
    tokio::spawn(async move {
        cancel_for_task.cancelled().await;
        leaf::shutdown(rt_id);
    });

    let stop_fn = move || {
        async move {
            leaf::shutdown(rt_id);
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), stopped_rx).await;
            Ok::<_, Error>(())
        }
        .boxed()
    };
    Ok(TransportHandle::new("leaf", listen, stop_fn))
}

#[cfg(not(feature = "backend-leaf"))]
async fn spawn_leaf_runtime(
    _config: String,
    listen: SocksEndpoint,
    _cancel: CancellationToken,
) -> Result<TransportHandle> {
    tracing::warn!("leaf backend is disabled (enable `backend-leaf` feature for real datapath)");
    let stop_fn = || async move { Ok::<_, Error>(()) }.boxed();
    Ok(TransportHandle::new("leaf-stub", listen, stop_fn))
}
