#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 3 ]]; then
  echo "Usage: $(basename "$0") <release-tag> <build-label> <asset-path>" >&2
  exit 1
fi

TAG="$1"
BUILD_LABEL="$2"
ASSET_PATH="$3"
TITLE="${TAG}"
TARGET="${GITHUB_SHA:-}"

if [[ -z "${GH_TOKEN:-}" ]]; then
  echo "GH_TOKEN is required" >&2
  exit 1
fi

if [[ -z "${GITHUB_REPOSITORY:-}" ]]; then
  echo "GITHUB_REPOSITORY is required" >&2
  exit 1
fi

if [[ -z "${TARGET}" ]]; then
  echo "GITHUB_SHA is required" >&2
  exit 1
fi

if [[ ! -f "${ASSET_PATH}" ]]; then
  echo "Asset not found: ${ASSET_PATH}" >&2
  exit 1
fi

if ! command -v gh >/dev/null 2>&1; then
  echo "gh CLI is required" >&2
  exit 1
fi

notes_file="$(mktemp)"
trap 'rm -f "${notes_file}"' EXIT
cat > "${notes_file}" <<EOF
Automated dev release for commit \`${TARGET}\`.

Build label: \`${BUILD_LABEL}\`
EOF

if gh api "repos/${GITHUB_REPOSITORY}/git/ref/tags/${TAG}" >/dev/null 2>&1; then
  gh api \
    --method PATCH \
    "repos/${GITHUB_REPOSITORY}/git/refs/tags/${TAG}" \
    -f sha="${TARGET}" \
    -F force=true >/dev/null
else
  gh api \
    --method POST \
    "repos/${GITHUB_REPOSITORY}/git/refs" \
    -f ref="refs/tags/${TAG}" \
    -f sha="${TARGET}" >/dev/null
fi

if gh release view "${TAG}" >/dev/null 2>&1; then
  gh release edit "${TAG}" \
    --target "${TARGET}" \
    --title "${TITLE}" \
    --prerelease \
    --notes-file "${notes_file}" >/dev/null
else
  gh release create "${TAG}" \
    --verify-tag \
    --title "${TITLE}" \
    --prerelease \
    --notes-file "${notes_file}" >/dev/null
fi

gh release upload "${TAG}" "${ASSET_PATH}" --clobber
