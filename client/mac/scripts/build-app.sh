#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script must run on macOS."
  exit 1
fi

CLIENT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MAC_ROOT="${CLIENT_ROOT}/mac"
cd "$CLIENT_ROOT"

source "${CLIENT_ROOT}/scripts/version.sh"

BASE_VERSION="$(virtue_base_version)"
BUILD_LABEL="$(virtue_build_label)"
APPLE_BUILD_NUMBER="$(virtue_apple_build_number)"
APP_NAME="Virtue.app"
APP_ROOT="target/macos/${APP_NAME}"
CODESIGN_IDENTITY="${CODESIGN_IDENTITY:--}"
DEVELOPMENT_TEAM="${DEVELOPMENT_TEAM:-}"
DAEMON_TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)

PROJECT_PATH="${MAC_ROOT}/VirtueMac.xcodeproj"
if [[ ! -d "$PROJECT_PATH" ]]; then
  echo "Missing ${PROJECT_PATH}. Run mac/scripts/generate-project.sh (requires xcodegen) and commit the result." >&2
  exit 1
fi

# 1. Build the headless daemon binary for both architectures via plain cargo,
#    then lipo them into one universal binary. The daemon is a separate
#    cargo binary, unaffected by the SwiftUI/Xcode rewrite.
for target in "${DAEMON_TARGETS[@]}"; do
  rustup target add "$target" >/dev/null 2>&1 || true
  VIRTUE_BUILD_LABEL="$BUILD_LABEL" cargo build --release --target "$target" -p virtue-mac
done
UNIVERSAL_DAEMON_DIR="target/macos-universal-daemon"
mkdir -p "$UNIVERSAL_DAEMON_DIR"
lipo -create \
  "target/${DAEMON_TARGETS[0]}/release/virtue-mac" \
  "target/${DAEMON_TARGETS[1]}/release/virtue-mac" \
  -output "${UNIVERSAL_DAEMON_DIR}/virtue-daemon"

# 2. Build the SwiftUI app via xcodebuild against the already-generated,
#    committed .xcodeproj — matching the iOS client's convention. CI must
#    NOT depend on `xcodegen` being installed/working at build time; the
#    project is regenerated and committed by developers via
#    generate-project.sh whenever project.yml changes. Xcode's default
#    ARCHS already produces a universal (arm64 + x86_64) app binary, and the
#    `build-rust-for-xcode.sh` preBuildScript mirrors that by lipo-ing a
#    universal `libvirtue_mac_rust.a` — one build covers both architectures.
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
