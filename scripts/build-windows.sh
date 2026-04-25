#!/usr/bin/env bash
# Builds rust/vpn21-core as a DLL for Windows and drops it into
# app/windows/bundled so the Flutter Windows Runner target can
# LoadLibrary it via dart:ffi.
#
# Runs natively on Windows (under MSYS / Git Bash) or as a cross compile
# from Linux/macOS using the `x86_64-pc-windows-gnu` mingw toolchain.
#
# Requires (native Windows build):
#   - rustup with `x86_64-pc-windows-msvc`
#   - Visual Studio Build Tools (msvc linker)
#
# Requires (cross-compile from Linux/macOS):
#   - rustup with `x86_64-pc-windows-gnu`
#   - MinGW-w64: `apt install mingw-w64` or `brew install mingw-w64`
#
# Usage:
#   scripts/build-windows.sh                # debug
#   scripts/build-windows.sh --release      # release + LTO
#   scripts/build-windows.sh --no-full      # only the leaf backend (no arti)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CRATE="$ROOT/rust/vpn21-core"
OUT="$ROOT/app/windows/bundled"

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

# Pick a default target: msvc when running on Windows, gnu when cross.
case "$(uname -s 2>/dev/null || echo Windows_NT)" in
  MINGW*|MSYS*|CYGWIN*|Windows_NT)
    DEFAULT_TARGET="x86_64-pc-windows-msvc"
    ;;
  *)
    DEFAULT_TARGET="x86_64-pc-windows-gnu"
    ;;
esac
TARGET="${VPN21_WINDOWS_TARGET:-$DEFAULT_TARGET}"
rustup target add "$TARGET" >/dev/null

cd "$CRATE"
echo "== cargo build --target $TARGET =="
cargo build $PROFILE_FLAG \
  --features "$FEATURES" \
  --target "$TARGET" \
  --lib

DLL_IN="$ROOT/target/$TARGET/$PROFILE_DIR/vpn21.dll"
LIB_IN="$ROOT/target/$TARGET/$PROFILE_DIR/vpn21.dll.lib"
if [[ ! -f "$DLL_IN" ]]; then
  echo "error: expected $DLL_IN to exist after cargo build" >&2
  exit 1
fi

mkdir -p "$OUT"
cp "$DLL_IN" "$OUT/vpn21.dll"
[[ -f "$LIB_IN" ]] && cp "$LIB_IN" "$OUT/vpn21.dll.lib"

mkdir -p "$OUT/include"
cp "$ROOT/app/ios/PacketTunnel/vpn21.h" "$OUT/include/"

echo
echo "Built $OUT/vpn21.dll"
file "$OUT/vpn21.dll" 2>/dev/null || true
