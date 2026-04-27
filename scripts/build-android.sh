#!/usr/bin/env bash
# Cross-compiles rust/vpn21-core into the four Android ABIs Flutter ships with
# and drops the .so files into app/android/app/src/main/jniLibs/<abi>.
#
# Requires:
#   - rustup (targets installed below on demand)
#   - cargo-ndk (`cargo install cargo-ndk`)
#   - ANDROID_NDK_HOME or ANDROID_NDK_ROOT environment variable
#
# Usage:
#   scripts/build-android.sh                # debug
#   scripts/build-android.sh --release      # release + strip
#   scripts/build-android.sh --no-full      # only the leaf backend (no arti)
#
# Environment variables (optional):
#   VPN21_ANDROID_API_LEVEL   default: 24 (matches Flutter's minSdkVersion)
#   VPN21_ANDROID_ABIS        override the ABI list, space separated

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CRATE="$ROOT/rust/vpn21-core"
OUT="$ROOT/app/android/app/src/main/jniLibs"

# Accept either of the two NDK env var names cargo-ndk recognises.
if [[ -z "${ANDROID_NDK_HOME:-}" && -n "${ANDROID_NDK_ROOT:-}" ]]; then
  export ANDROID_NDK_HOME="$ANDROID_NDK_ROOT"
fi
if [[ -z "${ANDROID_NDK_HOME:-}" ]]; then
  echo "error: set ANDROID_NDK_HOME (or ANDROID_NDK_ROOT) to your NDK install" >&2
  exit 2
fi
if ! command -v cargo-ndk >/dev/null 2>&1; then
  echo "error: cargo-ndk not installed. Run: cargo install cargo-ndk" >&2
  exit 2
fi

PROFILE_FLAG=""
PROFILE_DIR="debug"
# Android always wants the JNI shim — Vpn21VpnService loads
# `Java_com_vpn21_app_Vpn21Native_*` from libvpn21.so.
FEATURES="full,android-jni"
for arg in "$@"; do
  case "$arg" in
    --release) PROFILE_FLAG="--release"; PROFILE_DIR="release" ;;
    --no-full) FEATURES="backend-leaf,android-jni" ;;
    *) echo "unknown arg: $arg" >&2; exit 2 ;;
  esac
done

API_LEVEL="${VPN21_ANDROID_API_LEVEL:-24}"

# ABI ↔ rustup triple mapping.  Keep in sync with
# app/android/app/build.gradle `ndk.abiFilters`.
declare -A ABI_TO_TRIPLE=(
  [arm64-v8a]=aarch64-linux-android
  [armeabi-v7a]=armv7-linux-androideabi
  [x86]=i686-linux-android
  [x86_64]=x86_64-linux-android
)
ABIS="${VPN21_ANDROID_ABIS:-arm64-v8a armeabi-v7a x86_64}"

mkdir -p "$OUT"

CARGO_NDK_TARGETS=()
for abi in $ABIS; do
  triple="${ABI_TO_TRIPLE[$abi]:-}"
  if [[ -z "$triple" ]]; then
    echo "error: unknown Android ABI: $abi" >&2
    exit 2
  fi
  rustup target add "$triple" >/dev/null
  CARGO_NDK_TARGETS+=(--target "$triple")
done

cd "$CRATE"
cargo ndk \
  "${CARGO_NDK_TARGETS[@]}" \
  --output-dir "$OUT" \
  --platform "$API_LEVEL" \
  -- build $PROFILE_FLAG --features "$FEATURES"

echo
echo "Built vpn21 jniLibs (profile=$PROFILE_DIR, api=$API_LEVEL, features=$FEATURES):"
find "$OUT" -name "libvpn21.so" -print | sort
