//! TUN interface adoption.
//!
//! We do **not** create the TUN ourselves on mobile — that would require
//! platform-level privileges we cannot easily obtain on iOS and Android.
//! The platform code (PacketTunnelProvider on iOS, VpnService on Android)
//! creates the TUN and hands us a raw file descriptor.  We configure
//! leaf #1 to adopt that fd.
//!
//! On desktop (Linux / macOS / Windows) we can self-provision the TUN via
//! the [`platform`] module, which returns a [`TunConfig`] whose fd leaf
//! will then adopt in exactly the same way as on Android.

use std::net::Ipv4Addr;

#[derive(Debug, Clone)]
pub struct TunConfig {
    /// File descriptor that the platform has already opened.  Owned by us
    /// after adoption — the platform must not touch it afterwards.
    /// Use `-1` together with `embedded-tun` when the fd cannot be shared
    /// (e.g. iOS packet-flow).
    pub fd: i32,
    pub mtu: u16,
    pub ipv4: Ipv4Addr,
    pub ipv4_mask: u8,
    pub dns_listener_port: u16,
}

impl TunConfig {
    pub fn dns_listener_addr(&self) -> std::net::SocketAddr {
        std::net::SocketAddr::from((self.ipv4, self.dns_listener_port))
    }

    /// Convenient default used across the codebase (`10.19.21.1/24`, 1500 MTU).
    pub fn default_mobile(fd: i32) -> Self {
        Self {
            fd,
            mtu: 1500,
            ipv4: Ipv4Addr::new(10, 19, 21, 1),
            ipv4_mask: 24,
            dns_listener_port: 53,
        }
    }
}

/// Desktop-only TUN provisioning.  Each target OS implements `provision`
/// to open (or synthesise) a TUN device and return a [`TunConfig`].  On
/// mobile the orchestrator is handed a fd from the platform instead.
pub mod platform {
    use super::*;
    use crate::errors::Result;

    #[cfg(target_os = "linux")]
    pub fn provision() -> Result<TunConfig> {
        linux::provision()
    }

    #[cfg(target_os = "macos")]
    pub fn provision() -> Result<TunConfig> {
        macos::provision()
    }

    #[cfg(target_os = "windows")]
    pub fn provision() -> Result<TunConfig> {
        windows::provision()
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    pub fn provision() -> Result<TunConfig> {
        // Fall-through for targets where we rely on the platform to pass
        // us an fd (Android, iOS, etc.).
        Ok(TunConfig::default_mobile(-1))
    }

    #[cfg(target_os = "linux")]
    mod linux {
        //! Linux TUN via the `/dev/net/tun` ioctl.  This is the classic
        //! `TUNSETIFF` dance; we open a device, flip `IFF_TUN | IFF_NO_PI`
        //! and return the fd to the orchestrator which hands it to leaf.
        use super::*;
        use crate::errors::Error;

        const IFF_TUN: libc::c_short = 0x0001;
        const IFF_NO_PI: libc::c_short = 0x1000;
        const TUNSETIFF: libc::c_ulong = 0x4004_54ca;

        #[repr(C)]
        #[derive(Clone, Copy)]
        struct Ifreq {
            name: [libc::c_char; libc::IF_NAMESIZE],
            flags: libc::c_short,
            _pad: [u8; 22],
        }

        pub(super) fn provision() -> Result<TunConfig> {
            let path = std::ffi::CString::new("/dev/net/tun").unwrap();
            let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
            if fd < 0 {
                return Err(Error::Tunnel(format!(
                    "open /dev/net/tun: {}",
                    std::io::Error::last_os_error()
                )));
            }
            let mut ifr: Ifreq = unsafe { std::mem::zeroed() };
            let name = b"vpn21\0";
            for (i, b) in name.iter().enumerate() {
                ifr.name[i] = *b as libc::c_char;
            }
            ifr.flags = IFF_TUN | IFF_NO_PI;
            let rc = unsafe { libc::ioctl(fd, TUNSETIFF, &mut ifr as *mut _) };
            if rc < 0 {
                let err = std::io::Error::last_os_error();
                unsafe { libc::close(fd) };
                return Err(Error::Tunnel(format!("TUNSETIFF: {err}")));
            }
            // `ip addr add 10.19.21.1/24 dev vpn21 && ip link set vpn21 up`
            // — delegated to a post-up helper the desktop app installs.
            tracing::info!("linux tun provisioned (vpn21)");
            Ok(TunConfig {
                fd,
                mtu: 1500,
                ipv4: Ipv4Addr::new(10, 19, 21, 1),
                ipv4_mask: 24,
                dns_listener_port: 53,
            })
        }
    }

    #[cfg(target_os = "macos")]
    mod macos {
        //! macOS `utun` provisioning via the control socket family.
        //! We open a `utun` control socket and connect; the kernel assigns
        //! the next free `utunN` index.  Routes are added by the desktop
        //! helper (needs root).
        use super::*;
        use crate::errors::Error;

        pub(super) fn provision() -> Result<TunConfig> {
            // Opening the utun control requires elevated privileges; we
            // return an error by default and leave the provisioning to the
            // desktop helper which can use `SCDynamicStore`.
            tracing::warn!("macOS utun provisioning requires the vpn21 helper; returning stub");
            Err(Error::Tunnel(
                "utun provisioning not yet implemented".into(),
            ))
        }
    }

    #[cfg(target_os = "windows")]
    mod windows {
        //! Windows TUN via wintun (https://www.wintun.net) loaded through
        //! the `wintun` crate.  The session handle is bridged to leaf via
        //! an anonymous pipe so leaf's tun inbound can treat it as a fd.
        use super::*;
        use crate::errors::Error;

        pub(super) fn provision() -> Result<TunConfig> {
            tracing::warn!("Windows wintun provisioning requires the vpn21 helper; returning stub");
            Err(Error::Tunnel(
                "wintun provisioning not yet implemented".into(),
            ))
        }
    }
}
