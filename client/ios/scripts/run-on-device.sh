#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IOS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
DERIVED_DATA_PATH="$IOS_DIR/.derived-data-device"
TEAM_ID="${TEAM_ID:-6277E5UTS9}"
BUNDLE_ID="${BUNDLE_ID:-org.virtueinitiative.virtueios}"
DEVICE_UDID="${DEVICE_UDID:-}"

usage() {
  cat <<EOF
Usage: $(basename "$0") [options]

Options:
  --team-id <apple development team id>    Optional; default: ${TEAM_ID}
  --bundle-id <bundle identifier>          Default: ${BUNDLE_ID}
  --device-udid <physical device udid>     Optional; auto-selects first connected iOS device
  --derived-data <path>                    Default: ${DERIVED_DATA_PATH}

Environment alternatives:
  TEAM_ID, BUNDLE_ID, DEVICE_UDID
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --team-id)
      TEAM_ID="$2"
      shift 2
      ;;
    --bundle-id)
      BUNDLE_ID="$2"
      shift 2
      ;;
    --device-udid)
      DEVICE_UDID="$2"
      shift 2
      ;;
    --derived-data)
      DERIVED_DATA_PATH="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

if [[ -z "$DEVICE_UDID" ]]; then
  DEVICES_JSON="$(mktemp)"
  trap 'rm -f "$DEVICES_JSON"' EXIT
  xcrun devicectl list devices --json-output "$DEVICES_JSON" >/dev/null
  DEVICE_UDID="$(
    jq -r '
      .result.devices[]
      | select(.hardwareProperties.platform == "iOS")
      | select(.hardwareProperties.reality == "physical")
      | select(.deviceProperties.ddiServicesAvailable == true)
      | .hardwareProperties.udid
    ' "$DEVICES_JSON" | head -n1
  )"

  if [[ -z "$DEVICE_UDID" ]]; then
    DEVICE_UDID="$(
      jq -r '
        .result.devices[]
        | select(.hardwareProperties.platform == "iOS")
        | select(.hardwareProperties.reality == "physical")
        | .hardwareProperties.udid
      ' "$DEVICES_JSON" | head -n1
    )"
    if [[ -n "$DEVICE_UDID" ]]; then
      echo "Warning: selected device with ddiServicesAvailable=false." >&2
      echo "If install fails, unlock/trust the phone and open it once in Xcode." >&2
    fi
  fi
fi

if [[ -z "$DEVICE_UDID" ]]; then
  echo "No connected physical iOS device found." >&2
  echo "Connect/unlock/trust the phone, then retry with --device-udid if needed." >&2
  exit 1
fi

"$SCRIPT_DIR/build-ios.sh" \
  --destination "id=${DEVICE_UDID}" \
  --team-id "$TEAM_ID" \
  --bundle-id "$BUNDLE_ID" \
  --derived-data "$DERIVED_DATA_PATH"

APP_PATH="$DERIVED_DATA_PATH/Build/Products/Debug-iphoneos/VirtueIOS.app"
if [[ ! -d "$APP_PATH" ]]; then
  echo "Built app not found at $APP_PATH" >&2
  exit 1
fi

xcrun devicectl device install app --device "$DEVICE_UDID" "$APP_PATH"
xcrun devicectl device process launch --device "$DEVICE_UDID" "$BUNDLE_ID"

echo "Installed and launched ${BUNDLE_ID} on ${DEVICE_UDID}"
