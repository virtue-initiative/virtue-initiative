#!/bin/sh
set -eu

REPO="virtue-initiative/virtue-initiative"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
RELEASE_TAG_FILE="$SCRIPT_DIR/release-tag"
BUILD_LABEL_FILE="$SCRIPT_DIR/build-label"
LOCK_FILE="/run/lock/virtue-update.lock"

exec 9>"$LOCK_FILE"
if ! flock -n 9; then
  echo "virtue-update-check: another instance is already running, exiting" >&2
  exit 0
fi

if [ ! -f "$RELEASE_TAG_FILE" ] || [ ! -f "$BUILD_LABEL_FILE" ]; then
  echo "virtue-update-check: missing $RELEASE_TAG_FILE or $BUILD_LABEL_FILE" >&2
  exit 1
fi

RELEASE_TAG="$(cat "$RELEASE_TAG_FILE")"
LOCAL_BUILD_LABEL="$(cat "$BUILD_LABEL_FILE")"
ARCH="$(dpkg --print-architecture)"
RELEASES_URL="${VIRTUE_RELEASES_URL:-https://api.github.com/repos/$REPO/releases?per_page=30}"

# The baked-in tag only tells us which channel this build came from. Each
# version bump publishes to a brand-new tag, so following one fixed tag would
# strand every install at the version it shipped with. Dev builds track the
# newest prerelease, stable builds the newest full release.
case "$RELEASE_TAG" in
  *-dev) WANT_PRERELEASE=true; CHANNEL=dev ;;
  *) WANT_PRERELEASE=false; CHANNEL=stable ;;
esac

echo "virtue-update-check: checking $CHANNEL channel (local build '$LOCAL_BUILD_LABEL', arch '$ARCH')"

RELEASES_JSON="$(curl -fsSL "$RELEASES_URL")"

# Newest release in the channel that already has a .deb for this arch. The
# release is created by whichever platform job finishes first, so the newest
# one can briefly have no Linux asset; skip it rather than doing nothing.
ASSET_URL="$(printf '%s' "$RELEASES_JSON" | jq -r --arg arch "$ARCH" --argjson pre "$WANT_PRERELEASE" '
  [ .[]
    | select((.draft | not) and .prerelease == $pre)
    | . as $release
    | ($release.assets | map(select(.name | test("^virtue-linux_.*_" + $arch + "\\.deb$"))) | .[0]) as $asset
    | select($asset != null)
    | { published_at: $release.published_at, url: $asset.browser_download_url }
  ]
  | sort_by(.published_at) | reverse | .[0].url // empty
')"

if [ -z "$ASSET_URL" ]; then
  echo "virtue-update-check: no matching $CHANNEL release asset found for '$ARCH', nothing to do"
  exit 0
fi

ASSET_NAME="$(basename "$ASSET_URL")"
# Asset names are virtue-linux_<build_label>_<arch>.deb; strip the fixed
# prefix/suffix to recover the embedded build label for comparison.
REMOTE_BUILD_LABEL="${ASSET_NAME#virtue-linux_}"
REMOTE_BUILD_LABEL="${REMOTE_BUILD_LABEL%"_${ARCH}.deb"}"

if [ "$REMOTE_BUILD_LABEL" = "$LOCAL_BUILD_LABEL" ]; then
  echo "virtue-update-check: already up to date ($LOCAL_BUILD_LABEL), nothing to do"
  exit 0
fi

echo "virtue-update-check: new build available ($LOCAL_BUILD_LABEL -> $REMOTE_BUILD_LABEL), downloading"

TMP_DEB="$(mktemp --suffix=.deb)"
trap 'rm -f "$TMP_DEB"' EXIT

curl -fsSL -o "$TMP_DEB" "$ASSET_URL"

if [ ! -s "$TMP_DEB" ] || ! dpkg-deb --info "$TMP_DEB" >/dev/null 2>&1; then
  echo "virtue-update-check: downloaded file is not a valid .deb" >&2
  exit 1
fi

echo "virtue-update-check: installing $ASSET_NAME"
if ! dpkg -i "$TMP_DEB"; then
  echo "virtue-update-check: dpkg -i failed, retrying with apt-get install -f" >&2
  apt-get install -f -y
fi

echo "virtue-update-check: update to $REMOTE_BUILD_LABEL complete"
