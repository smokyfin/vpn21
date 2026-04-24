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
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CRATE="$ROOT/rust/vpn21-core"
BUILD="$ROOT/target/ios-xcframework"
XCF="$ROOT/app/ios/PacketTunnel/Vpn21Core.xcframework"

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

for t in aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios; do
  rustup target add "$t" >/dev/null
done

cd "$CRATE"
cargo build $PROFILE_FLAG --features "$FEATURES" --target aarch64-apple-ios
cargo build $PROFILE_FLAG --features "$FEATURES" --target aarch64-apple-ios-sim
cargo build $PROFILE_FLAG --features "$FEATURES" --target x86_64-apple-ios

rm -rf "$BUILD" "$XCF"
mkdir -p "$BUILD/sim-universal"
# Fat sim lib (arm64 + x86_64) — XCFramework wants one per platform variant.
lipo -create \
  "$ROOT/target/aarch64-apple-ios-sim/$PROFILE_DIR/libvpn21.a" \
  "$ROOT/target/x86_64-apple-ios/$PROFILE_DIR/libvpn21.a" \
  -output "$BUILD/sim-universal/libvpn21.a"

xcodebuild -create-xcframework \
  -library "$ROOT/target/aarch64-apple-ios/$PROFILE_DIR/libvpn21.a" \
  -headers "$ROOT/app/ios/PacketTunnel" \
  -library "$BUILD/sim-universal/libvpn21.a" \
  -headers "$ROOT/app/ios/PacketTunnel" \
  -output "$XCF"

echo
echo "Built $XCF"
