#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script must run on macOS."
  exit 1
fi

CLIENT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$CLIENT_ROOT"

VERSION="$(sed -n 's/^version = "\(.*\)"$/\1/p' mac/Cargo.toml | head -n1)"
APP_NAME="Virtue.app"
APP_ROOT="target/macos/${APP_NAME}"
CONTENTS_DIR="${APP_ROOT}/Contents"
MACOS_DIR="${CONTENTS_DIR}/MacOS"
RESOURCES_DIR="${CONTENTS_DIR}/Resources"
ICON_SOURCE="mac/assets/AppIcon.icns"

if [[ ! -f "$ICON_SOURCE" ]]; then
  echo "Missing ${ICON_SOURCE}. Run images/generate-icons.sh first."
  exit 1
fi

cargo build --release -p virtue-mac-client

rm -rf "$APP_ROOT"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

install -m 0755 target/release/virtue-mac-client "${MACOS_DIR}/Virtue"
install -m 0644 "$ICON_SOURCE" "${RESOURCES_DIR}/AppIcon.icns"

cat > "${CONTENTS_DIR}/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key>
  <string>Virtue</string>
  <key>CFBundleDisplayName</key>
  <string>Virtue</string>
  <key>CFBundleIdentifier</key>
  <string>codes.anb.virtue.mac</string>
  <key>CFBundleVersion</key>
  <string>${VERSION}</string>
  <key>CFBundleShortVersionString</key>
  <string>${VERSION}</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleExecutable</key>
  <string>Virtue</string>
  <key>CFBundleIconFile</key>
  <string>AppIcon.icns</string>
  <key>LSUIElement</key>
  <true/>
  <key>NSPrincipalClass</key>
  <string>NSApplication</string>
  <key>NSScreenCaptureUsageDescription</key>
  <string>Virtue captures screenshots for accountability monitoring.</string>
</dict>
</plist>
PLIST

echo "Built ${APP_ROOT}"
