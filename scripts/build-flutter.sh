#!/usr/bin/env bash
# Orchestrates a complete vpn21 build: it first builds the Rust core for
# the requested platform, then runs the matching `flutter build` so the
# resulting bundle ships with libvpn21 / vpn21.dll / Vpn21Core.xcframework
# alongside the Flutter assets.
#
# Usage:
#   scripts/build-flutter.sh android
#   scripts/build-flutter.sh ios
#   scripts/build-flutter.sh macos
#   scripts/build-flutter.sh linux
#   scripts/build-flutter.sh windows
#   scripts/build-flutter.sh all            # everything the host can build
#
# Pass any extra flags after `--`, they are forwarded to `flutter build`:
#   scripts/build-flutter.sh android --release -- --split-per-abi
#
# Environment overrides:
#   VPN21_FLUTTER_BIN   path to flutter (default: `flutter` on $PATH)
#   VPN21_PROFILE       debug | release (default: debug)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
APP="$ROOT/app"
SCRIPTS="$ROOT/scripts"
FLUTTER="${VPN21_FLUTTER_BIN:-flutter}"

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <android|ios|macos|linux|windows|all> [--release] [-- <flutter-args>]" >&2
  exit 2
fi
PLATFORM="$1"; shift

PROFILE_FLAG=""
RUST_FLAG=""
FORWARD=()
saw_dashdash=0
for arg in "$@"; do
  if (( saw_dashdash )); then
    FORWARD+=("$arg")
    continue
  fi
  case "$arg" in
    --release) PROFILE_FLAG="--release"; RUST_FLAG="--release" ;;
    --) saw_dashdash=1 ;;
    *) FORWARD+=("$arg") ;;
  esac
done

if ! command -v "$FLUTTER" >/dev/null 2>&1; then
  echo "error: flutter not found (set VPN21_FLUTTER_BIN or add flutter to PATH)" >&2
  exit 2
fi

build_one() {
  local p="$1"
  case "$p" in
    android)
      "$SCRIPTS/build-android.sh" $RUST_FLAG
      cd "$APP" && "$FLUTTER" build apk $PROFILE_FLAG ${FORWARD[@]+"${FORWARD[@]}"}
      ;;
    ios)
      "$SCRIPTS/build-ios.sh" $RUST_FLAG
      # Flutter's iOS build picks up the staged XCFramework via the Xcode
      # build phase wired up in app/ios/Podfile + Runner.xcodeproj.  We
      # build with --no-codesign by default so CI / dev shells without a
      # signing identity still produce a runnable .app.
      cd "$APP" && "$FLUTTER" build ios $PROFILE_FLAG --no-codesign ${FORWARD[@]+"${FORWARD[@]}"}
      ;;
    macos)
      "$SCRIPTS/build-macos.sh" $RUST_FLAG
      cd "$APP" && "$FLUTTER" build macos $PROFILE_FLAG ${FORWARD[@]+"${FORWARD[@]}"}
      ;;
    linux)
      "$SCRIPTS/build-linux.sh" $RUST_FLAG
      cd "$APP" && "$FLUTTER" build linux $PROFILE_FLAG ${FORWARD[@]+"${FORWARD[@]}"}
      ;;
    windows)
      "$SCRIPTS/build-windows.sh" $RUST_FLAG
      cd "$APP" && "$FLUTTER" build windows $PROFILE_FLAG ${FORWARD[@]+"${FORWARD[@]}"}
      ;;
    *)
      echo "error: unknown platform: $p" >&2
      exit 2
      ;;
  esac
}

if [[ "$PLATFORM" == "all" ]]; then
  case "$(uname -s)" in
    Darwin)
      for p in macos ios android; do build_one "$p"; done
      ;;
    Linux)
      for p in linux android; do build_one "$p"; done
      ;;
    MINGW*|MSYS*|CYGWIN*|Windows_NT)
      for p in windows android; do build_one "$p"; done
      ;;
    *)
      echo "error: cannot infer platforms for $(uname -s)" >&2
      exit 2
      ;;
  esac
else
  build_one "$PLATFORM"
fi

echo
echo "vpn21 build for $PLATFORM completed (profile=${PROFILE_FLAG:-debug})"
