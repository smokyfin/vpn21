//! iOS packet-pump FFI skeleton.
//!
//! On iOS the kernel owns the TUN: all traffic is delivered as IP packets
//! through `NEPacketTunnelFlow.readPackets` and written back with
//! `writePackets`.  Unlike every other platform we *cannot* hand a raw
//! file-descriptor to leaf — NEPacketTunnelFlow is a Foundation object
//! with no `fd` back-door.
//!
//! This module provides the Rust half of the bridge that lets the Swift
//! packet-tunnel extension hand packets to the core.  The long-term plan
//! is an in-process tun2socks that parses IP/TCP and dials arti's SOCKS5
//! inbound per-flow; until that lands we expose a minimal byte-pipe that
//! Swift can already drive so the full end-to-end path (read → enqueue →
//! dial arti → reply → writePackets) can be implemented incrementally and
//! tested against a real VLESS bridge.
//!
//! The queue is process-global (there is only ever one packet tunnel
//! alive) and bounded so a misbehaving Swift pump cannot exhaust memory.

use parking_lot::Mutex;
use std::collections::VecDeque;
use std::sync::Arc;

/// Hard cap on the number of in-flight packet buffers.  NEPacketTunnelFlow
/// already applies back-pressure, so this is a safety net rather than a
/// tuning knob.  At 1500-byte MTU this bounds memory at ~6 MB.
const MAX_QUEUE_LEN: usize = 4096;

#[derive(Default)]
struct PumpInner {
    /// Packets read from `NEPacketTunnelFlow` → core.
    inbound: VecDeque<Vec<u8>>,
    /// Packets produced by the core → `NEPacketTunnelFlow.writePackets`.
    outbound: VecDeque<Vec<u8>>,
    /// Set to `true` while a session is active.
    running: bool,
    /// Monotonic counters for lightweight observability.
    rx_packets: u64,
    tx_packets: u64,
    dropped_inbound: u64,
    dropped_outbound: u64,
}

#[derive(Clone, Default)]
pub struct IosPump {
    inner: Arc<Mutex<PumpInner>>,
}

static PUMP: once_cell::sync::Lazy<IosPump> = once_cell::sync::Lazy::new(IosPump::default);

/// Returns the process-wide iOS pump.
pub fn global() -> IosPump {
    PUMP.clone()
}

impl IosPump {
    pub fn start(&self) {
        let mut g = self.inner.lock();
        g.running = true;
        g.inbound.clear();
        g.outbound.clear();
    }

    pub fn stop(&self) {
        let mut g = self.inner.lock();
        g.running = false;
        g.inbound.clear();
        g.outbound.clear();
    }

    /// Called by Swift for each IP packet read from the TUN.  Returns
    /// `false` if the pump is stopped or the queue is full — Swift should
    /// then drop the packet and keep pumping.
    pub fn push_inbound(&self, pkt: Vec<u8>) -> bool {
        let mut g = self.inner.lock();
        if !g.running {
            return false;
        }
        if g.inbound.len() >= MAX_QUEUE_LEN {
            g.dropped_inbound = g.dropped_inbound.saturating_add(1);
            return false;
        }
        g.rx_packets = g.rx_packets.saturating_add(1);
        g.inbound.push_back(pkt);
        true
    }

    /// Called by the core to push a response packet back onto the TUN.
    pub fn push_outbound(&self, pkt: Vec<u8>) -> bool {
        let mut g = self.inner.lock();
        if !g.running {
            return false;
        }
        if g.outbound.len() >= MAX_QUEUE_LEN {
            g.dropped_outbound = g.dropped_outbound.saturating_add(1);
            return false;
        }
        g.tx_packets = g.tx_packets.saturating_add(1);
        g.outbound.push_back(pkt);
        true
    }

    /// Called by Swift on every `writePackets` completion to drain queued
    /// packets destined for the TUN.  Returns an empty vec when the queue
    /// is empty — Swift should re-arm the read loop.
    pub fn drain_outbound(&self, max: usize) -> Vec<Vec<u8>> {
        let mut g = self.inner.lock();
        let n = g.outbound.len().min(max);
        g.outbound.drain(..n).collect()
    }

    /// Statistics snapshot for the logs tab.
    pub fn stats(&self) -> IosPumpStats {
        let g = self.inner.lock();
        IosPumpStats {
            running: g.running,
            inbound_queued: g.inbound.len(),
            outbound_queued: g.outbound.len(),
            rx_packets: g.rx_packets,
            tx_packets: g.tx_packets,
            dropped_inbound: g.dropped_inbound,
            dropped_outbound: g.dropped_outbound,
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct IosPumpStats {
    pub running: bool,
    pub inbound_queued: usize,
    pub outbound_queued: usize,
    pub rx_packets: u64,
    pub tx_packets: u64,
    pub dropped_inbound: u64,
    pub dropped_outbound: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queues_respect_running_flag() {
        let p = IosPump::default();
        assert!(!p.push_inbound(vec![1, 2, 3]));
        p.start();
        assert!(p.push_inbound(vec![1, 2, 3]));
        p.stop();
        assert!(!p.push_inbound(vec![4]));
        // After stop, queues are cleared so drain is empty.
        p.start();
        assert!(p.drain_outbound(8).is_empty());
    }

    #[test]
    fn outbound_drain_returns_fifo_order() {
        let p = IosPump::default();
        p.start();
        for i in 0..5u8 {
            assert!(p.push_outbound(vec![i]));
        }
        let got = p.drain_outbound(3);
        assert_eq!(got, vec![vec![0u8], vec![1], vec![2]]);
        let got = p.drain_outbound(10);
        assert_eq!(got, vec![vec![3u8], vec![4]]);
    }

    #[test]
    fn queue_has_hard_cap() {
        let p = IosPump::default();
        p.start();
        // Push beyond the cap; the extras must be rejected.
        for _ in 0..MAX_QUEUE_LEN {
            assert!(p.push_inbound(vec![0]));
        }
        assert!(!p.push_inbound(vec![0]));
        let stats = p.stats();
        assert!(stats.dropped_inbound >= 1);
        assert_eq!(stats.inbound_queued, MAX_QUEUE_LEN);
    }
}
