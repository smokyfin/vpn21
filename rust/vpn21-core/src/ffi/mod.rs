//! C-ABI surface for Flutter / Kotlin / Swift.
//!
//! The Flutter side talks to us via `dart:ffi`; Kotlin and Swift reuse the
//! same symbols.  We deliberately expose a tiny surface — strings in / JSON
//! out — so we never have to regenerate platform bindings when we change an
//! internal struct.

use std::ffi::{c_char, c_int, CStr, CString};
use std::path::PathBuf;
use std::sync::Arc;

use once_cell::sync::OnceCell;

use crate::config::Profile;
use crate::logging;
use crate::orchestrator::{Orchestrator, SessionState, SessionStatus};
use crate::tun::TunConfig;
use crate::{init as core_init, InitOptions};

mod runtime;

use runtime::runtime;

static ORCH: OnceCell<Arc<Orchestrator>> = OnceCell::new();

#[allow(clippy::missing_safety_doc)]
#[no_mangle]
pub unsafe extern "C" fn vpn21_init(app_dir: *const c_char, verbose: c_int) -> c_int {
    let app_dir = match cstr_to_pathbuf(app_dir) {
        Ok(p) => p,
        Err(_) => return -1,
    };
    match core_init(InitOptions {
        app_dir: app_dir.clone(),
        verbose: verbose != 0,
    }) {
        Ok(()) => {
            let _ = ORCH.set(Orchestrator::new(app_dir));
            0
        }
        Err(e) => {
            tracing::error!(%e, "vpn21_init failed");
            -1
        }
    }
}

/// Parse a raw Xray-style profile JSON into a validated internal form.  The
/// result is returned as a JSON string to avoid multi-FFI-call boundaries.
///
/// Ownership: the returned C-string is allocated with [`CString`] and must
/// be freed with [`vpn21_string_free`].
#[allow(clippy::missing_safety_doc)]
#[no_mangle]
pub unsafe extern "C" fn vpn21_profile_parse(
    id: *const c_char,
    label: *const c_char,
    json: *const c_char,
) -> *mut c_char {
    let id = cstr(id).unwrap_or_default();
    let label = cstr(label).unwrap_or_default();
    let raw = match cstr(json) {
        Ok(s) => s,
        Err(_) => return CString::new("").unwrap().into_raw(),
    };
    let response = match Profile::from_json(&id, &label, &raw) {
        Ok(p) => serde_json::json!({ "ok": true, "profile": p }),
        Err(e) => serde_json::json!({ "ok": false, "error": e.to_string() }),
    };
    CString::new(response.to_string())
        .unwrap_or_else(|_| CString::new("{}").unwrap())
        .into_raw()
}

/// Starts the VPN session.  `profile_json` is a serialised [`Profile`] (the
/// object returned by [`vpn21_profile_parse`]).  `tun_fd`, `mtu`, `ipv4`,
/// `mask`, `dns_port` describe the already-opened TUN.
#[allow(clippy::missing_safety_doc)]
#[no_mangle]
pub unsafe extern "C" fn vpn21_start(
    profile_json: *const c_char,
    tun_fd: c_int,
    mtu: c_int,
    ipv4: *const c_char,
    mask: c_int,
    dns_port: c_int,
) -> *mut c_char {
    let Some(orch) = ORCH.get().cloned() else {
        return err("not initialised");
    };
    let profile_json = match cstr(profile_json) {
        Ok(s) => s,
        Err(_) => return err("bad profile_json"),
    };
    let profile: Profile = match serde_json::from_str(&profile_json) {
        Ok(p) => p,
        Err(e) => return err(&format!("profile parse: {e}")),
    };
    let ipv4 = match cstr(ipv4) {
        Ok(s) => match s.parse::<std::net::Ipv4Addr>() {
            Ok(v) => v,
            Err(e) => return err(&format!("ipv4: {e}")),
        },
        Err(_) => return err("ipv4 missing"),
    };
    let tun = TunConfig {
        fd: tun_fd,
        mtu: mtu as u16,
        ipv4,
        ipv4_mask: mask as u8,
        dns_listener_port: dns_port as u16,
    };
    let rt = runtime();
    let res = rt.block_on(async move { orch.start(profile, tun).await });
    match res {
        Ok(()) => ok("started"),
        Err(e) => err(&e.to_string()),
    }
}

