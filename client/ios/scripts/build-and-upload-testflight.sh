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
DERIVED_DATA_PATH="$BUILD_DIR/derived-data"

: "${IOS_TEAM_ID:?IOS_TEAM_ID is required}"
: "${IOS_ASC_KEY_ID:?IOS_ASC_KEY_ID is required}"
: "${IOS_ASC_ISSUER_ID:?IOS_ASC_ISSUER_ID is required}"
: "${IOS_ASC_API_KEY_PATH:?IOS_ASC_API_KEY_PATH is required (path to the .p8 file)}"

source "${CLIENT_ROOT}/scripts/version.sh"

MARKETING_VERSION="$(virtue_base_version)"
CURRENT_PROJECT_VERSION="$(virtue_apple_build_number).${GITHUB_RUN_NUMBER:-0}"
VIRTUE_BUILD_LABEL="$(virtue_build_label)"

rm -rf "$BUILD_DIR"
mkdir -p "$EXPORT_PATH"

echo "Archiving ${SCHEME} ${MARKETING_VERSION} (build ${CURRENT_PROJECT_VERSION})"

xcodebuild archive \
  -project "$PROJECT_PATH" \
  -scheme "$SCHEME" \
  -configuration Release \
  -destination "generic/platform=iOS" \
  -archivePath "$ARCHIVE_PATH" \
  -derivedDataPath "$DERIVED_DATA_PATH" \
  -allowProvisioningUpdates \
  -authenticationKeyPath "$IOS_ASC_API_KEY_PATH" \
  -authenticationKeyID "$IOS_ASC_KEY_ID" \
  -authenticationKeyIssuerID "$IOS_ASC_ISSUER_ID" \
  VIRTUE_APP_BUNDLE_ID=org.virtueinitiative.virtueios \
  MARKETING_VERSION="$MARKETING_VERSION" \
  CURRENT_PROJECT_VERSION="$CURRENT_PROJECT_VERSION" \
  VIRTUE_BUILD_LABEL="$VIRTUE_BUILD_LABEL" \
  CODE_SIGN_STYLE=Automatic \
  CODE_SIGN_IDENTITY="Apple Distribution" \
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
	<string>automatic</string>
	<key>uploadSymbols</key>
	<true/>
</dict>
</plist>
PLIST

xcodebuild -exportArchive \
  -archivePath "$ARCHIVE_PATH" \
  -exportPath "$EXPORT_PATH" \
  -exportOptionsPlist "$EXPORT_PATH/ExportOptions.plist" \
  -allowProvisioningUpdates \
  -authenticationKeyPath "$IOS_ASC_API_KEY_PATH" \
  -authenticationKeyID "$IOS_ASC_KEY_ID" \
  -authenticationKeyIssuerID "$IOS_ASC_ISSUER_ID"

IPA_PATH="$EXPORT_PATH/VirtueIOS.ipa"
if [[ ! -f "$IPA_PATH" ]]; then
  echo "Exported .ipa not found: $IPA_PATH" >&2
  exit 1
fi

echo "Uploading ${IPA_PATH} to TestFlight"

xcrun altool --upload-app \
  --type ios \
  --file "$IPA_PATH" \
  --apiKey "$IOS_ASC_KEY_ID" \
  --apiIssuer "$IOS_ASC_ISSUER_ID"

echo "Upload complete"
