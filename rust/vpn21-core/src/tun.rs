//! TUN interface adoption.
//!
//! We do **not** create the TUN ourselves — that would require platform-level
//! privileges we cannot easily obtain on iOS and Android.  Instead, the
//! platform code (PacketTunnelProvider on iOS, VpnService on Android, the
//! desktop helper on Windows / macOS / Linux) creates the TUN and hands us a
//! raw file descriptor.  We configure leaf #1 to adopt that fd.

use std::net::Ipv4Addr;

#[derive(Debug, Clone)]
pub struct TunConfig {
    /// File descriptor that the platform has already opened.  Owned by us
    /// after adoption — the platform must not touch it afterwards.
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
}
