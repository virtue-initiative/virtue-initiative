#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLIENT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$CLIENT_ROOT/.." && pwd)"
CROSS_DIR="$SCRIPT_DIR/apple-cross"

usage() {
  cat <<'EOF'
Build the macOS or iOS app from Linux. Rust compiles here, much faster than on the Mac. The Mac then
compiles the Swift, links and signs, over SSH. Linux never needs Apple's SDK.

Usage:
  remote-apple-build.sh --platform <mac|ios> [options]
  remote-apple-build.sh --prune [--host <ssh-host>]

Options:
  --platform <mac|ios>           The app to build (required)
  --host <ssh-host>              The Mac. Default: $VIRTUE_APPLE_BUILD_HOST, else
                                 virtue-ios@$VD_MAC_HOST (default 10.27.27.6)
  --profile <debug|release>      Default: debug
  --arch <x86_64|arm64|universal>
                                 Default: the Mac's own arch. iOS takes x86_64 or arm64 only.
  --destination <simulator|device>
                                 iOS only. Default: simulator
  --identity <codesign identity> macOS only. Default: - (ad-hoc)
  --run                          iOS simulator only: install and launch in the booted simulator
  --rust-only                    Stop after compiling Rust, without touching the Mac unless a
                                 binary needs linking there
  --prune                        Delete the Mac's build dirs for worktrees that no longer exist
  -h, --help                     Show this help

The Mac keeps a build dir per worktree in ~/virtue-builds, so worktrees don't clobber each other. The
macOS app also lands in /Users/Shared/virtue-builds/<worktree>/, where the test account can open it.
Full logs: client/target/remote-apple/. CI still builds what ships. See AGENTS.md.
EOF
}

die() {
  echo "remote-apple-build: $*" >&2
  exit 1
}

if [[ -f "${VIRTUE_DEV_ENV:-$HOME/.config/virtue-dev.env}" ]]; then
  set -a
  # shellcheck disable=SC1090
  . "${VIRTUE_DEV_ENV:-$HOME/.config/virtue-dev.env}"
  set +a
fi

PLATFORM=""
HOST="${VIRTUE_APPLE_BUILD_HOST:-virtue-ios@${VD_MAC_HOST:-10.27.27.6}}"
PROFILE="debug"
ARCH=""
DESTINATION="simulator"
IDENTITY="-"
RUN=0
RUST_ONLY=0
PRUNE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --platform)
      PLATFORM="${2:-}"
      shift 2
      ;;
    --host)
      HOST="${2:-}"
      shift 2
      ;;
    --profile)
      PROFILE="${2:-}"
      shift 2
      ;;
    --arch)
      ARCH="${2:-}"
      shift 2
      ;;
    --destination)
      DESTINATION="${2:-}"
      shift 2
      ;;
    --identity)
      IDENTITY="${2:-}"
      shift 2
      ;;
    --run)
      RUN=1
      shift
      ;;
    --rust-only)
      RUST_ONLY=1
      shift
      ;;
    --prune)
      PRUNE=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

mac() {
  ssh -o BatchMode=yes -o ConnectTimeout=10 "$HOST" "$@"
}

# Same key remote-windows-build.sh and scripts/cow-cache.sh use for this worktree.
WORKTREE_KEY="$(basename "$REPO_ROOT")-$(printf '%s' "$REPO_ROOT" | sha256sum | cut -c1-8)"
BUILD_DIR="virtue-builds/$WORKTREE_KEY"
SHARED_DIR="/Users/Shared/virtue-builds/$WORKTREE_KEY"

if [[ $PRUNE -eq 1 ]]; then
  while read -r key source; do
    if [[ -n "$key" && ! -d "$source" ]]; then
      echo "Removing $key (worktree $source is gone)"
      mac "rm -rf ~/virtue-builds/$key /Users/Shared/virtue-builds/$key"
    fi
  done < <(mac 'for d in ~/virtue-builds/*/; do [ -f "$d/source" ] && echo "$(basename "$d") $(cat "$d/source")"; done')
  exit 0
fi

