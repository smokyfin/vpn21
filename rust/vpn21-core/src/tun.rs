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
        //!
        //! Sequence (see `sys/sys_domain.h`, `net/if_utun.h`):
        //!   1. `socket(PF_SYSTEM, SOCK_DGRAM, SYSPROTO_CONTROL)`
        //!   2. fill `ctl_info { ctl_name = "com.apple.net.utun_control" }`
        //!      and `ioctl(CTLIOCGINFO, &info)` to resolve `ctl_id`
        //!   3. `connect(sockaddr_ctl { sc_id = info.ctl_id, sc_unit = 0 })`
        //!      to let the kernel pick the next free `utunN` index
        //!   4. `getsockopt(UTUN_OPT_IFNAME)` so we can log the interface
        //!
        //! Requires root or the `network.extension` entitlement on a signed
        //! bundle.  On failure the orchestrator falls back to leaf's
        //! auto-mode, so the user still gets a working tunnel.
        use super::*;
        use crate::errors::Error;
        use std::mem::{size_of, zeroed};

        const PF_SYSTEM: libc::c_int = 32;
        const AF_SYSTEM: libc::c_uchar = 32;
        const SYSPROTO_CONTROL: libc::c_int = 2;
        const UTUN_CONTROL_NAME: &str = "com.apple.net.utun_control";
        const CTLIOCGINFO: libc::c_ulong = 0xc064_4e03; // _IOWR('N', 3, ctl_info)
        const UTUN_OPT_IFNAME: libc::c_int = 2;

        #[repr(C)]
        #[derive(Copy, Clone)]
        struct CtlInfo {
            ctl_id: u32,
            ctl_name: [libc::c_char; 96],
        }

        #[repr(C)]
        #[derive(Copy, Clone)]
        struct SockaddrCtl {
            sc_len: libc::c_uchar,
            sc_family: libc::c_uchar,
            ss_sysaddr: u16,
            sc_id: u32,
            sc_unit: u32,
            sc_reserved: [u32; 5],
        }

        pub(super) fn provision() -> Result<TunConfig> {
            let fd = unsafe { libc::socket(PF_SYSTEM, libc::SOCK_DGRAM, SYSPROTO_CONTROL) };
            if fd < 0 {
                return Err(Error::Tunnel(format!(
                    "socket(PF_SYSTEM): {}",
                    std::io::Error::last_os_error()
                )));
            }

            let mut info: CtlInfo = unsafe { zeroed() };
            for (i, b) in UTUN_CONTROL_NAME.bytes().enumerate() {
                info.ctl_name[i] = b as libc::c_char;
            }
            if unsafe { libc::ioctl(fd, CTLIOCGINFO, &mut info as *mut _) } < 0 {
                let err = std::io::Error::last_os_error();
                unsafe { libc::close(fd) };
                return Err(Error::Tunnel(format!("CTLIOCGINFO: {err}")));
            }

            let sc = SockaddrCtl {
                sc_len: size_of::<SockaddrCtl>() as u8,
                sc_family: AF_SYSTEM,
                ss_sysaddr: 2, // AF_SYS_CONTROL
                sc_id: info.ctl_id,
                sc_unit: 0, // kernel picks the next free utunN
                sc_reserved: [0; 5],
            };
            let rc = unsafe {
                libc::connect(
                    fd,
                    &sc as *const _ as *const libc::sockaddr,
                    size_of::<SockaddrCtl>() as u32,
                )
            };
            if rc < 0 {
                let err = std::io::Error::last_os_error();
                unsafe { libc::close(fd) };
                return Err(Error::Tunnel(format!(
                    "utun connect: {err} (hint: run as root or sign with the network-extension entitlement)"
                )));
            }

            let mut name = [0u8; 32];
            let mut name_len = name.len() as libc::socklen_t;
            let _ = unsafe {
                libc::getsockopt(
                    fd,
                    SYSPROTO_CONTROL,
                    UTUN_OPT_IFNAME,
                    name.as_mut_ptr() as *mut _,
                    &mut name_len,
                )
            };
            let ifname = std::str::from_utf8(&name[..name_len.saturating_sub(1) as usize])
                .unwrap_or("utun?");
            tracing::info!(fd, ifname, "macOS utun provisioned");

            Ok(TunConfig {
                fd,
                mtu: 1500,
                ipv4: Ipv4Addr::new(10, 19, 21, 1),
                ipv4_mask: 24,
                dns_listener_port: 53,
            })
        }
    }

    #[cfg(target_os = "windows")]
    mod windows {
        //! Windows TUN via wintun.
        //!
        //! leaf already supports wintun end-to-end: it loads `wintun.dll`
        //! itself and drives the adapter through the `tun` crate's Windows
        //! back-end.  Our job here is therefore just to:
        //!
        //!   1. locate a bundled `wintun.dll` (next to the binary, or via
        //!      `VPN21_WINTUN_PATH`),
        //!   2. sanity-check it opens,
        //!   3. return a sentinel [`TunConfig`] (`fd = -1`) that the
        //!      orchestrator translates into `tun_auto = true` with the
        //!      resolved DLL path for leaf.
        use super::*;
        use crate::errors::Error;

        pub(super) fn provision() -> Result<TunConfig> {
            let path = locate_wintun_dll().ok_or_else(|| {
                Error::Tunnel(
                    "wintun.dll not found (set VPN21_WINTUN_PATH or ship it next to the exe)"
                        .into(),
                )
            })?;
            // Try to dlopen to fail fast if the DLL is unusable.
            if unsafe { libloading::Library::new(&path) }.is_err() {
                return Err(Error::Tunnel(format!("failed to load {path}")));
            }
            tracing::info!(wintun_path = %path, "wintun.dll validated");
            Ok(TunConfig {
                fd: -1,
                mtu: 1500,
                ipv4: Ipv4Addr::new(10, 19, 21, 1),
                ipv4_mask: 24,
                dns_listener_port: 53,
            })
        }

        fn locate_wintun_dll() -> Option<String> {
            if let Ok(p) = std::env::var("VPN21_WINTUN_PATH") {
                if std::path::Path::new(&p).exists() {
                    return Some(p);
                }
            }
            if let Some(dir) = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            {
                let candidate = dir.join("wintun.dll");
                if candidate.exists() {
                    return Some(candidate.to_string_lossy().into_owned());
                }
            }
            if std::path::Path::new("wintun.dll").exists() {
                return Some("wintun.dll".to_string());
            }
            None
        }
    }
}
