//! Profile parsing.
//!
//! The Flutter UI hands us a blob that is usually the Xray-style JSON a user
//! pasted, fetched from a URL or scanned from a QR code.  From that blob we
//! only consume a small, fixed set of fields:
//!
//! * `bridge_rsa_id` — the Tor bridge's RSA fingerprint.
//! * `bridge_ed25519_id` — the Tor bridge's Ed25519 identity key.
//! * `doh_server` — the DoH URL used by our DNS resolver.
//! * `doh_server_ip` (optional) — an IP to dial directly, bypassing the
//!   system resolver entirely.
//! * `outbounds[].protocol == "vless"` — the VLESS/REALITY/gRPC parameters.
//!
//! All other top-level keys (routing, inbounds, logs) are ignored — they
//! belong to the legacy Xray world and have no meaning in our pipeline.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use url::Url;

use crate::errors::{Error, Result};

/// The parsed, normalised representation used everywhere inside the core.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Profile {
    pub id: String,
    pub label: String,

    pub bridge_rsa_id: String,
    pub bridge_ed25519_id: String,

    pub doh_server: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doh_server_ip: Option<IpAddr>,

    /// The PT outbound that arti will tunnel over.
    pub pt_outbound: VlessOutbound,

    /// Raw outbounds — provider implementations may consume them directly if
    /// they want to support extras like `direct`/`block`.
    pub raw_outbounds: Vec<serde_json::Value>,

    /// Preferred exit country (ISO 3166-1 alpha-2) selected by the user from
    /// the UI; "" means "let Tor choose".
    #[serde(default)]
    pub exit_country: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VlessOutbound {
    pub address: String,
    pub port: u16,
    pub user_id: String,
    #[serde(default)]
    pub flow: String,
    pub network: String, // "grpc", "tcp", "ws", ...
    #[serde(default)]
    pub grpc_service_name: String,
    #[serde(default)]
    pub grpc_multi_mode: bool,
    pub security: String, // "reality", "tls", "none"
    #[serde(default)]
    pub reality: Option<RealitySettings>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RealitySettings {
    pub server_name: String,
    pub public_key: String,
    pub short_id: String,
    #[serde(default)]
    pub fingerprint: String,
}

impl Profile {
    /// Parses a raw JSON blob (Xray-style) into a [`Profile`].  Missing
    /// optional values fall back to sane defaults.  Missing required values
    /// produce an [`Error::InvalidProfile`].
    pub fn from_json(id: impl Into<String>, label: impl Into<String>, raw: &str) -> Result<Self> {
        let v: serde_json::Value = serde_json::from_str(raw)?;
        let bridge_rsa_id = v
            .get("bridge_rsa_id")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::InvalidProfile("missing bridge_rsa_id".into()))?
            .to_owned();
        let bridge_ed25519_id = v
            .get("bridge_ed25519_id")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::InvalidProfile("missing bridge_ed25519_id".into()))?
            .to_owned();
        let doh_server = v
            .get("doh_server")
            .and_then(|x| x.as_str())
            .ok_or_else(|| Error::InvalidProfile("missing doh_server".into()))?
            .to_owned();

        // Fail fast on malformed DoH URLs so we never ship a profile that
        // silently bypasses DoH at runtime.
        let _ = Url::parse(&doh_server)
            .map_err(|e| Error::InvalidProfile(format!("doh_server: {e}")))?;

        let doh_server_ip = v
            .get("doh_server_ip")
            .and_then(|x| x.as_str())
            .map(|s| s.parse::<IpAddr>())
            .transpose()
            .map_err(|e| Error::InvalidProfile(format!("doh_server_ip: {e}")))?;

        let outbounds: Vec<serde_json::Value> = v
            .get("outbounds")
            .and_then(|x| x.as_array())
            .cloned()
            .unwrap_or_default();
        if outbounds.is_empty() {
            return Err(Error::InvalidProfile("empty outbounds".into()));
        }

        let pt = outbounds
            .iter()
            .find(|o| {
                o.get("protocol").and_then(|x| x.as_str()) == Some("vless")
                    && o.get("tag").and_then(|x| x.as_str()) != Some("direct")
            })
            .ok_or_else(|| Error::InvalidProfile("no vless outbound".into()))?;

        let pt_outbound = parse_vless(pt)?;

        Ok(Self {
            id: id.into(),
            label: label.into(),
            bridge_rsa_id,
            bridge_ed25519_id,
            doh_server,
            doh_server_ip,
            pt_outbound,
            raw_outbounds: outbounds,
            exit_country: String::new(),
        })
    }

    pub fn with_exit_country(mut self, cc: impl Into<String>) -> Self {
        self.exit_country = cc.into();
        self
    }
}