[[ "$PLATFORM" == "mac" || "$PLATFORM" == "ios" ]] || die "--platform must be mac or ios"
[[ "$PROFILE" == "debug" || "$PROFILE" == "release" ]] || die "--profile must be debug or release"
[[ "$DESTINATION" == "simulator" || "$DESTINATION" == "device" ]] || die "--destination must be simulator or device"
if [[ $RUN -eq 1 && ("$PLATFORM" != "ios" || "$DESTINATION" != "simulator") ]]; then
  die "--run only works for the iOS simulator"
fi
command -v clang >/dev/null || die "clang not found"
command -v rsync >/dev/null || die "rsync not found"

if [[ -z "$ARCH" ]]; then
  if [[ "$PLATFORM" == "ios" && "$DESTINATION" == "device" ]]; then
    ARCH="arm64"
  else
    ARCH="$(mac uname -m)"
  fi
fi

# Rust target -> the same target in clang's spelling.
clang_target() {
  case "$1" in
    x86_64-apple-darwin) echo "x86_64-apple-macos13.0" ;;
    aarch64-apple-darwin) echo "arm64-apple-macos13.0" ;;
    aarch64-apple-ios) echo "arm64-apple-ios16.0" ;;
    aarch64-apple-ios-sim) echo "arm64-apple-ios16.0-simulator" ;;
    x86_64-apple-ios) echo "x86_64-apple-ios16.0-simulator" ;;
    *) die "no clang target for $1" ;;
  esac
}

case "$PLATFORM-$ARCH-$DESTINATION" in
  mac-x86_64-*) TARGETS=(x86_64-apple-darwin) ;;
  mac-arm64-*) TARGETS=(aarch64-apple-darwin) ;;
  mac-universal-*) TARGETS=(aarch64-apple-darwin x86_64-apple-darwin) ;;
  ios-arm64-device) TARGETS=(aarch64-apple-ios) ;;
  ios-arm64-simulator) TARGETS=(aarch64-apple-ios-sim) ;;
  ios-x86_64-simulator) TARGETS=(x86_64-apple-ios) ;;
  *) die "can't build $PLATFORM for $ARCH on $DESTINATION" ;;
esac

# shellcheck disable=SC1091
source "$CLIENT_ROOT/scripts/version.sh"
export VIRTUE_BUILD_LABEL VIRTUE_GIT_SHORT_HASH VIRTUE_GIT_REF_NAME VIRTUE_BUILD_DATE VIRTUE_COMMIT_MINUTES
VIRTUE_GIT_SHORT_HASH="$(virtue_git_short_hash)"
VIRTUE_GIT_REF_NAME="$(virtue_git_ref_name)"
VIRTUE_BUILD_DATE="$(virtue_build_date)"
VIRTUE_COMMIT_MINUTES="$(virtue_commit_minutes)"
VIRTUE_BUILD_LABEL="$(virtue_build_label)"

TARGET_ROOT="${CARGO_TARGET_DIR:-$CLIENT_ROOT/target}"
LOG_DIR="$TARGET_ROOT/remote-apple"
mkdir -p "$LOG_DIR"
LOG_FILE="$LOG_DIR/$PLATFORM-$(date +%Y%m%d-%H%M%S).log"

# Run a command with its output in the log, showing the end of it only if it fails.
logged() {
  if ! "$@" >>"$LOG_FILE" 2>&1; then
    tail -n 40 "$LOG_FILE" >&2
    die "failed; full log: $LOG_FILE"
  fi
}

REMOTE_HOME="$(mac 'echo $HOME')"

build_rust() {
  local target="$1" triple_var cargo_args=(build --target "$target")
  triple_var="$(tr 'a-z-' 'A-Z_' <<<"$target")"
  [[ "$PROFILE" == "release" ]] && cargo_args+=(--release)
  if [[ "$PLATFORM" == "mac" ]]; then
    cargo_args+=(-p virtue-mac-ffi --lib -p virtue-mac --bin virtue-mac)
  else
    cargo_args+=(-p virtue-ios)
  fi
  rustup target add "$target" >/dev/null 2>&1 || true
  # cc-rs asks xcrun for the SDK unless SDKROOT is set. apple-cc ignores it.
  (
    cd "$CLIENT_ROOT"
    env \
      "CC_${target//-/_}=$CROSS_DIR/apple-cc" \
      "CARGO_TARGET_${triple_var}_LINKER=$CROSS_DIR/apple-link" \
      "APPLE_CC_TARGET=$(clang_target "$target")" \
      "VIRTUE_APPLE_LINK_HOST=$HOST" \
      "VIRTUE_APPLE_LINK_DIR=$REMOTE_HOME/$BUILD_DIR/link" \
      "SDKROOT=$CROSS_DIR/include" \
      MACOSX_DEPLOYMENT_TARGET=13.0 \
      IPHONEOS_DEPLOYMENT_TARGET=16.0 \
      cargo "${cargo_args[@]}"
  )
}

