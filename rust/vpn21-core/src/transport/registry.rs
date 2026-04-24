//! Provider registry.  Order matters: the first provider whose capabilities
//! match a profile is picked.

use std::sync::Arc;

use super::leaf_impl::LeafProvider;
use super::TransportProvider;

pub fn default_registry() -> Vec<Arc<dyn TransportProvider>> {
    vec![Arc::new(LeafProvider::default()) as Arc<dyn TransportProvider>]
}

/// Finds the first provider that advertises support for `protocol`
/// ("vless" / "trojan" / ...) and `network` ("grpc" / "tcp" / "ws").
pub fn pick(
    providers: &[Arc<dyn TransportProvider>],
    protocol: &str,
    network: &str,
    reality: bool,
) -> Option<Arc<dyn TransportProvider>> {
    for p in providers {
        let caps = p.capabilities();
        let proto_ok = match protocol {
            "vless" => caps.supports_vless,
            "trojan" => caps.supports_trojan,
            _ => false,
        };
        let net_ok = match network {
            "grpc" => caps.supports_grpc,
            "tcp" => caps.supports_tcp,
            "ws" => caps.supports_ws,
            _ => true,
        };
        if proto_ok && net_ok && (!reality || caps.supports_reality) {
            return Some(p.clone());
        }
    }
    None
}
