#!/usr/bin/env bash
set -euo pipefail

# Renders the Sparkle appcast <item> for a built DMG, EdDSA-signed with the
# release key.
#
# The fragment is published as a release asset next to the DMG rather than a
# whole appcast, because the feed itself is assembled elsewhere: the DMG and
# its signature only exist on a macOS runner (sign_update is a macOS binary,
# and the key must not travel to the Linux runner), while the feed is served
# by the landing site, which already polls the releases API at deploy time.
# See landing/scripts/build-appcast.mjs.
#
# Usage:
#   SPARKLE_ED_PRIVATE_KEY=... make-appcast-item.sh <dmg-path> <download-url> <channel> <output-path>
#
# <channel> is "stable" or "dev"; a dev item carries <sparkle:channel>dev, which
# only dev-channel builds opt into (UpdateController.allowedChannels).

if [[ $# -ne 4 ]]; then
  echo "Usage: $(basename "$0") <dmg-path> <download-url> <channel> <output-path>" >&2
  exit 1
fi

DMG_PATH="$1"
DOWNLOAD_URL="$2"
CHANNEL="$3"
OUTPUT_PATH="$4"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLIENT_ROOT="$(cd "${SCRIPT_DIR}/../.." && pwd)"

# Pinned deliberately: the tool that signs releases should not float. Keep in
# step with the Sparkle version in mac/project.yml.
SPARKLE_TOOLS_VERSION="2.9.6"

if [[ ! -f "$DMG_PATH" ]]; then
  echo "DMG not found: ${DMG_PATH}" >&2
  exit 1
fi

case "$CHANNEL" in
  stable|dev) ;;
  *)
    echo "Unsupported channel: ${CHANNEL} (expected stable|dev)" >&2
    exit 1
    ;;
esac

if [[ -z "${SPARKLE_ED_PRIVATE_KEY:-}" ]]; then
  echo "SPARKLE_ED_PRIVATE_KEY is required to sign an appcast item." >&2
  exit 1
fi

source "${CLIENT_ROOT}/scripts/version.sh"

BASE_VERSION="$(virtue_base_version)"
BUNDLE_VERSION="$(virtue_mac_bundle_version)"
MINIMUM_SYSTEM_VERSION="13.0"

# Fetch sign_update. Only the signing tool is needed, not the framework.
TOOLS_DIR="$(mktemp -d)"
trap 'rm -rf "$TOOLS_DIR"' EXIT

curl -fsSL -o "${TOOLS_DIR}/Sparkle.tar.xz" \
  "https://github.com/sparkle-project/Sparkle/releases/download/${SPARKLE_TOOLS_VERSION}/Sparkle-${SPARKLE_TOOLS_VERSION}.tar.xz"
tar -xJf "${TOOLS_DIR}/Sparkle.tar.xz" -C "$TOOLS_DIR" bin/sign_update

# sign_update prints the enclosure attributes ready to paste, e.g.
#   sparkle:edSignature="…" length="12345"
SIGNATURE_ATTRS="$(printf '%s' "$SPARKLE_ED_PRIVATE_KEY" | "${TOOLS_DIR}/bin/sign_update" --ed-key-file - "$DMG_PATH")"

if [[ -z "$SIGNATURE_ATTRS" ]]; then
  echo "sign_update produced no signature for ${DMG_PATH}" >&2
  exit 1
fi

CHANNEL_ELEMENT=""
if [[ "$CHANNEL" == "dev" ]]; then
  CHANNEL_ELEMENT=$'\n  <sparkle:channel>dev</sparkle:channel>'
fi

PUB_DATE="$(date -u '+%a, %d %b %Y %H:%M:%S +0000')"

mkdir -p "$(dirname "$OUTPUT_PATH")"
# Emitted unindented; the feed assembler owns the final indentation.
cat > "$OUTPUT_PATH" <<EOF
<item>
  <title>${BASE_VERSION}</title>
  <pubDate>${PUB_DATE}</pubDate>${CHANNEL_ELEMENT}
  <sparkle:version>${BUNDLE_VERSION}</sparkle:version>
  <sparkle:shortVersionString>${BASE_VERSION}</sparkle:shortVersionString>
  <sparkle:minimumSystemVersion>${MINIMUM_SYSTEM_VERSION}</sparkle:minimumSystemVersion>
  <enclosure url="${DOWNLOAD_URL}" type="application/octet-stream" ${SIGNATURE_ATTRS} />
</item>
EOF

echo "Wrote appcast item for ${BUNDLE_VERSION} (${CHANNEL}) to ${OUTPUT_PATH}"