fn parse_vless(o: &serde_json::Value) -> Result<VlessOutbound> {
    let settings = o
        .get("settings")
        .ok_or_else(|| Error::InvalidProfile("vless: missing settings".into()))?;
    let vnext = settings
        .get("vnext")
        .and_then(|x| x.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| Error::InvalidProfile("vless: missing vnext".into()))?;

    let address = vnext
        .get("address")
        .and_then(|x| x.as_str())
        .ok_or_else(|| Error::InvalidProfile("vless: missing address".into()))?
        .to_owned();
    let port = vnext
        .get("port")
        .and_then(|x| x.as_u64())
        .ok_or_else(|| Error::InvalidProfile("vless: missing port".into()))? as u16;

    let user = vnext
        .get("users")
        .and_then(|x| x.as_array())
        .and_then(|a| a.first())
        .ok_or_else(|| Error::InvalidProfile("vless: missing users".into()))?;
    let user_id = user
        .get("id")
        .and_then(|x| x.as_str())
        .ok_or_else(|| Error::InvalidProfile("vless: missing user id".into()))?
        .to_owned();
    let flow = user
        .get("flow")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_owned();

    let stream = o.get("streamSettings").cloned().unwrap_or_default();
    let network = stream
        .get("network")
        .and_then(|x| x.as_str())
        .unwrap_or("tcp")
        .to_owned();
    let grpc_service_name = stream
        .get("grpcSettings")
        .and_then(|x| x.get("serviceName"))
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_owned();
    let grpc_multi_mode = stream
        .get("grpcSettings")
        .and_then(|x| x.get("mode"))
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let security = stream
        .get("security")
        .and_then(|x| x.as_str())
        .unwrap_or("none")
        .to_owned();

    let reality = if security == "reality" {
        let r = stream.get("realitySettings").ok_or_else(|| {
            Error::InvalidProfile("vless: security=reality but no realitySettings".into())
        })?;
        Some(RealitySettings {
            server_name: r
                .get("serverName")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_owned(),
            public_key: r
                .get("publicKey")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_owned(),
            short_id: r
                .get("shortId")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_owned(),
            fingerprint: r
                .get("fingerprint")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_owned(),
        })
    } else {
        None
    };

    Ok(VlessOutbound {
        address,
        port,
        user_id,
        flow,
        network,
        grpc_service_name,
        grpc_multi_mode,
        security,
        reality,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../../../docs/sample_profile.json");

    #[test]
    fn parses_sample_profile() {
        let p = Profile::from_json("s", "sample", SAMPLE).expect("parse");
        assert_eq!(p.pt_outbound.network, "grpc");
        assert_eq!(p.pt_outbound.security, "reality");
        assert_eq!(p.pt_outbound.port, 8090);
        assert_eq!(p.doh_server, "https://dns.google/dns-query");
        assert!(p.pt_outbound.reality.is_some());
        let r = p.pt_outbound.reality.unwrap();
        assert_eq!(r.server_name, "urentbike.ru");
        assert_eq!(r.short_id, "6b2f4e6ac9b1d2f0");
    }

    #[test]
    fn rejects_missing_doh() {
        let raw = r#"{"bridge_rsa_id":"x","bridge_ed25519_id":"y","outbounds":[]}"#;
        assert!(Profile::from_json("a", "b", raw).is_err());
    }

    #[test]
    fn rejects_invalid_doh_url() {
        let raw = r#"{"bridge_rsa_id":"x","bridge_ed25519_id":"y","doh_server":"not a url","outbounds":[]}"#;
        let err = Profile::from_json("a", "b", raw).unwrap_err().to_string();
        assert!(err.contains("doh_server"), "got: {err}");
    }

    #[test]
    fn accepts_doh_server_ip() {
        let raw = r#"{"bridge_rsa_id":"x","bridge_ed25519_id":"y","doh_server":"https://dns.google/dns-query","doh_server_ip":"8.8.8.8","outbounds":[{"protocol":"vless","settings":{"vnext":[{"address":"1.2.3.4","port":443,"users":[{"id":"u"}]}]},"streamSettings":{"network":"grpc","grpcSettings":{"serviceName":"g"},"security":"none"}}]}"#;
        let p = Profile::from_json("a", "b", raw).unwrap();
        assert_eq!(p.doh_server_ip.unwrap().to_string(), "8.8.8.8");
    }

    #[test]
    fn rejects_invalid_doh_server_ip() {
        let raw = r#"{"bridge_rsa_id":"x","bridge_ed25519_id":"y","doh_server":"https://dns.google/dns-query","doh_server_ip":"not-an-ip","outbounds":[]}"#;
        let err = Profile::from_json("a", "b", raw).unwrap_err().to_string();
        assert!(err.contains("doh_server_ip"), "got: {err}");
    }

    #[test]
    fn skips_direct_outbound() {
        // When multiple outbounds are present, we must pick the vless one
        // and ignore `direct`/`block` that Xray profiles often ship with.
        let raw = r#"{
            "bridge_rsa_id": "x", "bridge_ed25519_id": "y",
            "doh_server": "https://dns.google/dns-query",
            "outbounds": [
                {"protocol":"freedom","tag":"direct"},
                {"protocol":"blackhole","tag":"block"},
                {"protocol":"vless","settings":{"vnext":[{"address":"1.2.3.4","port":443,"users":[{"id":"u"}]}]},"streamSettings":{"network":"tcp","security":"none"}}
            ]
        }"#;
        let p = Profile::from_json("a", "b", raw).unwrap();
        assert_eq!(p.pt_outbound.port, 443);
        assert_eq!(p.pt_outbound.user_id, "u");
    }
}
