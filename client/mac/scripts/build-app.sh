#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script must run on macOS."
  exit 1
fi

CLIENT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$CLIENT_ROOT"

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' mac/Cargo.toml | head -n1)"
APP_NAME="BePure.app"
APP_ROOT="target/macos/${APP_NAME}"
CONTENTS_DIR="${APP_ROOT}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"

cargo build --release -p bepure-mac-client

rm -rf "$APP_ROOT"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

install -m 0755 target/release/bepure-mac-client "${MACOS_DIR}/BePure"

cat > "${CONTENTS_DIR}/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>BePure</string>
  <key>CFBundleDisplayName</key>
  <string>BePure</string>
  <key>CFBundleIdentifier</key>
  <string>codes.anb.bepure.mac</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleExecutable</key>
  <string>BePure</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
  <key>NSScreenCaptureUsageDescription</key>
  <string>BePure captures screenshots for accountability monitoring.</string>
</dict>
</plist>
PLIST

echo "Built ${APP_ROOT}"
