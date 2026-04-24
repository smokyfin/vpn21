# vpn21 — architecture

## Goals

* Pure-Rust (+ C/C++ where unavoidable) VPN datapath so that iOS Network
  Extension constraints are satisfied.
* Deliberate, small FFI surface between the UI (Flutter) and the Rust
  orchestrator.
* Pluggable transport layer — today we ship a leaf-based VLESS+REALITY+gRPC
  provider, tomorrow we want to drop in xray-core / lyrebird without touching
  the orchestrator.
* Strong isolation of on-disk artefacts (configs, unix sockets, SOCKS
  credentials). Nothing ever lives in `/tmp` on mobile.

## Crates

```
rust/
  vpn21-core/            ← single crate (cdylib + rlib + staticlib)
    src/
      lib.rs             ← public re-exports + init
      config/            ← profile parsing
      transport/         ← TransportProvider trait + leaf impl
      arti_pipeline.rs   ← spawn/monitor arti with PT SOCKS
      dns/               ← TUN-bound DNS resolver + DoH client
      orchestrator.rs    ← session state machine
      tun.rs             ← TUN FD adoption (platform-dependent)
      ffi/               ← C ABI bindings for Flutter
      logging.rs         ← ring buffer + UI tailing
      secure_store.rs    ← random ports / creds, scoped paths
```

The crate is compiled as:

* `cdylib` for Android (`libvpn21.so`) and desktop Linux/macOS/Windows.
* `staticlib` for iOS (embedded into a Swift XCFramework so it can live
  inside a PacketTunnelProvider).

## Session state machine

```
        ┌────── start(profile) ──────┐
        │                            │
Idle ──▶ Bootstrapping ──▶ Connecting ──▶ Connected
  ▲          │                │              │
  │          ▼                ▼              ▼
  │        Error           Error          Disconnecting
  │          │                │              │
  └──────────┴────────────────┴──────────────┘
```

Cancellation is cooperative: every async task shares a
`tokio_util::sync::CancellationToken`. `stop()` cancels the token and awaits
all tasks with a hard 5 s budget before aborting them, so the UI can never
hang on "Disconnecting".

## Ports & credentials

For every session we pick, atomically and with retries:

* `leaf1_socks` — random high port, SOCKS5 user/pass (random 32 bytes).
* `arti_socks_in` — the SOCKS endpoint arti exposes *to* leaf#1.
* `arti_pt_socks_out` — the SOCKS endpoint that the PT driver uses to reach
  leaf#2.
* `leaf2_socks` — random high port, SOCKS5 user/pass.
* `dns_listener` — `udp` bound to the TUN interface address on port 53.

All listeners bind to `127.0.0.1` (or the TUN IP for DNS) and reject peers
that cannot authenticate. Credentials exist only in memory and in a
permission-restricted JSON blob inside the app-private directory.

## On-disk layout (mobile)

```
<Application Support>/vpn21/
  profiles/<id>.json         ← user profiles (chmod 600)
  runtime/session.json       ← current session ports/creds (0600, deleted on stop)
  cache/arti/                ← arti state (directory cache, guard data)
  logs/app.log               ← rotating log, tailed by the UI
```

## Pluggable transport abstraction

```rust
pub trait TransportProvider: Send + Sync + 'static {
    fn id(&self) -> &'static str;                   // "leaf", "xray", ...
    fn capabilities(&self) -> TransportCapabilities;
    fn start(&self, cfg: TransportConfig, cancel: CancellationToken)
        -> futures::future::BoxFuture<'static, Result<TransportHandle>>;
}
```

The leaf provider today parses the Xray-style JSON (`outbounds`) and
generates an equivalent leaf config in memory. Future providers receive the
same input shape and are free to map it however they want.

## DNS

`vpn21-core` embeds a tiny UDP DNS server (via `hickory-proto`) bound to the
TUN IP. It receives queries from the TUN, applies an optional rule set
(domains declared `direct` are resolved locally and bypass the tunnel) and
forwards the rest as DoH to the configured `doh_server`. If
`doh_server_ip` is present we connect directly to that IP and use the host
name only for SNI, so the system resolver is never consulted.

## Threads

* Tokio current-thread runtime per provider (leaf spawns its own internally).
* One orchestrator task owns the session state machine.
* One logging task drains the in-process ring buffer into the log file.

## Security notes

* No `/tmp`. All paths derive from `dirs_next::data_local_dir()` on desktop,
  from `Application Support` on iOS, from `filesDir` on Android.
* On Unix, runtime files are created with `OpenOptions::mode(0o600)` and the
  runtime directory with `0o700`.
* UNIX sockets, when used (desktop), live inside the per-user runtime dir
  and are guarded by filesystem permissions.
* SOCKS listeners always require user/password authentication — random 32
  bytes encoded base64 per session.

## Future work

* Replace the leaf provider with xray-core when/if its FFI stabilises.
* Add lyrebird (obfs4) as a secondary PT, chained before VLESS.
* Kill-switch toggle that drops all non-tunnel traffic via the OS VPN API.
