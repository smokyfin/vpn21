//! JNI surface for Android.
//!
//! The Kotlin `Vpn21VpnService` calls these symbols directly via
//! `external fun` on a companion object.  This keeps the TUN file
//! descriptor entirely on the native side: Kotlin opens the fd, hands it
//! straight to Rust, and Dart never sees it.
//!
//! All symbols use the `Java_com_vpn21_app_Vpn21Native_*` JNI naming
//! convention so they bind without any extra `RegisterNatives` call.

use jni::objects::{JClass, JString};
use jni::sys::{jboolean, jint, jstring};
use jni::JNIEnv;

use crate::config::Profile;
use crate::orchestrator::{Orchestrator, SessionState};
use crate::tun::TunConfig;
use crate::{init as core_init, InitOptions};

use super::runtime::runtime;
use super::ORCH;

/// `Vpn21Native.nativeInit(appDir: String, verbose: Boolean): Int`.
///
/// Idempotent — calling it twice is a no-op so the Kotlin side can be
/// written defensively and call `nativeInit` from both `MainActivity` and
/// `Vpn21VpnService.onCreate`.
#[no_mangle]
pub extern "system" fn Java_com_vpn21_app_Vpn21Native_nativeInit(
    mut env: JNIEnv,
    _class: JClass,
    app_dir: JString,
    verbose: jboolean,
) -> jint {
    let app_dir: String = match env.get_string(&app_dir) {
        Ok(s) => s.into(),
        Err(_) => return -1,
    };
    let opts = InitOptions {
        app_dir: app_dir.into(),
        verbose: verbose != 0,
    };
    match core_init(opts.clone()) {
        Ok(()) => {
            let _ = ORCH.set(Orchestrator::new(opts.app_dir));
            0
        }
        Err(e) => {
            tracing::error!(%e, "vpn21 nativeInit failed");
            -1
        }
    }
}

/// `Vpn21Native.nativeStartWithFd(profileJson: String, fd: Int, mtu: Int,
/// ipv4: String, mask: Int, dnsPort: Int): String`.
///
/// Returns the same JSON shape as the C-ABI `vpn21_start_with_fd` (so the
/// Kotlin side can decode `{ ok, detail|error }` without a second round
/// trip).  Ownership of `fd` transfers to Rust on success.
#[no_mangle]
pub extern "system" fn Java_com_vpn21_app_Vpn21Native_nativeStartWithFd(
    mut env: JNIEnv,
    _class: JClass,
    profile_json: JString,
    fd: jint,
    mtu: jint,
    ipv4: JString,
    mask: jint,
    dns_port: jint,
) -> jstring {
    let Some(orch) = ORCH.get().cloned() else {
        return jni_return(&mut env, json_err("not initialised"));
    };
    let profile_json: String = match env.get_string(&profile_json) {
        Ok(s) => s.into(),
        Err(_) => return jni_return(&mut env, json_err("bad profile_json")),
    };
    let profile: Profile = match serde_json::from_str(&profile_json) {
        Ok(p) => p,
        Err(e) => return jni_return(&mut env, json_err(&format!("profile parse: {e}"))),
    };
    let ipv4: String = match env.get_string(&ipv4) {
        Ok(s) => s.into(),
        Err(_) => return jni_return(&mut env, json_err("ipv4 missing")),
    };
    let ipv4 = match ipv4.parse::<std::net::Ipv4Addr>() {
        Ok(a) => a,
        Err(e) => return jni_return(&mut env, json_err(&format!("ipv4: {e}"))),
    };
    let tun = TunConfig {
        fd,
        mtu: mtu as u16,
        ipv4,
        ipv4_mask: mask as u8,
        dns_listener_port: dns_port as u16,
    };
    let rt = runtime();
    let res = rt.block_on(async move { orch.start(profile, tun).await });
    let payload = match res {
        Ok(()) => json_ok("started"),
        Err(e) => json_err(&e.to_string()),
    };
    jni_return(&mut env, payload)
}

/// `Vpn21Native.nativeStop(): String`.
#[no_mangle]
pub extern "system" fn Java_com_vpn21_app_Vpn21Native_nativeStop(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let Some(orch) = ORCH.get().cloned() else {
        return jni_return(&mut env, json_err("not initialised"));
    };
    let rt = runtime();
    let payload = match rt.block_on(async move { orch.stop().await }) {
        Ok(()) => json_ok("stopped"),
        Err(e) => json_err(&e.to_string()),
    };
    jni_return(&mut env, payload)
}

/// `Vpn21Native.nativeStatus(): String`.
#[no_mangle]
pub extern "system" fn Java_com_vpn21_app_Vpn21Native_nativeStatus(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let Some(orch) = ORCH.get().cloned() else {
        return jni_return(&mut env, json_err("not initialised"));
    };
    let s = orch.status();
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
    })
    .to_string();
    jni_return(&mut env, payload)
}

fn json_ok(detail: &str) -> String {
    serde_json::json!({ "ok": true, "detail": detail }).to_string()
}

fn json_err(message: &str) -> String {
    serde_json::json!({ "ok": false, "error": message }).to_string()
}

fn jni_return(env: &mut JNIEnv, body: String) -> jstring {
    match env.new_string(body) {
        Ok(s) => s.into_raw(),
        Err(_) => std::ptr::null_mut(),
    }
}
