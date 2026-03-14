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

if ! gh release view "${TAG}" >/dev/null 2>&1; then
  exit 0
fi

while IFS=$'\t' read -r asset_id asset_name; do
  [[ -n "${asset_id}" ]] || continue

  if [[ "${asset_name}" == ${ASSET_GLOB} ]]; then
    gh api \
      --method DELETE \
      "repos/${GITHUB_REPOSITORY}/releases/assets/${asset_id}" >/dev/null
  fi
done < <(
  gh api \
    "repos/${GITHUB_REPOSITORY}/releases/tags/${TAG}" \
    --template '{{range .assets}}{{printf "%v\t%s\n" .id .name}}{{end}}'
)