#[no_mangle]
pub extern "C" fn vpn21_stop() -> *mut c_char {
    let Some(orch) = ORCH.get().cloned() else {
        return err("not initialised");
    };
    let rt = runtime();
    match rt.block_on(async move { orch.stop().await }) {
        Ok(()) => ok("stopped"),
        Err(e) => err(&e.to_string()),
    }
}

#[no_mangle]
pub extern "C" fn vpn21_status() -> *mut c_char {
    let Some(orch) = ORCH.get().cloned() else {
        return err("not initialised");
    };
    let s: SessionStatus = orch.status();
    let payload = serde_json::json!({
        "state": match s.state {
            SessionState::Idle => "idle",
            SessionState::Bootstrapping => "bootstrapping",
            SessionState::Connecting => "connecting",
            SessionState::Connected => "connected",
            SessionState::Disconnecting => "disconnecting",
            SessionState::Error => "error",
        },
        "detail": s.detail,
        "progress": s.progress,
    });
    cstr_out(payload.to_string())
}

#[no_mangle]
pub extern "C" fn vpn21_logs(limit: c_int) -> *mut c_char {
    let lim = limit.max(1) as usize;
    let entries = logging::tail(lim);
    let v: Vec<_> = entries
        .into_iter()
        .map(|e| {
            serde_json::json!({
                "ts_ms": e.timestamp_ms,
                "level": e.level,
                "msg": e.message,
            })
        })
        .collect();
    cstr_out(serde_json::json!({ "entries": v }).to_string())
}

#[no_mangle]
pub extern "C" fn vpn21_logs_clear() {
    logging::clear();
}

/// Desktop-only: provision a TUN interface on the host OS and return a
/// JSON descriptor (`{ "ok": true, "tun": { "fd": ..., "mtu": ..., ...} }`)
/// that Flutter can feed back into [`vpn21_start`].  On mobile this is a
/// no-op that returns `{"ok": false, "error": "unsupported"}`.
#[no_mangle]
pub extern "C" fn vpn21_tun_provision() -> *mut c_char {
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
        match crate::tun::platform::provision() {
            Ok(cfg) => cstr_out(
                serde_json::json!({
                    "ok": true,
                    // Field names match the Android/iOS platform channel
                    // (camelCase) so the Dart `requestTun` consumer can use
                    // a single code path across every platform.
                    "tun": {
                        "fd": cfg.fd,
                        "mtu": cfg.mtu,
                        "ipv4": cfg.ipv4.to_string(),
                        "mask": cfg.ipv4_mask,
                        "dnsPort": cfg.dns_listener_port,
                    }
                })
                .to_string(),
            ),
            Err(e) => err(&e.to_string()),
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        err("unsupported on this platform")
    }
}

// ---------------- iOS packet pump -----------------------------------------
//
// Thin FFI around [`crate::ios_pump::IosPump`] so the Swift packet-tunnel
// extension can push packets read from `NEPacketTunnelFlow` into the core
// and drain outbound packets that the core produced.  Memory ownership:
// for `push_inbound` the buffer is copied out of the C pointer and owned
// by Rust; for `drain_outbound` Rust writes at most `cap` bytes into the
// caller-supplied buffer and returns the number of bytes written, with the
// exact packet boundary encoded as a single leading length-prefixed TLV:
//
//   [u16 big-endian length][packet bytes]...
//
// so Swift can re-assemble the original packet boundaries without a second
// FFI call per packet.  The `max_packets` argument bounds how many packets
// we will encode in one batch.
//
// These functions are cheap and lock-free *except* for a single mutex
// held for the duration of the copy; that is fine because iOS always
// calls us from the single packet-tunnel thread.

