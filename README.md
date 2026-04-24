# vpn21

Cross-platform VPN client that tunnels traffic through Tor using a custom
pluggable transport (VLESS + REALITY + gRPC). The goal is to expose a single,
polished user experience (Flutter) on top of a production-grade Rust core
that can be reused across Android, iOS, Windows, Linux and macOS.

## Traffic pipeline

```
device apps
    │
    ▼
┌───────────┐  1. packets                  ┌────────────────┐
│   TUN     │ ────────────────────────────▶│  leaf #1       │
│ interface │                              │  TUN → SOCKS   │
└───────────┘                              └──────┬─────────┘
                                                  │ 2. SOCKS5 (loopback)
                                                  ▼
                                           ┌────────────────┐
                                           │     arti       │
                                           │  (Tor client)  │
                                           │ PT = vless     │
                                           └──────┬─────────┘
                                                  │ 3. SOCKS5 from
                                                  │    PT driver
                                                  ▼
                                           ┌────────────────┐
                                           │   leaf #2      │
                                           │  vless+reality │
                                           │  +grpc         │
                                           └──────┬─────────┘
                                                  │ 4. TLS / gRPC
                                                  ▼
                                      ┌────────────────────────┐
                                      │  remote Tor bridge     │
                                      │  ORPort 127.0.0.1:9001 │
                                      └────────────────────────┘
```

A custom DNS server is bound to the TUN interface IP. Queries are forwarded
over DoH (default Cloudflare) through leaf #2 so that no plaintext DNS ever
leaves the device. If `doh_server_ip` is set in the profile, that IP is used
verbatim (the system resolver is bypassed).

## Repository layout

| path                        | description                                   |
| --------------------------- | --------------------------------------------- |
| `rust/vpn21-core`           | Rust workspace crate: orchestrator + FFI      |
| `rust/vpn21-ffi-header`     | Generated C header for Flutter                |
| `app/`                      | Flutter app (Dart) + native glue              |
| `app/android`               | Android Kotlin + JNI bridge                   |
| `app/ios`                   | iOS Swift app + PacketTunnel extension        |
| `docs/ARCHITECTURE.md`      | Detailed design notes                         |
| `docs/PLUGINS.md`           | How to add xray-core / lyrebird providers     |

## Building

See `docs/ARCHITECTURE.md` and `docs/PLUGINS.md`. The first end-to-end
targets are Android and iOS; the desktop platforms share the same Rust core
and only need a different TUN provider and UI shell.

### Rust

```
cargo check --workspace                               # default (stub) features
cargo test  -p vpn21-core                             # 10+ unit tests
cargo build --release --features full                 # real leaf + arti datapath
```

`rust-toolchain.toml` pins Rust 1.90. `cargo test --lib` covers the config
parser, the secure-store permissions/RNG, and SOCKS URL encoding.

### Mobile packaging

```
# Android: produces libvpn21.so in app/android/app/src/main/jniLibs/<abi>
ANDROID_NDK_HOME=... scripts/build-android.sh --release

# iOS: produces app/ios/PacketTunnel/Vpn21Core.xcframework
scripts/build-ios.sh --release
```

### Desktop TUN

The core exposes `vpn21_tun_provision` on Linux/macOS/Windows which opens
a TUN device (Linux uses `/dev/net/tun`) and hands the fd back to Flutter.
On macOS and Windows the call currently returns an error describing the
helper that still needs to be shipped (utun / wintun).
