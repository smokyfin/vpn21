#!/usr/bin/env bash
# Cross-compiles rust/vpn21-core for macOS (Intel + Apple Silicon) and
# drops a fat dylib into app/macos/Frameworks so the Flutter desktop
# Runner target can DYLD_LOAD it via dart:ffi.
#
# Requires:
#   - macOS with Xcode command line tools
#   - rustup (targets installed on demand)
#
# Usage:
#   scripts/build-macos.sh                # debug
#   scripts/build-macos.sh --release      # release + LTO
#   scripts/build-macos.sh --no-full      # only the leaf backend (no arti)
#
# Environment variables (optional):
#   MACOSX_DEPLOYMENT_TARGET   default: 11.0  (matches Flutter desktop floor)
#   VPN21_MACOS_TARGETS        override the target list, space separated

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CRATE="$ROOT/rust/vpn21-core"
OUT_DIR="$ROOT/app/macos/Frameworks"
DYLIB_OUT="$OUT_DIR/libvpn21.dylib"

: "${MACOSX_DEPLOYMENT_TARGET:=11.0}"
export MACOSX_DEPLOYMENT_TARGET

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
  echo "error: macOS builds require macOS" >&2
  exit 2
fi
if ! xcode-select -p >/dev/null 2>&1; then
  echo "error: Xcode command line tools not installed (run 'xcode-select --install')" >&2
  exit 2
fi

TARGETS="${VPN21_MACOS_TARGETS:-aarch64-apple-darwin x86_64-apple-darwin}"

for t in $TARGETS; do
  rustup target add "$t" >/dev/null
done

cd "$CRATE"
for t in $TARGETS; do
  echo "== cargo build --target $t =="
  cargo build $PROFILE_FLAG \
    --features "$FEATURES" \
    --target "$t" \
    --lib
done

mkdir -p "$OUT_DIR"

# lipo the per-arch dylibs into a fat dylib that runs on both Apple Silicon
# and Intel.  The Cargo.toml lists `cdylib` first, so a stock `cargo build`
# does produce libvpn21.dylib alongside the `.a`.
LIPO_INPUTS=()
for t in $TARGETS; do
  in="$ROOT/target/$t/$PROFILE_DIR/libvpn21.dylib"
  if [[ ! -f "$in" ]]; then
    echo "error: expected $in to exist after cargo build" >&2
    exit 1
  fi
  LIPO_INPUTS+=("$in")
done
lipo -create "${LIPO_INPUTS[@]}" -output "$DYLIB_OUT"

# Set the install_name so the framework loader resolves the lib relative
# to @rpath when embedded into the .app bundle.
install_name_tool -id "@rpath/libvpn21.dylib" "$DYLIB_OUT"

# Stage the public C header for Swift bridging.
mkdir -p "$OUT_DIR/Headers"
cp "$ROOT/app/ios/PacketTunnel/vpn21.h" "$OUT_DIR/Headers/"

echo
echo "Built $DYLIB_OUT"
lipo -info "$DYLIB_OUT"
echo "MACOSX_DEPLOYMENT_TARGET=$MACOSX_DEPLOYMENT_TARGET"