#[allow(clippy::missing_safety_doc)]
#[no_mangle]
pub unsafe extern "C" fn vpn21_ios_pump_start() {
    crate::ios_pump::global().start();
}

#[allow(clippy::missing_safety_doc)]
#[no_mangle]
pub unsafe extern "C" fn vpn21_ios_pump_stop() {
    crate::ios_pump::global().stop();
}

#[allow(clippy::missing_safety_doc)]
#[no_mangle]
pub unsafe extern "C" fn vpn21_ios_pump_push_inbound(data: *const u8, len: usize) -> c_int {
    if data.is_null() || len == 0 {
        return -1;
    }
    let slice = std::slice::from_raw_parts(data, len);
    if crate::ios_pump::global().push_inbound(slice.to_vec()) {
        0
    } else {
        -1
    }
}

/// Drains up to `max_packets` packets out of the pump's outbound queue into
/// `buf` (capacity `cap` bytes) using the TLV framing described above.
/// Returns the number of bytes written (>= 0) or -1 if `buf` is null / the
/// batch would not fit.  Swift uses the returned length to slice the buffer
/// into individual packets before handing them to `writePackets`.
#[allow(clippy::missing_safety_doc)]
#[no_mangle]
pub unsafe extern "C" fn vpn21_ios_pump_drain_outbound(
    buf: *mut u8,
    cap: usize,
    max_packets: usize,
) -> isize {
    if buf.is_null() || cap == 0 {
        return -1;
    }
    let pkts = crate::ios_pump::global().drain_outbound(max_packets);
    let mut out = std::slice::from_raw_parts_mut(buf, cap);
    let mut written: usize = 0;
    for p in &pkts {
        let need = 2 + p.len();
        if written + need > cap {
            // Spill the rest back; don't lie to the caller about a partial packet.
            // We push the unwritten tail back into the queue to preserve ordering.
            let remaining: Vec<Vec<u8>> = pkts
                [pkts.iter().position(|x| std::ptr::eq(x, p)).unwrap()..]
                .iter()
                .cloned()
                .collect();
            for r in remaining.into_iter().rev() {
                crate::ios_pump::global().push_outbound(r);
            }
            break;
        }
        let len = p.len() as u16;
        out[..2].copy_from_slice(&len.to_be_bytes());
        out[2..2 + p.len()].copy_from_slice(p);
        out = &mut out[need..];
        written += need;
    }
    written as isize
}

/// Returns a JSON-encoded [`IosPumpStats`] snapshot; Swift uses this to
/// surface queue pressure in the Logs tab.  Caller must free with
/// [`vpn21_string_free`].
#[allow(clippy::missing_safety_doc)]
#[no_mangle]
pub unsafe extern "C" fn vpn21_ios_pump_stats() -> *mut c_char {
    let s = crate::ios_pump::global().stats();
    cstr_out(serde_json::to_string(&s).unwrap_or_else(|_| "{}".into()))
}

/// Frees a C string previously returned by a `vpn21_*` function.
#[allow(clippy::missing_safety_doc)]
#[no_mangle]
pub unsafe extern "C" fn vpn21_string_free(s: *mut c_char) {
    if s.is_null() {
        return;
    }
    let _ = CString::from_raw(s);
}

// --- helpers -------------------------------------------------------------

unsafe fn cstr(p: *const c_char) -> Result<String, ()> {
    if p.is_null() {
        return Err(());
    }
    CStr::from_ptr(p)
        .to_str()
        .map(|s| s.to_owned())
        .map_err(|_| ())
}

unsafe fn cstr_to_pathbuf(p: *const c_char) -> Result<PathBuf, ()> {
    cstr(p).map(PathBuf::from)
}

fn cstr_out(s: String) -> *mut c_char {
    CString::new(s).unwrap_or_default().into_raw()
}

fn ok(msg: &str) -> *mut c_char {
    cstr_out(serde_json::json!({ "ok": true, "detail": msg }).to_string())
}

fn err(msg: &str) -> *mut c_char {
    cstr_out(serde_json::json!({ "ok": false, "error": msg }).to_string())
}
