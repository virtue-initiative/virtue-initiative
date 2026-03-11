#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IOS_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
CLIENT_ROOT="$(cd "${IOS_DIR}/.." && pwd)"
PROJECT_PATH="$IOS_DIR/VirtueIOS.xcodeproj"
SCHEME="VirtueIOS"
CONFIGURATION="Debug"
DERIVED_DATA_PATH="$IOS_DIR/.derived-data"
DESTINATION="generic/platform=iOS Simulator"
TEAM_ID="${TEAM_ID:-}"
BUNDLE_ID="${BUNDLE_ID:-org.virtueinitiative.virtueios}"
CODE_SIGNING_ALLOWED="${CODE_SIGNING_ALLOWED:-YES}"

source "${CLIENT_ROOT}/scripts/version.sh"

MARKETING_VERSION="$(virtue_base_version)"
CURRENT_PROJECT_VERSION="$(virtue_apple_build_number)"
VIRTUE_BUILD_LABEL="$(virtue_build_label)"

usage() {
  cat <<EOF
Usage: $(basename "$0") [options]

Options:
  --destination <xcodebuild destination>   Default: ${DESTINATION}
  --team-id <apple development team id>    Optional for simulator, required for device signing
  --bundle-id <bundle identifier>          Default: ${BUNDLE_ID}
  --configuration <Debug|Release>          Default: ${CONFIGURATION}
  --derived-data <path>                    Default: ${DERIVED_DATA_PATH}
  --code-signing-allowed <YES|NO>          Default: ${CODE_SIGNING_ALLOWED}
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --destination)
      DESTINATION="$2"
      shift 2
      ;;
    --team-id)
      TEAM_ID="$2"
      shift 2
      ;;
    --bundle-id)
      BUNDLE_ID="$2"
      shift 2
      ;;
    --configuration)
      CONFIGURATION="$2"
      shift 2
      ;;
    --derived-data)
      DERIVED_DATA_PATH="$2"
      shift 2
      ;;
    --code-signing-allowed)
      CODE_SIGNING_ALLOWED="$2"
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

ARGS=(
  -project "$PROJECT_PATH"
  -scheme "$SCHEME"
  -configuration "$CONFIGURATION"
  -destination "$DESTINATION"
  -derivedDataPath "$DERIVED_DATA_PATH"
  VIRTUE_APP_BUNDLE_ID="$BUNDLE_ID"
  MARKETING_VERSION="$MARKETING_VERSION"
  CURRENT_PROJECT_VERSION="$CURRENT_PROJECT_VERSION"
  VIRTUE_BUILD_LABEL="$VIRTUE_BUILD_LABEL"
  CODE_SIGNING_ALLOWED="$CODE_SIGNING_ALLOWED"
)

if [[ -n "$TEAM_ID" ]]; then
  ARGS+=(
    DEVELOPMENT_TEAM="$TEAM_ID"
    CODE_SIGN_STYLE=Automatic
    -allowProvisioningUpdates
  )
fi

xcodebuild "${ARGS[@]}" build

echo "Build complete"
echo "Derived data: $DERIVED_DATA_PATH"
