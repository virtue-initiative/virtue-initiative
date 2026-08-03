#!/bin/sh
set -eu

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="${SRCROOT:-$(cd "$SCRIPT_DIR/.." && pwd)}"
RUST_DIR="$ROOT_DIR/rust"
MANIFEST_PATH="$RUST_DIR/Cargo.toml"
LIB_NAME="libvirtue_mac_rust.a"

PROFILE_DIR="debug"
CARGO_EXTRA=""
if [ "${CONFIGURATION:-Debug}" = "Release" ]; then
  PROFILE_DIR="release"
  CARGO_EXTRA="--release"
fi

# client/mac/rust is a member of the client/ Cargo workspace, so cargo
# always builds into the shared client/target dir (the same one
# `cargo build -p virtue-mac` and CI's rust-cache step use).
TARGET_DIR="${CARGO_TARGET_DIR:-$(cd "$RUST_DIR/../.." && pwd)/target}"

LIB_DEST="$BUILT_PRODUCTS_DIR/$LIB_NAME"
rm -f "$LIB_DEST"

# build-app.sh (the CI/release path) already builds virtue-mac-ffi for every
# requested arch, into this same TARGET_DIR, before invoking xcodebuild —
# and sets VIRTUE_MAC_RUST_PREBUILT to say so. Re-running cargo here for
# that case was found to always miss cargo's own fingerprint cache (full
# dependency recompile, ~3 min per arch) even though the artifacts were
# already fresh, so skip straight to using them instead of invoking cargo
# again. Local Xcode builds (no VIRTUE_MAC_RUST_PREBUILT) still build here.
build_target() {
  target="$1"
  if [ -n "${VIRTUE_MAC_RUST_PREBUILT:-}" ]; then
    return 0
  fi
  rustup target add "$target" >/dev/null 2>&1 || true
  if [ -n "$CARGO_EXTRA" ]; then
    cargo build --manifest-path "$MANIFEST_PATH" --target "$target" $CARGO_EXTRA
  else
    cargo build --manifest-path "$MANIFEST_PATH" --target "$target"
  fi
}

LIB_INPUTS=""
LIB_COUNT=0

case " ${ARCHS:-} " in
  *" arm64 "*)
  target="aarch64-apple-darwin"
  if [ -n "${VIRTUE_MAC_RUST_PREBUILT:-}" ]; then
    echo "Using prebuilt Rust bridge for target: ${target} (config: ${CONFIGURATION:-Debug})"
  else
    echo "Building Rust bridge for target: ${target} (config: ${CONFIGURATION:-Debug})"
  fi
  build_target "$target"
  LIB_INPUTS="$LIB_INPUTS $TARGET_DIR/$target/$PROFILE_DIR/$LIB_NAME"
  LIB_COUNT=$((LIB_COUNT + 1))
  ;;
esac

case " ${ARCHS:-} " in
  *" x86_64 "*)
  target="x86_64-apple-darwin"
  if [ -n "${VIRTUE_MAC_RUST_PREBUILT:-}" ]; then
    echo "Using prebuilt Rust bridge for target: ${target} (config: ${CONFIGURATION:-Debug})"
  else
    echo "Building Rust bridge for target: ${target} (config: ${CONFIGURATION:-Debug})"
  fi
  build_target "$target"
  LIB_INPUTS="$LIB_INPUTS $TARGET_DIR/$target/$PROFILE_DIR/$LIB_NAME"
  LIB_COUNT=$((LIB_COUNT + 1))
  ;;
esac

if [ "$LIB_COUNT" -eq 0 ]; then
  echo "No supported arch found in ARCHS='${ARCHS:-}'" >&2
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

echo "Prepared Rust bridge library -> $LIB_DEST"
