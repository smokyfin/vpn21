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

### Build scripts

All five platforms have a dedicated Rust build script under `scripts/`:

| script                 | output                                                                              |
| ---------------------- | ----------------------------------------------------------------------------------- |
| `build-android.sh`     | `app/android/app/src/main/jniLibs/<abi>/libvpn21.so` (per-ABI cargo-ndk build)      |
| `build-ios.sh`         | `app/ios/PacketTunnel/Vpn21Core.xcframework` (arm64 device + arm64/x86_64 sim)      |
| `build-macos.sh`       | `app/macos/Frameworks/libvpn21.dylib` (universal Apple Silicon + Intel)             |
| `build-linux.sh`       | `app/linux/bundled/libvpn21.so`                                                     |
| `build-windows.sh`     | `app/windows/bundled/vpn21.dll` (msvc on Windows, mingw cross from *nix)            |

`scripts/build-flutter.sh <platform>` orchestrates the full build pipeline
end-to-end: it runs the matching Rust script first and then `flutter
build <platform>` so the resulting bundle ships the native lib alongside
the Flutter assets.  `scripts/build-flutter.sh all` is a convenience that
builds every platform the host can target.

```
# Single platform
ANDROID_NDK_HOME=... scripts/build-flutter.sh android --release
scripts/build-flutter.sh ios     --release
scripts/build-flutter.sh macos   --release
scripts/build-flutter.sh linux   --release
scripts/build-flutter.sh windows --release

# Or, if you only need the Rust artifact:
scripts/build-android.sh --release   # ditto: ios / macos / linux / windows
```

### Architectural rule: the TUN fd never crosses the Dart boundary

The whole VPN datapath (TUN adoption, leaf #1 / arti / leaf #2 / DNS) lives
in `vpn21-core` (Rust).  Flutter only deals with profiles and lifecycle:

* **Android.**  `Vpn21VpnService` opens the fd via
  `VpnService.Builder.establish()` and hands it directly into Rust through
  the JNI shim `Java_com_vpn21_app_Vpn21Native_nativeStartWithFd` (compiled
  in via the `android-jni` cargo feature).  Dart only sends a
  `vpn21/native#startVpn` MethodChannel call.
* **iOS.**  `PacketTunnelProvider` calls the C-ABI `vpn21_start_with_fd`
  from the extension once `setTunnelNetworkSettings` returns.  Dart asks
  the system to start the extension via `NETunnelProviderManager`.
* **Desktop.**  Dart calls `vpn21_start_desktop(profile_json)` via
  `dart:ffi`; Rust opens its own TUN through `tun::platform::provision`
  (`/dev/net/tun` on Linux, utun on macOS, WinTun on Windows).

This is what keeps the iOS NetworkExtension RAM budget (~50 MB resident)
realistic — no Dart isolate is loaded inside the extension; only the
small Rust core + leaf + arti runtime.

### Provisioning desktop projects

The Flutter desktop runner directories (`app/linux`, `app/macos`,
`app/windows`) only contain the bits we add by hand
(`vpn21_method_channel.{cc,cpp}`, `MainFlutterWindow.swift`).  The full
runner / CMakeLists / Xcode project is generated on demand from a Flutter
SDK that has the corresponding desktop target enabled:

```
flutter config --enable-linux-desktop --enable-macos-desktop --enable-windows-desktop
cd app && flutter create --platforms=linux,macos,windows .
```

After that, the Rust build scripts above drop the native lib into the
runner-bundled directory and `flutter build <platform>` picks it up.
