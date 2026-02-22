#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script must run on macOS."
  exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLIENT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"
cd "$CLIENT_ROOT"

"${SCRIPT_DIR}/build-app.sh"

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' mac/Cargo.toml | head -n1)"
APP_NAME="BePure.app"
DMG_NAME="BePure-${VERSION}.dmg"
DMG_PATH="target/macos/${DMG_NAME}"
STAGING_DIR="target/macos/dmg-staging"

rm -rf "$STAGING_DIR" "$DMG_PATH"
mkdir -p "$STAGING_DIR"
cp -R "target/macos/${APP_NAME}" "$STAGING_DIR/${APP_NAME}"
ln -s /Applications "$STAGING_DIR/Applications"

hdiutil create \
  -volname "BePure" \
  -srcfolder "$STAGING_DIR" \
  -ov \
  -format UDZO \
  "$DMG_PATH"

rm -rf "$STAGING_DIR"
echo "Built ${DMG_PATH}"
