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

source "${CLIENT_ROOT}/scripts/version.sh"

BUILD_LABEL="$(virtue_build_label)"
APP_NAME="Virtue.app"
DMG_NAME="Virtue-${BUILD_LABEL}.dmg"
DMG_PATH="target/macos/${DMG_NAME}"
TEMP_DMG_PATH="target/macos/Virtue-${BUILD_LABEL}-temp.dmg"
STAGING_DIR="target/macos/dmg-staging"
VOLUME_NAME="Virtue"

rm -rf "$STAGING_DIR" "$DMG_PATH" "$TEMP_DMG_PATH"
mkdir -p "$STAGING_DIR"
cp -R "target/macos/${APP_NAME}" "$STAGING_DIR/${APP_NAME}"
ln -s /Applications "$STAGING_DIR/Applications"

hdiutil create \
  -volname "$VOLUME_NAME" \
  -srcfolder "$STAGING_DIR" \
  -ov \
  -format UDRW \
  "$TEMP_DMG_PATH"

ATTACH_OUTPUT="$(hdiutil attach -readwrite -noverify -noautoopen "$TEMP_DMG_PATH")"
DEVICE="$(printf '%s\n' "$ATTACH_OUTPUT" | awk '/\/Volumes\// {print $1; exit}')"
MOUNT_PATH="$(printf '%s\n' "$ATTACH_OUTPUT" | awk '/\/Volumes\// {print substr($0, index($0, "/Volumes/")); exit}')"
DISK_NAME="$(basename "$MOUNT_PATH")"

cleanup() {
  if mount | grep -q "on ${MOUNT_PATH} "; then
    hdiutil detach "$DEVICE" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

osascript <<EOF
tell application "Finder"
  tell disk "${DISK_NAME}"
    open
    set current view of container window to icon view
    set toolbar visible of container window to false
    set statusbar visible of container window to false
    set bounds of container window to {100, 100, 700, 420}
    set theViewOptions to the icon view options of container window
    set arrangement of theViewOptions to not arranged
    set icon size of theViewOptions to 128
    set position of item "${APP_NAME}" to {150, 150}
    set position of item "Applications" to {450, 150}
    update without registering applications
    delay 1
    close
  end tell
end tell
EOF

hdiutil detach "$DEVICE"
trap - EXIT

hdiutil convert "$TEMP_DMG_PATH" -format UDZO -ov -o "$DMG_PATH"

rm -rf "$STAGING_DIR" "$TEMP_DMG_PATH"
echo "Built ${DMG_PATH}"
