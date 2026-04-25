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

# Simulator targets:
#   * aarch64-apple-ios-sim — Apple Silicon Macs
#   * x86_64-apple-ios      — Intel Macs (Rust still uses the non-sim triple
#                             for the x86_64 simulator slice; they are
#                             lipo-compatible because both carry the
#                             simulator platform in their LC_BUILD_VERSION).
TARGETS="${VPN21_IOS_TARGETS:-aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios}"

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
mkdir -p "$BUILD/sim-universal"

# Build the fat simulator slice (arm64 + x86_64).  If the user overrode
# VPN21_IOS_TARGETS we try to be lenient: lipo whatever sim slices exist.
SIM_INPUTS=()
for t in $TARGETS; do
  case "$t" in
    aarch64-apple-ios-sim|x86_64-apple-ios)
      SIM_INPUTS+=("$ROOT/target/$t/$PROFILE_DIR/libvpn21.a")
      ;;
  esac
done
if [[ ${#SIM_INPUTS[@]} -gt 0 ]]; then
  lipo -create "${SIM_INPUTS[@]}" -output "$BUILD/sim-universal/libvpn21.a"
fi

XCF_ARGS=()
# Device slice.
if [[ -f "$ROOT/target/aarch64-apple-ios/$PROFILE_DIR/libvpn21.a" ]]; then
  XCF_ARGS+=(-library "$ROOT/target/aarch64-apple-ios/$PROFILE_DIR/libvpn21.a"
             -headers "$ROOT/app/ios/PacketTunnel")
fi
# Simulator slice (fat).
if [[ -f "$BUILD/sim-universal/libvpn21.a" ]]; then
  XCF_ARGS+=(-library "$BUILD/sim-universal/libvpn21.a"
             -headers "$ROOT/app/ios/PacketTunnel")
fi
if [[ ${#XCF_ARGS[@]} -eq 0 ]]; then
  echo "error: no iOS slices were built; check cargo output above" >&2
  exit 1
fi

xcodebuild -create-xcframework "${XCF_ARGS[@]}" -output "$XCF"

echo
echo "Built $XCF"
echo "IPHONEOS_DEPLOYMENT_TARGET=$IPHONEOS_DEPLOYMENT_TARGET"
