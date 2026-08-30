#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script must run on macOS."
  exit 1
fi

ARCH="universal"
PROFILE="release"

usage() {
  cat <<EOF
Usage: $(basename "$0") [--arch universal|arm64|x86_64] [--profile debug|release]

  --arch universal   Build both architectures and lipo them (default; what CI/release use).
  --arch arm64        Build only the Apple Silicon slice. Faster for local iteration on arm64 hosts.
  --arch x86_64        Build only the Intel slice. Faster for local iteration on Intel hosts.
  --profile debug     Build the debug profile. Faster; used for PR/non-release CI runs.
  --profile release   Build the release profile (default; what CI/release use).
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --arch)
      ARCH="$2"
      shift 2
      ;;
    --profile)
      PROFILE="$2"
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

case "$PROFILE" in
  debug|release) ;;
  *)
    echo "Unknown --profile value: $PROFILE (expected debug|release)" >&2
    exit 1
    ;;
esac

CLIENT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MAC_ROOT="${CLIENT_ROOT}/mac"
cd "$CLIENT_ROOT"

source "${CLIENT_ROOT}/scripts/version.sh"

BASE_VERSION="$(virtue_base_version)"
BUILD_LABEL="$(virtue_build_label)"
RELEASE_CHANNEL="$(virtue_release_channel)"
# CFBundleVersion, and therefore what Sparkle orders updates by. Derived from
# the commit (see virtue_mac_bundle_version), NOT from APPLE_BUILD_NUMBER:
# that one is shared with iOS and only changes when someone bumps
# version.properties, so every dev-channel build between bumps would compare
# equal and never update. Nothing has to be regenerated or committed for this
# to be correct — it recomputes on every build.
MAC_BUNDLE_VERSION="$(virtue_mac_bundle_version)"
APP_NAME="Virtue.app"
APP_ROOT="target/macos/${APP_NAME}"
# Default to a real credentialed identity, matching ios/scripts/run-on-device.sh's
# TEAM_ID default — ad-hoc signing isn't sufficient for the SwiftUI app's
# SMAppService-registered daemon (macOS SIGKILLs it with "Code Signature
# Invalid" once a real Team ID has ever been registered for this bundle ID,
# since the ad-hoc signature has no Team ID to satisfy that check). CI has no
# access to this identity, so it must override both vars to "-"/"" explicitly.
# "Developer ID Application" (with no name/team suffix) matches whichever such
# identity is present in the local signer's keychain, so this doesn't need to
# hardcode any one developer's name.
CODESIGN_IDENTITY="${CODESIGN_IDENTITY:-Developer ID Application}"
DEVELOPMENT_TEAM="${DEVELOPMENT_TEAM:-Y2Z8ZS4D33}"

# Auto-update (Sparkle) is opt-in at build time, mirroring the Linux package's
# /usr/lib/virtue/auto-update-enabled flag: without VIRTUE_ENABLE_AUTO_UPDATE=1
# the feed URL and public key stay empty, and UpdateController never starts the
# updater. Only the release-branch CI job sets it, so a locally built or
# PR-built app can never silently replace itself with a GitHub build.
#
# One feed serves both channels; dev builds opt into dev-tagged appcast items
# via SPUUpdaterDelegate.allowedChannels. See landing/scripts/build-appcast.mjs.
SPARKLE_FEED_URL=""
SPARKLE_PUBLIC_KEY=""
if [[ "${VIRTUE_ENABLE_AUTO_UPDATE:-}" == "1" ]]; then
  SPARKLE_FEED_URL="${VIRTUE_SPARKLE_FEED_URL:-https://virtueinitiative.org/appcast.xml}"
  SPARKLE_PUBLIC_KEY="${VIRTUE_SPARKLE_PUBLIC_KEY:-HtpEKdwRb1gFDQsdKAdACAjgO/uqWA5t2SoIOmI1i8Q=}"
  if [[ -z "$SPARKLE_PUBLIC_KEY" ]]; then
    echo "VIRTUE_ENABLE_AUTO_UPDATE=1 but no Sparkle public key is available." >&2
    exit 1
  fi
fi

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

if [[ "$PROFILE" == "release" ]]; then
  CARGO_PROFILE_FLAG="--release"
  XCODE_CONFIGURATION="Release"
else
  CARGO_PROFILE_FLAG=""
  XCODE_CONFIGURATION="Debug"
fi
CARGO_PROFILE_DIR="$PROFILE"

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
  VIRTUE_BUILD_LABEL="$BUILD_LABEL" cargo build $CARGO_PROFILE_FLAG --target "$target" -p virtue-mac -p virtue-mac-ffi
done
UNIVERSAL_DAEMON_DIR="target/macos-universal-daemon"
mkdir -p "$UNIVERSAL_DAEMON_DIR"
if [[ "${#DAEMON_TARGETS[@]}" -eq 1 ]]; then
  cp "target/${DAEMON_TARGETS[0]}/${CARGO_PROFILE_DIR}/virtue-mac" "${UNIVERSAL_DAEMON_DIR}/virtue-daemon"
