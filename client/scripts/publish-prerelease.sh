#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: $(basename "$0") <build-label> <asset-path>" >&2
  exit 1
fi

BUILD_LABEL="$1"
ASSET_PATH="$2"
TAG="build-${BUILD_LABEL}"
TITLE="Build ${BUILD_LABEL}"
TARGET="${GITHUB_SHA:-}"

if [[ -z "${GH_TOKEN:-}" ]]; then
  echo "GH_TOKEN is required" >&2
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
Automated prerelease for commit \`${TARGET}\`.

Build label: \`${BUILD_LABEL}\`
EOF

for attempt in 1 2 3 4 5; do
  if gh release view "${TAG}" >/dev/null 2>&1; then
    break
  fi

  if gh release create "${TAG}" \
    --target "${TARGET}" \
    --title "${TITLE}" \
    --prerelease \
    --notes-file "${notes_file}" >/dev/null 2>&1; then
    break
  fi

  if [[ "${attempt}" -eq 5 ]]; then
    echo "Failed to create or discover release ${TAG}" >&2
    exit 1
  fi

  sleep 3
done

gh release upload "${TAG}" "${ASSET_PATH}" --clobber
