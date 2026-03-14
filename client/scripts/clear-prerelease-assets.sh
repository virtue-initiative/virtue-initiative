#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: $(basename "$0") <release-tag> <asset-glob>" >&2
  exit 1
fi

TAG="$1"
ASSET_GLOB="$2"

if [[ -z "${GH_TOKEN:-}" ]]; then
  echo "GH_TOKEN is required" >&2
  exit 1
fi

if [[ -z "${GITHUB_REPOSITORY:-}" ]]; then
  echo "GITHUB_REPOSITORY is required" >&2
  exit 1
fi

if ! command -v gh >/dev/null 2>&1; then
  echo "gh CLI is required" >&2
  exit 1
fi

if ! gh release view "${TAG}" -R "${GITHUB_REPOSITORY}" >/dev/null 2>&1; then
  exit 0
fi

while IFS=$'\t' read -r asset_id asset_name; do
  [[ -n "${asset_name}" ]] || continue

  if [[ "${asset_name}" == ${ASSET_GLOB} ]]; then
    echo "Removing prerelease asset: ${asset_name}"
    gh release delete-asset "${TAG}" "${asset_name}" --yes -R "${GITHUB_REPOSITORY}" >/dev/null
  fi
done < <(
  gh release view "${TAG}" \
    -R "${GITHUB_REPOSITORY}" \
    --json assets \
    --template '{{range .assets}}{{printf "%v\t%s\n" .id .name}}{{end}}'
)