else
  lipo -create \
    "target/${DAEMON_TARGETS[0]}/${CARGO_PROFILE_DIR}/virtue-mac" \
    "target/${DAEMON_TARGETS[1]}/${CARGO_PROFILE_DIR}/virtue-mac" \
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
    -configuration "${XCODE_CONFIGURATION}"
    -derivedDataPath "${CLIENT_ROOT}/target/macos/DerivedData"
    MARKETING_VERSION="${BASE_VERSION}"
    CURRENT_PROJECT_VERSION="${MAC_BUNDLE_VERSION}"
    VIRTUE_BUILD_LABEL="${BUILD_LABEL}"
    VIRTUE_RELEASE_CHANNEL="${RELEASE_CHANNEL}"
    VIRTUE_SPARKLE_FEED_URL="${SPARKLE_FEED_URL}"
    VIRTUE_SPARKLE_PUBLIC_KEY="${SPARKLE_PUBLIC_KEY}"
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
cp -R "target/macos/DerivedData/Build/Products/${XCODE_CONFIGURATION}/${APP_NAME}" "$APP_ROOT"

# 3. Bundle the universal daemon binary inside the app. `daemon_exe_path()`
#    (Rust FFI) and the coordinator both resolve this same path at runtime.
install -m 0755 "${UNIVERSAL_DAEMON_DIR}/virtue-daemon" "${APP_ROOT}/Contents/MacOS/virtue-daemon"

# 4. Re-sign: adding a file after `xcodebuild` invalidates the app's
#    signature, so both the embedded daemon and the outer bundle must be
#    signed (in that order) after copying it in.
#
#    NOT `--deep`. Sparkle.framework ships nested code of its own (Autoupdate,
#    Updater.app, and the Downloader/Installer XPC services), which xcodebuild
#    has already signed correctly with this same identity while embedding it.
#    `--deep` would re-sign all of that from the outside with the outer
#    bundle's flags — which Sparkle documents as breaking its updater, and
#    which Apple deprecates for exactly this reason. Signing only what we
#    actually added, and letting the outer seal reference the existing nested
#    signatures, is both correct and what `--verify --deep --strict` below
#    checks.
#
#    Sparkle's nested code does need re-signing, though, and not for the same
#    reason. xcodebuild signs the *outer* Sparkle.framework with our identity
#    when embedding it, but leaves the helpers inside it (Autoupdate,
#    Updater.app, and the two XPC services) carrying the ad-hoc signature they
#    shipped with in the SPM binary artifact — verified by checking
#    `codesign -dv` TeamIdentifier on each, which reads "not set". That still
#    passes `--verify --deep --strict` locally, because ad-hoc signatures are
#    valid signatures; it fails at *notarization*, which rejects nested
#    executables that aren't Developer ID signed with a hardened runtime. So
#    sign them explicitly, inside out — deepest first, each seal being
#    included in the one above it.
SPARKLE_VERSION_DIR="${APP_ROOT}/Contents/Frameworks/Sparkle.framework/Versions/B"
# Deepest first; each seal is included in the one above it.
SPARKLE_NESTED_CODE=(
  "${SPARKLE_VERSION_DIR}/XPCServices/Downloader.xpc"
  "${SPARKLE_VERSION_DIR}/XPCServices/Installer.xpc"
  "${SPARKLE_VERSION_DIR}/Updater.app"
  "${SPARKLE_VERSION_DIR}/Autoupdate"
)
if [[ -d "$SPARKLE_VERSION_DIR" ]]; then
  for nested in "${SPARKLE_NESTED_CODE[@]}"; do
    if [[ -e "$nested" ]]; then
      codesign --force --options runtime --sign "$CODESIGN_IDENTITY" "$nested"
    fi
  done
  # Versioned framework: sign the version directory, not the .framework
  # symlink farm, which codesign rejects as an unrecognized bundle format.
  codesign --force --options runtime --sign "$CODESIGN_IDENTITY" "$SPARKLE_VERSION_DIR"
fi

codesign --force --options runtime --sign "$CODESIGN_IDENTITY" "${APP_ROOT}/Contents/MacOS/virtue-daemon"
codesign --force --options runtime --sign "$CODESIGN_IDENTITY" "$APP_ROOT"

# 5. Fail the build here rather than shipping a bundle whose nested Sparkle
#    code is unsigned or mis-sealed — that failure mode is otherwise invisible
#    until an update refuses to install on a user's machine.
codesign --verify --deep --strict --verbose=2 "$APP_ROOT"

#    `--verify --deep` above accepts ad-hoc signatures, so it would not catch
#    the nested-Sparkle problem the previous step exists to fix. Assert the
#    team identifier on every nested executable directly. Skipped for ad-hoc
#    builds (CI PR builds pass CODESIGN_IDENTITY=-), which have no team by
#    definition.
#
#    Note the deliberate absence of a pipe here. `codesign -dv ... | grep -q`
#    is a trap under `set -o pipefail`: grep exits at the first match and
#    SIGPIPEs codesign, so the pipeline reports codesign's 141 rather than
#    grep's 0, and the check fails at random depending on how much output
#    codesign had left to write. Capture once, match in the shell.
if [[ "$CODESIGN_IDENTITY" != "-" && -d "$SPARKLE_VERSION_DIR" ]]; then
  for nested in "${SPARKLE_NESTED_CODE[@]}"; do
    [[ -e "$nested" ]] || continue
    nested_signature_info="$(codesign -dv "$nested" 2>&1 || true)"
    if [[ "$nested_signature_info" != *"TeamIdentifier=${DEVELOPMENT_TEAM}"* ]]; then
      echo "Nested code is not signed with team ${DEVELOPMENT_TEAM}: ${nested}" >&2
      echo "Notarization would reject this build." >&2
      exit 1
    fi
  done
fi

echo "Built ${APP_ROOT}"
