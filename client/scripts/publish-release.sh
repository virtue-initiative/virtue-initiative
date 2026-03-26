#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 4 ]]; then
  echo "Usage: $(basename "$0") <release-tag> <build-label> <asset-path> <release-channel>" >&2
  exit 1
fi

TAG="$1"
BUILD_LABEL="$2"
ASSET_PATH="$3"
RELEASE_CHANNEL="$4"
TARGET="${GITHUB_SHA:-}"

case "${RELEASE_CHANNEL}" in
  stable)
    IS_PRERELEASE=0
    NOTES_HEADER="Automated stable release"
    ;;
  dev)
    IS_PRERELEASE=1
    NOTES_HEADER="Automated dev release"
    ;;
  *)
    echo "Unsupported release channel: ${RELEASE_CHANNEL}" >&2
    exit 1
    ;;
esac

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

tag_ref_endpoint="repos/${GITHUB_REPOSITORY}/git/ref/tags/${TAG}"
tag_refs_endpoint="repos/${GITHUB_REPOSITORY}/git/refs"

fetch_tag_sha() {
  gh api "${tag_ref_endpoint}" --jq '.object.sha' 2>/dev/null
}

ensure_tag() {
  local existing_sha
  existing_sha="$(fetch_tag_sha || true)"

  if [[ -n "${existing_sha}" ]]; then
    if [[ "${IS_PRERELEASE}" -eq 1 ]]; then
      if [[ "${existing_sha}" != "${TARGET}" ]]; then
        gh api \
          --method PATCH \
          "repos/${GITHUB_REPOSITORY}/git/refs/tags/${TAG}" \
          -f sha="${TARGET}" \
          -F force=true >/dev/null
      fi
      return 0
    fi

    if [[ "${existing_sha}" != "${TARGET}" ]]; then
      echo "Stable tag ${TAG} already exists at ${existing_sha} and will not be moved to ${TARGET}." >&2
      exit 1
    fi
    return 0
  fi

  if gh api \
    --method POST \
    "${tag_refs_endpoint}" \
    -f ref="refs/tags/${TAG}" \
    -f sha="${TARGET}" >/dev/null 2>&1; then
    return 0
  fi

  existing_sha="$(fetch_tag_sha || true)"

  if [[ -z "${existing_sha}" ]]; then
    echo "Failed to create tag ${TAG}." >&2
    exit 1
  fi

  if [[ "${IS_PRERELEASE}" -eq 1 ]]; then
    if [[ "${existing_sha}" != "${TARGET}" ]]; then
      gh api \
        --method PATCH \
        "repos/${GITHUB_REPOSITORY}/git/refs/tags/${TAG}" \
        -f sha="${TARGET}" \
        -F force=true >/dev/null
    fi
    return 0
  fi

  if [[ "${existing_sha}" != "${TARGET}" ]]; then
    echo "Stable tag ${TAG} was created concurrently for ${existing_sha}, not ${TARGET}." >&2
    exit 1
  fi
}

ensure_release() {
  local notes_file
  notes_file="$(mktemp)"
  trap 'rm -f "${notes_file}"' RETURN

  cat > "${notes_file}" <<EOF
${NOTES_HEADER} for commit \`${TARGET}\`.

Build label: \`${BUILD_LABEL}\`
EOF

  if gh release view "${TAG}" -R "${GITHUB_REPOSITORY}" >/dev/null 2>&1; then
    local edit_args=(
      "${TAG}"
      "--target" "${TARGET}"
      "--title" "${TAG}"
      "--notes-file" "${notes_file}"
      "-R" "${GITHUB_REPOSITORY}"
    )

    if [[ "${IS_PRERELEASE}" -eq 1 ]]; then
      edit_args+=("--prerelease")
    fi

    gh release edit "${edit_args[@]}" >/dev/null
    return 0
  fi

  local create_args=(
    "${TAG}"
    "--verify-tag"
    "--target" "${TARGET}"
    "--title" "${TAG}"
    "--notes-file" "${notes_file}"
    "-R" "${GITHUB_REPOSITORY}"
  )

  if [[ "${IS_PRERELEASE}" -eq 1 ]]; then
    create_args+=("--prerelease")
  fi

  if gh release create "${create_args[@]}" >/dev/null 2>&1; then
    return 0
  fi

  if gh release view "${TAG}" -R "${GITHUB_REPOSITORY}" >/dev/null 2>&1; then
    local retry_edit_args=(
      "${TAG}"
      "--target" "${TARGET}"
      "--title" "${TAG}"
      "--notes-file" "${notes_file}"
      "-R" "${GITHUB_REPOSITORY}"
    )

    if [[ "${IS_PRERELEASE}" -eq 1 ]]; then
      retry_edit_args+=("--prerelease")
    fi

    gh release edit "${retry_edit_args[@]}" >/dev/null
    return 0
  fi

  echo "Failed to create or update release ${TAG}." >&2
  exit 1
}

ensure_tag
ensure_release

gh release upload "${TAG}" "${ASSET_PATH}" --clobber -R "${GITHUB_REPOSITORY}"
