#!/bin/sh
set -eu

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="${SRCROOT:-$(cd "$SCRIPT_DIR/.." && pwd)}"
RUST_DIR="$ROOT_DIR/rust"
MANIFEST_PATH="$RUST_DIR/Cargo.toml"
LIB_NAME="libvirtue_ios_rust.a"

if [ "${PLATFORM_NAME:-}" != "iphoneos" ] && [ "${PLATFORM_NAME:-}" != "iphonesimulator" ]; then
  echo "Unsupported PLATFORM_NAME='${PLATFORM_NAME:-}'" >&2
  exit 1
fi

PROFILE_DIR="debug"
CARGO_EXTRA=""
if [ "${CONFIGURATION:-Debug}" = "Release" ]; then
  PROFILE_DIR="release"
  CARGO_EXTRA="--release"
fi

LIB_DEST="$BUILT_PRODUCTS_DIR/$LIB_NAME"
rm -f "$LIB_DEST"

build_target() {
  target="$1"
  rustup target add "$target" >/dev/null 2>&1 || true
  if [ -n "$CARGO_EXTRA" ]; then
    cargo build --manifest-path "$MANIFEST_PATH" --target "$target" $CARGO_EXTRA
  else
    cargo build --manifest-path "$MANIFEST_PATH" --target "$target"
  fi
}

if [ "${PLATFORM_NAME:-}" = "iphoneos" ]; then
  local_target="aarch64-apple-ios"
  echo "Building Rust bridge for target: ${local_target} (config: ${CONFIGURATION:-Debug})"
  build_target "$local_target"
  cp "$RUST_DIR/target/$local_target/$PROFILE_DIR/$LIB_NAME" "$LIB_DEST"
else
  LIB_INPUTS=""
  LIB_COUNT=0

  case " ${ARCHS:-} " in
    *" arm64 "*)
    arm_target="aarch64-apple-ios-sim"
    echo "Building Rust bridge for target: ${arm_target} (config: ${CONFIGURATION:-Debug})"
    build_target "$arm_target"
    LIB_INPUTS="$LIB_INPUTS $RUST_DIR/target/$arm_target/$PROFILE_DIR/$LIB_NAME"
    LIB_COUNT=$((LIB_COUNT + 1))
    ;;
  esac

  case " ${ARCHS:-} " in
    *" x86_64 "*)
    x86_target="x86_64-apple-ios"
    echo "Building Rust bridge for target: ${x86_target} (config: ${CONFIGURATION:-Debug})"
    build_target "$x86_target"
    LIB_INPUTS="$LIB_INPUTS $RUST_DIR/target/$x86_target/$PROFILE_DIR/$LIB_NAME"
    LIB_COUNT=$((LIB_COUNT + 1))
    ;;
  esac

  if [ "$LIB_COUNT" -eq 0 ]; then
    echo "No supported simulator arch found in ARCHS='${ARCHS:-}'" >&2
    exit 1
  fi

  if [ "$LIB_COUNT" -eq 1 ]; then
    # shellcheck disable=SC2086
    set -- $LIB_INPUTS
    cp "$1" "$LIB_DEST"
  else
    # shellcheck disable=SC2086
    lipo -create $LIB_INPUTS -output "$LIB_DEST"
  fi
fi

echo "Prepared Rust bridge library -> $LIB_DEST"
