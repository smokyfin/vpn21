import Foundation
import NetworkExtension

/// Thin Swift facade over `libvpn21.a`.  The real datapath lives in Rust; we
/// just forward packets between `NEPacketTunnelFlow` and the in-process
/// `TUN` buffer that `vpn21-core` exposes when built with
/// `feature="embedded-tun"`.
enum Vpn21Bridge {
    private static var running = false

    static func start(profileJson: String, packetFlow: NEPacketTunnelFlow) {
        guard !running else { return }
        running = true
        let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory())
        let appDir = appSupport.appendingPathComponent("vpn21", isDirectory: true).path
        try? FileManager.default.createDirectory(atPath: appDir, withIntermediateDirectories: true)

        _ = appDir.withCString { dir in
            vpn21_init(dir, 1)
        }
        let res = profileJson.withCString { pjson in
            "10.19.21.2".withCString { ipv4 in
                vpn21_start(pjson, -1, 1500, ipv4, 24, 53)
            }
        }
        if let res = res {
            vpn21_string_free(res)
        }
        // The packet pump is driven by Rust in `embedded-tun` mode; we only
        // have to keep a strong reference to `packetFlow` so NE doesn't
        // release it while we're still writing to it.
        PacketFlowHolder.shared.flow = packetFlow
    }

    static func stop() {
        if let res = vpn21_stop() {
            vpn21_string_free(res)
        }
        PacketFlowHolder.shared.flow = nil
        running = false
    }
}

final class PacketFlowHolder {
    static let shared = PacketFlowHolder()
    var flow: NEPacketTunnelFlow?
}
