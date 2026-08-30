#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RUST_DIR="$(cd "$SCRIPT_DIR/../rust" && pwd)"
# client/ios/rust is a member of the client/ Cargo workspace, so cargo
# always builds into the shared client/target dir.
TARGET_DIR="${CARGO_TARGET_DIR:-$(cd "$RUST_DIR/../.." && pwd)/target}"
OUT_DIR="$TARGET_DIR/ios-xcframework"
LIB_NAME="libvirtue_ios_rust.a"

cd "$RUST_DIR"

cargo build --release --target aarch64-apple-ios
cargo build --release --target aarch64-apple-ios-sim

mkdir -p "$OUT_DIR"

xcodebuild -create-xcframework \
  -library "$TARGET_DIR/aarch64-apple-ios/release/$LIB_NAME" \
  -library "$TARGET_DIR/aarch64-apple-ios-sim/release/$LIB_NAME" \
  -output "$OUT_DIR/VirtueIOSRust.xcframework"

echo "Built: $OUT_DIR/VirtueIOSRust.xcframework"
