//! Desktop TUN helpers — adapter layer on top of [`crate::tun::platform`].
//!
//! This module exists so the FFI / orchestrator can build a [`TunConfig`]
//! from raw values supplied by platform glue (fd from a helper binary, from
//! `vpn21_tun_provision`, or from a placeholder wintun handle).  The real
//! OS-specific provisioning (utun / `/dev/net/tun` / wintun) lives in
//! [`crate::tun::platform`] and is invoked directly from FFI.

use std::net::Ipv4Addr;

use crate::errors::{Error, Result};
use crate::tun::TunConfig;

/// Default TUN subnet used on desktop if the profile does not override.
pub const DEFAULT_TUN_IPV4: Ipv4Addr = Ipv4Addr::new(10, 19, 21, 1);
pub const DEFAULT_TUN_MASK: u8 = 24;
pub const DEFAULT_MTU: u16 = 1500;
pub const DEFAULT_DNS_PORT: u16 = 53;

/// Builds a [`TunConfig`] from the values returned by the desktop platform
/// channel.  `fd` may be -1 on systems (like macOS) where the TUN is owned
/// by the system extension and only a sentinel is returned, or on Windows
/// where leaf itself drives wintun through `wintun.dll`.
pub fn desktop_tun_config(
    fd: i32,
    mtu: Option<u16>,
    ipv4: Option<Ipv4Addr>,
    mask: Option<u8>,
    dns_port: Option<u16>,
) -> Result<TunConfig> {
    if fd < 0 && !cfg!(any(target_os = "macos", target_os = "windows")) {
        return Err(Error::Tunnel(format!(
            "desktop: invalid tun fd {fd} (sentinel only allowed on macOS/Windows)"
        )));
    }
    Ok(TunConfig {
        fd,
        mtu: mtu.unwrap_or(DEFAULT_MTU),
        ipv4: ipv4.unwrap_or(DEFAULT_TUN_IPV4),
        ipv4_mask: mask.unwrap_or(DEFAULT_TUN_MASK),
        dns_listener_port: dns_port.unwrap_or(DEFAULT_DNS_PORT),
    })
}
