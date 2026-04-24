import NetworkExtension
import os.log

/// PacketTunnelProvider is where the VPN actually runs on iOS.  It sets up
/// the TUN via `setTunnelNetworkSettings`, obtains the packetFlow fd through
/// a private API and hands it to the `vpn21-core` static library.
final class PacketTunnelProvider: NEPacketTunnelProvider {
    private let log = OSLog(subsystem: "com.vpn21.app", category: "pt")

    override func startTunnel(options: [String : NSObject]? = nil, completionHandler: @escaping (Error?) -> Void) {
        os_log("startTunnel", log: log, type: .info)

        let settings = NEPacketTunnelNetworkSettings(tunnelRemoteAddress: "10.19.21.1")
        settings.mtu = 1500
        let ipv4 = NEIPv4Settings(addresses: ["10.19.21.2"], subnetMasks: ["255.255.255.0"])
        ipv4.includedRoutes = [NEIPv4Route.default()]
        settings.ipv4Settings = ipv4
        let dns = NEDNSSettings(servers: ["10.19.21.1"])
        dns.matchDomains = [""]
        settings.dnsSettings = dns

        setTunnelNetworkSettings(settings) { [weak self] error in
            guard let self = self else { return }
            if let error = error { completionHandler(error); return }
            // Hand over the packet flow to the Rust core.  `vpn21_start`
            // expects a plain POSIX fd, but on iOS we must route packets
            // through `packetFlow.readPackets`.  The Rust side is compiled
            // with `feature="embedded-tun"` for iOS and loops over the flow.
            Vpn21Bridge.start(
                profileJson: (self.protocolConfiguration as? NETunnelProviderProtocol)?
                    .providerConfiguration?["profile"] as? String ?? "",
                packetFlow: self.packetFlow
            )
            completionHandler(nil)
        }
    }

    override func stopTunnel(with reason: NEProviderStopReason, completionHandler: @escaping () -> Void) {
        os_log("stopTunnel reason=%{public}@", log: log, type: .info, String(describing: reason))
        Vpn21Bridge.stop()
        completionHandler()
    }
}
