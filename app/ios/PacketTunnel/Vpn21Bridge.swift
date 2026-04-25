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
        guard let appSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
            running = false
            return
        }
        let appDir = appSupport.appendingPathComponent("vpn21", isDirectory: true).path
        try? FileManager.default.createDirectory(atPath: appDir, withIntermediateDirectories: true)

        _ = appDir.withCString { dir in
            vpn21_init(dir, 1)
        }
        // Prime the iOS packet pump before handing the profile to the core
        // so no packets are dropped in the window between `vpn21_start` and
        // the first `readPackets` completion.
        vpn21_ios_pump_start()
        // The PacketTunnel extension owns the in-process packet flow rather
        // than a real fd, so we pass -1 and rely on the iOS pump (started
        // above) to ferry packets between Rust and `NEPacketTunnelFlow`.
        let res = profileJson.withCString { pjson in
            "10.19.21.2".withCString { ipv4 in
                vpn21_start_with_fd(pjson, -1, 1500, ipv4, 24, 53)
            }
        }
        if let res = res {
            vpn21_string_free(res)
        }
        PacketFlowHolder.shared.flow = packetFlow
        // Start the Swift-side pump loop: every `readPackets` completion
        // feeds each packet into Rust and drains any outbound packets the
        // core produced.  The loop stops itself when `stop()` clears the
        // packet-flow reference.
        pumpReadLoop()
    }

    static func stop() {
        vpn21_ios_pump_stop()
        if let res = vpn21_stop() {
            vpn21_string_free(res)
        }
        PacketFlowHolder.shared.flow = nil
        running = false
    }

    /// Drives `NEPacketTunnelFlow.readPackets` → Rust pump → writePackets.
    /// Re-arms itself until `PacketFlowHolder.shared.flow` goes nil.
    private static func pumpReadLoop() {
        guard let flow = PacketFlowHolder.shared.flow else { return }
        flow.readPackets { packets, protocols in
            for (idx, pkt) in packets.enumerated() {
                _ = pkt.withUnsafeBytes { (buf: UnsafeRawBufferPointer) -> Int32 in
                    guard let base = buf.baseAddress else { return -1 }
                    return vpn21_ios_pump_push_inbound(base.assumingMemoryBound(to: UInt8.self), buf.count)
                }
                _ = protocols[safe: idx] // silence unused warning in release
            }
            // Drain any packets the core produced back to the TUN.  4 MB is
            // more than enough for an NE callback's worth of packets.
            var scratch = [UInt8](repeating: 0, count: 4 * 1024 * 1024)
            let n = scratch.withUnsafeMutableBufferPointer { ptr -> Int in
                guard let base = ptr.baseAddress else { return 0 }
                let rc = vpn21_ios_pump_drain_outbound(base, ptr.count, 256)
                return rc < 0 ? 0 : Int(rc)
            }
            if n > 0 {
                var out: [Data] = []
                var numbers: [NSNumber] = []
                var off = 0
                while off + 2 <= n {
                    let len = Int(UInt16(scratch[off]) << 8 | UInt16(scratch[off + 1]))
                    off += 2
                    if off + len > n { break }
                    out.append(Data(scratch[off..<off + len]))
                    // IPv4 = AF_INET (2); iOS will re-classify anyway.
                    numbers.append(NSNumber(value: AF_INET))
                    off += len
                }
                if !out.isEmpty {
                    flow.writePackets(out, withProtocols: numbers)
                }
            }
            if PacketFlowHolder.shared.flow != nil {
                pumpReadLoop()
            }
        }
    }
}

private extension Array {
    subscript(safe i: Int) -> Element? { indices.contains(i) ? self[i] : nil }
}

final class PacketFlowHolder {
    static let shared = PacketFlowHolder()
    var flow: NEPacketTunnelFlow?
}
