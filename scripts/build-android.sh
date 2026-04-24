#!/usr/bin/env bash
# Cross-compiles rust/vpn21-core into the four Android ABIs Flutter ships with
# and drops the .so files into app/android/app/src/main/jniLibs/<abi>.
#
# Requires:
#   - rustup (targets installed below on demand)
#   - cargo-ndk (`cargo install cargo-ndk`)
#   - ANDROID_NDK_HOME environment variable
#
# Usage:
#   scripts/build-android.sh                # debug
#   scripts/build-android.sh --release      # release + strip
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CRATE="$ROOT/rust/vpn21-core"
OUT="$ROOT/app/android/app/src/main/jniLibs"

if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
  echo "error: ANDROID_NDK_HOME not set" >&2
  exit 2
fi
if ! command -v cargo-ndk >/dev/null 2>&1; then
  echo "error: cargo-ndk not installed. Run: cargo install cargo-ndk" >&2
  exit 2
fi

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

mkdir -p "$OUT"

# Android ABIs and their rustup target triples
declare -a TARGETS=(
  "arm64-v8a      aarch64-linux-android"
  "armeabi-v7a    armv7-linux-androideabi"
  "x86            i686-linux-android"
  "x86_64         x86_64-linux-android"
)

for row in "${TARGETS[@]}"; do
  set -- $row
  abi="$1"; triple="$2"
  rustup target add "$triple" >/dev/null
done

cd "$CRATE"
cargo ndk \
  --target aarch64-linux-android \
  --target armv7-linux-androideabi \
  --target i686-linux-android \
  --target x86_64-linux-android \
  --output-dir "$OUT" \
  --platform 24 \
  -- build $PROFILE_FLAG --features "$FEATURES"

echo
echo "Built vpn21 jniLibs:"
find "$OUT" -name "libvpn21.so" -printf "  %p\n"
