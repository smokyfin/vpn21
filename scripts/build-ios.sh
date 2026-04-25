#!/usr/bin/env bash
# Cross-compiles rust/vpn21-core as a static library for iOS device + simulator
# and bundles the result as an XCFramework that the Runner target and the
# PacketTunnel extension both link against.
#
# Requires:
#   - macOS with Xcode command line tools (`xcode-select -p` returns a path)
#   - rustup (targets installed on demand)
#
# Usage:
#   scripts/build-ios.sh                # debug
#   scripts/build-ios.sh --release      # release + LTO
#   scripts/build-ios.sh --no-full      # only the leaf backend (no arti)
#
# Environment variables (optional):
#   IPHONEOS_DEPLOYMENT_TARGET   default: 14.0 (see below)
#   VPN21_IOS_TARGETS            override the target list, space separated
#
# Why we set IPHONEOS_DEPLOYMENT_TARGET: when unset, cc-rs falls back to
# iOS 10.0.  Xcode 15+ SDKs build C objects against the SDK version and
# reference symbols (e.g. `___chkstk_darwin`) that only exist on iOS 13+.
# Linking them against iOS 10 fails at ld-time.  14.0 is the floor used
# by Flutter 3.x and covers > 99.5 % of active devices.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CRATE="$ROOT/rust/vpn21-core"
BUILD="$ROOT/target/ios-xcframework"
XCF="$ROOT/app/ios/PacketTunnel/Vpn21Core.xcframework"

: "${IPHONEOS_DEPLOYMENT_TARGET:=14.0}"
export IPHONEOS_DEPLOYMENT_TARGET

PROFILE_FLAG=""
PROFILE_DIR="debug"
FEATURES="full"
for arg in "$@"; do
  case "$arg" in
    --release) PROFILE_FLAG="--release"; PROFILE_DIR="release" ;;
    --no-full) FEATURES="backend-leaf" ;;
    *) echo "unknown arg: $arg" >&2; exit 2 ;;
  esac
done

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "error: iOS builds require macOS" >&2
  exit 2
fi
if ! xcode-select -p >/dev/null 2>&1; then
  echo "error: Xcode command line tools are not installed (run 'xcode-select --install')" >&2
  exit 2
fi

# Simulator targets (both carry PLATFORM_IOSSIMULATOR in LC_BUILD_VERSION
# so they are lipo-compatible into a single simulator slice):
#   * aarch64-apple-ios-sim — Apple Silicon Macs
#   * x86_64-apple-ios-sim  — Intel Macs (since Rust 1.73 the old
#                             x86_64-apple-ios triple is a *device* target,
#                             so lipo-ing it with aarch64-apple-ios-sim
#                             produced a universal lib with mismatched
#                             platform tags that xcodebuild -create-
#                             xcframework rejected).
TARGETS="${VPN21_IOS_TARGETS:-aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios-sim}"

for t in $TARGETS; do
  rustup target add "$t" >/dev/null
done

cd "$CRATE"

# Build ONLY the staticlib crate-type for each iOS target.
#
# Our Cargo.toml declares `crate-type = ["cdylib", "staticlib", "rlib"]` so
# Android / Linux / Windows get their cdylib — but `cdylib` on iOS forces
# the linker to resolve every symbol, which is not what we want for a
# staticlib that will be linked into the PacketTunnel extension later.
# `cargo rustc -- --crate-type=staticlib` overrides the Cargo.toml list for
# this invocation only.
for t in $TARGETS; do
  echo "== cargo rustc --target $t (crate-type=staticlib, IPHONEOS_DEPLOYMENT_TARGET=$IPHONEOS_DEPLOYMENT_TARGET) =="
  cargo rustc $PROFILE_FLAG \
    --features "$FEATURES" \
    --target "$t" \
    --lib \
    -- --crate-type=staticlib
done

rm -rf "$BUILD" "$XCF"
mkdir -p "$BUILD/sim-universal" "$BUILD/headers"
# Stage ONLY the public C header into a dedicated directory.  xcodebuild's
# `-headers` copies the whole tree it points at into the framework's
# Headers/ subdir, so pointing it at `app/ios/PacketTunnel` would also
# ship the Swift sources and Info.plist as if they were headers.
cp "$ROOT/app/ios/PacketTunnel/vpn21.h" "$BUILD/headers/"

# Build the fat simulator slice (arm64 + x86_64).  Bucket the requested
# targets by slice so that overriding VPN21_IOS_TARGETS cannot silently
# pick up stale `.a` files from a previous build.
SIM_INPUTS=()
DEVICE_LIB=""
for t in $TARGETS; do
  case "$t" in
    aarch64-apple-ios)
      DEVICE_LIB="$ROOT/target/$t/$PROFILE_DIR/libvpn21.a"
      ;;
    aarch64-apple-ios-sim|x86_64-apple-ios-sim)
      SIM_INPUTS+=("$ROOT/target/$t/$PROFILE_DIR/libvpn21.a")
      ;;
  esac
done
if [[ ${#SIM_INPUTS[@]} -gt 0 ]]; then
  lipo -create "${SIM_INPUTS[@]}" -output "$BUILD/sim-universal/libvpn21.a"
fi

XCF_ARGS=()
# Device slice (only if the caller actually asked for aarch64-apple-ios).
if [[ -n "$DEVICE_LIB" && -f "$DEVICE_LIB" ]]; then
  XCF_ARGS+=(-library "$DEVICE_LIB"
             -headers "$BUILD/headers")
fi
# Simulator slice (fat).
if [[ -f "$BUILD/sim-universal/libvpn21.a" ]]; then
  XCF_ARGS+=(-library "$BUILD/sim-universal/libvpn21.a"
             -headers "$BUILD/headers")
fi
if [[ ${#XCF_ARGS[@]} -eq 0 ]]; then
  echo "error: no iOS slices were built; check cargo output above" >&2
  exit 1
fi

xcodebuild -create-xcframework "${XCF_ARGS[@]}" -output "$XCF"

echo
echo "Built $XCF"
echo "IPHONEOS_DEPLOYMENT_TARGET=$IPHONEOS_DEPLOYMENT_TARGET"
