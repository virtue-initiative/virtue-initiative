#!/usr/bin/env bash
set -euo pipefail

VIRTUE_VERSION_SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
VIRTUE_CLIENT_ROOT="$(cd "${VIRTUE_VERSION_SCRIPT_DIR}/.." && pwd)"
VIRTUE_VERSION_FILE="${VIRTUE_CLIENT_ROOT}/version.properties"
VIRTUE_REPO_ROOT="$(cd "${VIRTUE_CLIENT_ROOT}/.." && pwd)"

virtue_require_version_file() {
  if [[ ! -f "${VIRTUE_VERSION_FILE}" ]]; then
    echo "Missing version file: ${VIRTUE_VERSION_FILE}" >&2
    return 1
  fi
}

virtue_version_property() {
  local key="$1"
  virtue_require_version_file
  awk -F= -v search_key="$key" '
    $1 == search_key {
      value = substr($0, index($0, "=") + 1)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", value)
      print value
      found = 1
      exit
    }
    END {
      if (!found) {
        exit 1
      }
    }
  ' "${VIRTUE_VERSION_FILE}"
}

virtue_base_version() {
  virtue_version_property "VERSION"
}

virtue_android_version_code() {
  virtue_version_property "ANDROID_VERSION_CODE"
}

virtue_apple_build_number() {
  virtue_version_property "APPLE_BUILD_NUMBER"
}

virtue_git_short_hash() {
  if [[ -n "${VIRTUE_GIT_SHORT_HASH:-}" ]]; then
    printf '%s\n' "${VIRTUE_GIT_SHORT_HASH}"
    return 0
  fi

  if [[ -n "${GITHUB_SHA:-}" ]]; then
    printf '%.7s\n' "${GITHUB_SHA}"
    return 0
  fi

  git -C "${VIRTUE_REPO_ROOT}" rev-parse --short HEAD
}

virtue_build_label() {
  printf '%s-%s\n' "$(virtue_base_version)" "$(virtue_git_short_hash)"
}

virtue_print_env() {
  printf 'VIRTUE_BASE_VERSION=%s\n' "$(virtue_base_version)"
  printf 'VIRTUE_ANDROID_VERSION_CODE=%s\n' "$(virtue_android_version_code)"
  printf 'VIRTUE_APPLE_BUILD_NUMBER=%s\n' "$(virtue_apple_build_number)"
  printf 'VIRTUE_GIT_SHORT_HASH=%s\n' "$(virtue_git_short_hash)"
  printf 'VIRTUE_BUILD_LABEL=%s\n' "$(virtue_build_label)"
}

if [[ "${BASH_SOURCE[0]}" == "${0}" ]]; then
  case "${1:-env}" in
    env)
      virtue_print_env
      ;;
    *)
      echo "Usage: $(basename "$0") [env]" >&2
      exit 1
      ;;
  esac
fi
