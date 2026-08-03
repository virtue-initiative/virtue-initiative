#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script must run on macOS."
  exit 1
fi

ARCH="universal"

usage() {
  cat <<EOF
Usage: $(basename "$0") [--arch universal|arm64|x86_64]

  --arch universal   Build both architectures and lipo them (default; what CI/release use).
  --arch arm64        Build only the Apple Silicon slice. Faster for local iteration on arm64 hosts.
  --arch x86_64        Build only the Intel slice. Faster for local iteration on Intel hosts.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch)
      ARCH="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

CLIENT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MAC_ROOT="${CLIENT_ROOT}/mac"
cd "$CLIENT_ROOT"

source "${CLIENT_ROOT}/scripts/version.sh"

BASE_VERSION="$(virtue_base_version)"
BUILD_LABEL="$(virtue_build_label)"
APPLE_BUILD_NUMBER="$(virtue_apple_build_number)"
APP_NAME="Virtue.app"
APP_ROOT="target/macos/${APP_NAME}"
# Default to a real credentialed identity, matching ios/scripts/run-on-device.sh's
# TEAM_ID default — ad-hoc signing isn't sufficient for the SwiftUI app's
# SMAppService-registered daemon (macOS SIGKILLs it with "Code Signature
# Invalid" once a real Team ID has ever been registered for this bundle ID,
# since the ad-hoc signature has no Team ID to satisfy that check). CI has no
# access to this identity, so it must override both vars to "-"/"" explicitly.
CODESIGN_IDENTITY="${CODESIGN_IDENTITY:-Developer ID Application: Jeffrey Baumes (6277E5UTS9)}"
DEVELOPMENT_TEAM="${DEVELOPMENT_TEAM:-6277E5UTS9}"

case "$ARCH" in
  universal)
    DAEMON_TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
    XCODE_ARCHS=""
    ;;
  arm64)
    DAEMON_TARGETS=(aarch64-apple-darwin)
    XCODE_ARCHS="arm64"
    ;;
  x86_64)
    DAEMON_TARGETS=(x86_64-apple-darwin)
    XCODE_ARCHS="x86_64"
    ;;
  *)
    echo "Unknown --arch value: $ARCH (expected universal|arm64|x86_64)" >&2
    exit 1
    ;;
esac

PROJECT_PATH="${MAC_ROOT}/VirtueMac.xcodeproj"
if [[ ! -d "$PROJECT_PATH" ]]; then
  echo "Missing ${PROJECT_PATH}. Run mac/scripts/generate-project.sh (requires xcodegen) and commit the result." >&2
  exit 1
fi

# 1. Build the headless daemon binary for the requested architecture(s) via
#    plain cargo. In universal mode, lipo the two slices together; in
#    single-arch mode just use the one slice directly. The daemon is a
#    separate cargo binary, unaffected by the SwiftUI/Xcode rewrite.
#
#    Also build virtue-mac-ffi (the staticlib the SwiftUI app links against)
#    in this same invocation, per target. xcodebuild's own "Build Rust
#    Bridge" prebuild-script phase (build-rust-for-xcode.sh) builds that
#    same crate again for the same target/profile, but as a separate `cargo`
#    invocation it was found to always miss cargo's fingerprint cache and
#    recompile the entire dependency tree from scratch (~3 min/arch) even
#    though the artifacts here are already fresh. Building it here instead,
#    and telling that script to skip its own build via
#    VIRTUE_MAC_RUST_PREBUILT below, avoids paying for that twice.
for target in "${DAEMON_TARGETS[@]}"; do
  rustup target add "$target" >/dev/null 2>&1 || true
  VIRTUE_BUILD_LABEL="$BUILD_LABEL" cargo build --release --target "$target" -p virtue-mac -p virtue-mac-ffi
done
UNIVERSAL_DAEMON_DIR="target/macos-universal-daemon"
mkdir -p "$UNIVERSAL_DAEMON_DIR"
if [[ "${#DAEMON_TARGETS[@]}" -eq 1 ]]; then
  cp "target/${DAEMON_TARGETS[0]}/release/virtue-mac" "${UNIVERSAL_DAEMON_DIR}/virtue-daemon"
else
  lipo -create \
    "target/${DAEMON_TARGETS[0]}/release/virtue-mac" \
    "target/${DAEMON_TARGETS[1]}/release/virtue-mac" \
    -output "${UNIVERSAL_DAEMON_DIR}/virtue-daemon"
fi

# 2. Build the SwiftUI app via xcodebuild against the already-generated,
#    committed .xcodeproj — matching the iOS client's convention. CI must
#    NOT depend on `xcodegen` being installed/working at build time; the
#    project is regenerated and committed by developers via
#    generate-project.sh whenever project.yml changes. Xcode's default
#    ARCHS already produces a universal (arm64 + x86_64) app binary, and the
#    `build-rust-for-xcode.sh` preBuildScript mirrors that by lipo-ing a
#    universal `libvirtue_mac_rust.a` — one build covers both architectures.
#    VIRTUE_MAC_RUST_PREBUILT tells that script the .a it needs was already
#    built above, so it just picks it up instead of invoking cargo again.
export VIRTUE_MAC_RUST_PREBUILT=1
(
  cd "$MAC_ROOT"
  XCODEBUILD_ARGS=(
    -project VirtueMac.xcodeproj
    -scheme VirtueMac
    -configuration Release
    -derivedDataPath "${CLIENT_ROOT}/target/macos/DerivedData"
    MARKETING_VERSION="${BASE_VERSION}"
    CURRENT_PROJECT_VERSION="${APPLE_BUILD_NUMBER}"
    VIRTUE_BUILD_LABEL="${BUILD_LABEL}"
    CODE_SIGN_IDENTITY="${CODESIGN_IDENTITY}"
  )
  # Automatic signing needs an explicit team when using a real (non-adhoc)
  # identity — the Team ID from e.g. "Developer ID Application: NAME (TEAMID)".
  if [[ -n "$DEVELOPMENT_TEAM" ]]; then
    XCODEBUILD_ARGS+=(DEVELOPMENT_TEAM="${DEVELOPMENT_TEAM}")
  fi
  # Constrains both the Swift compile and the `build-rust-for-xcode.sh`
  # preBuildScript (which reads $ARCHS) to a single slice.
  if [[ -n "$XCODE_ARCHS" ]]; then
    XCODEBUILD_ARGS+=(ARCHS="${XCODE_ARCHS}" ONLY_ACTIVE_ARCH=NO)
  fi
  xcodebuild "${XCODEBUILD_ARGS[@]}" build
)

rm -rf "$APP_ROOT"
mkdir -p "$(dirname "$APP_ROOT")"
cp -R "target/macos/DerivedData/Build/Products/Release/${APP_NAME}" "$APP_ROOT"

# 3. Bundle the universal daemon binary inside the app. `daemon_exe_path()`
#    (Rust FFI) and the coordinator both resolve this same path at runtime.
install -m 0755 "${UNIVERSAL_DAEMON_DIR}/virtue-daemon" "${APP_ROOT}/Contents/MacOS/virtue-daemon"

# 4. Re-sign: adding a file after `xcodebuild` invalidates the app's
#    signature, so both the embedded daemon and the outer bundle must be
#    signed (in that order) after copying it in.
codesign --force --options runtime --sign "$CODESIGN_IDENTITY" "${APP_ROOT}/Contents/MacOS/virtue-daemon"
codesign --force --deep --options runtime --sign "$CODESIGN_IDENTITY" "$APP_ROOT"

echo "Built ${APP_ROOT}"
