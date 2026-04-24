//! Desktop TUN helpers.
//!
//! On desktop platforms (Linux, macOS, Windows) the Dart side spawns a
//! privileged helper or uses admin APIs to create the TUN, then passes us the
//! resulting fd (Unix) or handle (Windows).  This module provides the
//! platform-specific constants and validation.

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
/// by the system extension and only a sentinel is returned.
pub fn desktop_tun_config(
    fd: i32,
    mtu: Option<u16>,
    ipv4: Option<Ipv4Addr>,
    mask: Option<u8>,
    dns_port: Option<u16>,
) -> Result<TunConfig> {
    if fd < 0 && !cfg!(target_os = "macos") {
        return Err(Error::Tunnel(format!(
            "desktop: invalid tun fd {fd} (only macOS allows sentinel fd)"
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

/// On Linux, creates a TUN device using the `tun` crate or raw `ioctl`.
/// This is a placeholder: the real implementation will use `TUNSETIFF`.
#[cfg(target_os = "linux")]
pub fn create_tun_linux(name: &str) -> Result<i32> {
    use std::fs::OpenOptions;
    use std::os::unix::io::IntoRawFd;

    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/net/tun")
        .map_err(|e| Error::Tunnel(format!("open /dev/net/tun: {e}")))?;
    let fd = file.into_raw_fd();
    tracing::info!(fd, name, "opened tun device (raw)");
    // The actual TUNSETIFF ioctl + route configuration is performed by the
    // desktop helper binary that runs with elevated privileges.
    Ok(fd)
}

/// Placeholder for macOS TUN creation.
#[cfg(target_os = "macos")]
pub fn create_tun_macos() -> Result<i32> {
    // On macOS the `utun` device is created via the system extension (or a
    // privileged helper).  We return -1 as a sentinel; the system extension
    // owns the actual datapath.
    tracing::info!("macOS: TUN owned by system extension, returning sentinel fd");
    Ok(-1)
}

/// Placeholder for Windows TUN creation.
#[cfg(target_os = "windows")]
pub fn create_tun_windows() -> Result<i32> {
    // On Windows we use WinTUN or a TAP-Windows adapter.  The driver handle
    // is obtained by the desktop helper and encoded as a raw fd for us.
    tracing::warn!("Windows TUN: not yet implemented");
    Err(Error::Tunnel("Windows TUN not yet implemented".into()))
}
