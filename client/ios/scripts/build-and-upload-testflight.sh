#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IOS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
CLIENT_ROOT="$(cd "${IOS_DIR}/.." && pwd)"
PROJECT_PATH="$IOS_DIR/VirtueIOS.xcodeproj"
SCHEME="VirtueIOS"
BUILD_DIR="$IOS_DIR/.build-testflight"
ARCHIVE_PATH="$BUILD_DIR/VirtueIOS.xcarchive"
EXPORT_PATH="$BUILD_DIR/export"
# Reuses the derived data build-ios.sh already populated earlier in the same CI
# job (client-ios.yml's "Build iOS app bundle" step), so this archive step
# relinks/re-signs already-compiled object files instead of recompiling the
# whole app from scratch.
DERIVED_DATA_PATH="$IOS_DIR/.derived-data-ci-ios"

: "${IOS_TEAM_ID:?IOS_TEAM_ID is required}"
: "${IOS_ASC_KEY_ID:?IOS_ASC_KEY_ID is required}"
: "${IOS_ASC_ISSUER_ID:?IOS_ASC_ISSUER_ID is required}"
: "${IOS_ASC_API_KEY_PATH:?IOS_ASC_API_KEY_PATH is required (path to the .p8 file)}"
: "${IOS_APP_PROVISIONING_PROFILE_PATH:?IOS_APP_PROVISIONING_PROFILE_PATH is required (path to the app .mobileprovision file)}"
: "${IOS_EXT_PROVISIONING_PROFILE_PATH:?IOS_EXT_PROVISIONING_PROFILE_PATH is required (path to the extension .mobileprovision file)}"

APP_PROFILE_NAME='Virtue iOS App Store'
EXT_PROFILE_NAME='Virtue iOS Safari Ext App Store'

source "${CLIENT_ROOT}/scripts/version.sh"

MARKETING_VERSION="$(virtue_base_version)"
CURRENT_PROJECT_VERSION="$(virtue_apple_build_number).${GITHUB_RUN_NUMBER:-0}"
VIRTUE_BUILD_LABEL="$(virtue_build_label)"

rm -rf "$BUILD_DIR"
mkdir -p "$EXPORT_PATH"

PROFILES_DIR="$HOME/Library/MobileDevice/Provisioning Profiles"
mkdir -p "$PROFILES_DIR"
cp "$IOS_APP_PROVISIONING_PROFILE_PATH" "$PROFILES_DIR/virtue-ios-app-store.mobileprovision"
cp "$IOS_EXT_PROVISIONING_PROFILE_PATH" "$PROFILES_DIR/virtue-ios-safari-ext-app-store.mobileprovision"

# altool only looks for the API key by ID in a fixed set of directories; it
# doesn't accept an arbitrary path like xcodebuild's -authenticationKeyPath does.
ASC_KEYS_DIR="$HOME/.appstoreconnect/private_keys"
mkdir -p "$ASC_KEYS_DIR"
cp "$IOS_ASC_API_KEY_PATH" "$ASC_KEYS_DIR/AuthKey_${IOS_ASC_KEY_ID}.p8"

echo "Archiving ${SCHEME} ${MARKETING_VERSION} (build ${CURRENT_PROJECT_VERSION})"

xcodebuild archive \
  -project "$PROJECT_PATH" \
  -scheme "$SCHEME" \
  -configuration Release \
  -destination "generic/platform=iOS" \
  -archivePath "$ARCHIVE_PATH" \
  -derivedDataPath "$DERIVED_DATA_PATH" \
  VIRTUE_APP_BUNDLE_ID=org.virtueinitiative.virtueios \
  MARKETING_VERSION="$MARKETING_VERSION" \
  CURRENT_PROJECT_VERSION="$CURRENT_PROJECT_VERSION" \
  VIRTUE_BUILD_LABEL="$VIRTUE_BUILD_LABEL" \
  DEVELOPMENT_TEAM="$IOS_TEAM_ID"

cat > "$EXPORT_PATH/ExportOptions.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>method</key>
	<string>app-store-connect</string>
	<key>teamID</key>
	<string>${IOS_TEAM_ID}</string>
	<key>signingStyle</key>
	<string>manual</string>
	<key>provisioningProfiles</key>
	<dict>
		<key>org.virtueinitiative.virtueios</key>
		<string>${APP_PROFILE_NAME}</string>
		<key>org.virtueinitiative.virtueios.broadcast</key>
		<string>${EXT_PROFILE_NAME}</string>
	</dict>
	<key>uploadSymbols</key>
	<true/>
</dict>
</plist>
PLIST

xcodebuild -exportArchive \
  -archivePath "$ARCHIVE_PATH" \
  -exportPath "$EXPORT_PATH" \
  -exportOptionsPlist "$EXPORT_PATH/ExportOptions.plist"

IPA_PATH="$(find "$EXPORT_PATH" -maxdepth 1 -name '*.ipa' -print -quit)"
if [[ -z "$IPA_PATH" || ! -f "$IPA_PATH" ]]; then
  echo "No exported .ipa found under: $EXPORT_PATH" >&2
  ls -la "$EXPORT_PATH" >&2 || true
  exit 1
fi

echo "Uploading ${IPA_PATH} to TestFlight"

xcrun altool --upload-app \
  --type ios \
  --file "$IPA_PATH" \
  --apiKey "$IOS_ASC_KEY_ID" \
  --apiIssuer "$IOS_ASC_ISSUER_ID"

echo "Upload complete"