for target in "${TARGETS[@]}"; do
  echo "Compiling Rust for $target..."
  logged build_rust "$target"
done

if [[ $RUST_ONLY -eq 1 ]]; then
  echo "Rust built in $TARGET_ROOT."
  exit 0
fi

echo "Copying to $HOST..."
mac "mkdir -p $BUILD_DIR/client && echo $(printf '%q' "$REPO_ROOT") > $BUILD_DIR/source"
logged rsync -az --delete \
  --exclude=target --exclude='.derived-data*' --exclude=dist \
  --exclude=android --exclude=windows --exclude=linux \
  "$CLIENT_ROOT/" "$HOST:$BUILD_DIR/client/"
for target in "${TARGETS[@]}"; do
  out="$TARGET_ROOT/$target/$PROFILE"
  remote_out="$BUILD_DIR/client/target/$target/$PROFILE"
  mac "mkdir -p $remote_out"
  if [[ "$PLATFORM" == "mac" ]]; then
    logged rsync -a "$out/libvirtue_mac_rust.a" "$out/virtue-mac" "$HOST:$remote_out/"
  else
    logged rsync -a "$out/libvirtue_ios_rust.a" "$HOST:$remote_out/"
  fi
done

REMOTE_ENV="VIRTUE_BUILD_LABEL=$VIRTUE_BUILD_LABEL VIRTUE_GIT_SHORT_HASH=$VIRTUE_GIT_SHORT_HASH"
REMOTE_ENV+=" VIRTUE_GIT_REF_NAME=$(printf '%q' "$VIRTUE_GIT_REF_NAME") VIRTUE_BUILD_DATE=$VIRTUE_BUILD_DATE"
REMOTE_ENV+=" VIRTUE_COMMIT_MINUTES=$VIRTUE_COMMIT_MINUTES"

echo "Building the app on $HOST..."
if [[ "$PLATFORM" == "mac" ]]; then
  logged mac "cd $BUILD_DIR/client && $REMOTE_ENV VIRTUE_MAC_RUST_PREBUILT=1 CODESIGN_IDENTITY=$(printf '%q' "$IDENTITY") ./mac/scripts/build-app.sh --arch $ARCH --profile $PROFILE"
  logged mac "mkdir -p $SHARED_DIR && rm -rf $SHARED_DIR/Virtue.app && ditto $BUILD_DIR/client/target/macos/Virtue.app $SHARED_DIR/Virtue.app && chmod -R a+rX /Users/Shared/virtue-builds"
  echo "Built $HOST:$SHARED_DIR/Virtue.app"
else
  sdk="iphoneos"
  [[ "$DESTINATION" == "simulator" ]] && sdk="iphonesimulator"
  configuration="Debug"
  [[ "$PROFILE" == "release" ]] && configuration="Release"
  logged mac "cd $BUILD_DIR/client && $REMOTE_ENV VIRTUE_IOS_RUST_PREBUILT=1 ./ios/scripts/build-ios.sh --sdk $sdk --configuration $configuration --arch $ARCH --derived-data $REMOTE_HOME/$BUILD_DIR/derived-data-ios --code-signing-allowed NO"
  app="$BUILD_DIR/derived-data-ios/Build/Products/$configuration-$sdk/VirtueIOS.app"
  echo "Built $HOST:$app"
  if [[ $RUN -eq 1 ]]; then
    logged mac "xcrun simctl install booted $app && xcrun simctl launch booted org.virtueinitiative.virtueios"
    echo "Launched in the booted simulator."
  fi
fi
