#!/usr/bin/env bash
# Builds rust/vpn21-core as a shared library for the host Linux ABI and
# drops it into app/linux/bundled so the Flutter Linux Runner target can
# DYLD_LOAD it via dart:ffi.
#
# We do not cross-compile to other Linux ABIs by default — Flutter's
# linux/CMakeLists.txt copies the bundled lib into the app's lib/ at install
# time.  Pass VPN21_LINUX_TARGET to override.
#
# Requires:
#   - rustup (host target only by default)
#   - On Debian/Ubuntu: `sudo apt install build-essential pkg-config libssl-dev`
#
# Usage:
#   scripts/build-linux.sh                # debug
#   scripts/build-linux.sh --release      # release + LTO
#   scripts/build-linux.sh --no-full      # only the leaf backend (no arti)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CRATE="$ROOT/rust/vpn21-core"
OUT="$ROOT/app/linux/bundled"

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

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "error: Linux builds require Linux" >&2
  exit 2
fi

TARGET="${VPN21_LINUX_TARGET:-$(rustc -vV | awk '/host:/ {print $2}')}"
rustup target add "$TARGET" >/dev/null

cd "$CRATE"
echo "== cargo build --target $TARGET =="
cargo build $PROFILE_FLAG \
  --features "$FEATURES" \
  --target "$TARGET" \
  --lib

SO_IN="$ROOT/target/$TARGET/$PROFILE_DIR/libvpn21.so"
if [[ ! -f "$SO_IN" ]]; then
  echo "error: expected $SO_IN to exist after cargo build" >&2
  exit 1
fi

mkdir -p "$OUT"
cp "$SO_IN" "$OUT/libvpn21.so"

# Stage the public C header for the Linux Runner if it ever switches to a
# C++ inline call (today it talks to the lib via dart:ffi only).
mkdir -p "$OUT/include"
cp "$ROOT/app/ios/PacketTunnel/vpn21.h" "$OUT/include/"

echo
echo "Built $OUT/libvpn21.so"
file "$OUT/libvpn21.so" || true
