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

virtue_build_date() {
  if [[ -n "${VIRTUE_BUILD_DATE:-}" ]]; then
    printf '%s\n' "${VIRTUE_BUILD_DATE}"
    return 0
  fi

  date -u +%Y-%m-%d
}

virtue_git_ref_name() {
  if [[ -n "${VIRTUE_GIT_REF_NAME:-}" ]]; then
    printf '%s\n' "${VIRTUE_GIT_REF_NAME}"
    return 0
  fi

  if [[ -n "${GITHUB_REF_NAME:-}" ]]; then
    printf '%s\n' "${GITHUB_REF_NAME}"
    return 0
  fi

  local branch_name
  branch_name="$(git -C "${VIRTUE_REPO_ROOT}" rev-parse --abbrev-ref HEAD 2>/dev/null || true)"
  if [[ -n "${branch_name}" && "${branch_name}" != "HEAD" ]]; then
    printf '%s\n' "${branch_name}"
    return 0
  fi

  printf 'detached\n'
}

virtue_release_channel() {
  if [[ -n "${VIRTUE_RELEASE_CHANNEL:-}" ]]; then
    case "${VIRTUE_RELEASE_CHANNEL}" in
      stable|dev)
        printf '%s\n' "${VIRTUE_RELEASE_CHANNEL}"
        return 0
        ;;
      *)
        echo "Unsupported VIRTUE_RELEASE_CHANNEL: ${VIRTUE_RELEASE_CHANNEL}" >&2
        return 1
        ;;
    esac
  fi

  case "$(virtue_git_ref_name)" in
    main)
      printf 'stable\n'
      ;;
    *)
      printf 'dev\n'
      ;;
  esac
}

virtue_release_tag() {
  local base_version
  base_version="$(virtue_base_version)"

  if [[ "$(virtue_release_channel)" == "stable" ]]; then
    printf '%s\n' "${base_version}"
    return 0
  fi

  printf '%s-dev\n' "${base_version}"
}

virtue_build_label() {
  printf '%s-%s-%s\n' "$(virtue_release_tag)" "$(virtue_build_date)" "$(virtue_git_short_hash)"
}

# Minutes since the Unix epoch of the current commit. Used as the fourth
# CFBundleVersion component for the macOS app (see virtue_mac_bundle_version).
# Commit time — not build time — so rebuilding the same commit produces the
# same version and Sparkle correctly sees "no update", and so CI and a local
# build of the same checkout agree. Minutes (not seconds) keeps the value
# comfortably inside 32 bits for the next few centuries.
virtue_commit_minutes() {
  if [[ -n "${VIRTUE_COMMIT_MINUTES:-}" ]]; then
    printf '%s\n' "${VIRTUE_COMMIT_MINUTES}"
    return 0
  fi

  local commit_seconds
  commit_seconds="$(git -C "${VIRTUE_REPO_ROOT}" log -1 --format=%ct 2>/dev/null || true)"
  if [[ -z "${commit_seconds}" ]]; then
    # No git (e.g. a source tarball): fall back to now, which is still
    # monotonic across successive builds.
    commit_seconds="$(date -u +%s)"
  fi

  printf '%s\n' "$(( commit_seconds / 60 ))"
}

# CFBundleVersion for the macOS app: <VERSION>.<commit-minutes>.
#
# This is what Sparkle orders updates by, so it must increase for every build
# an installed app could be offered — including dev-channel builds, which all
# share one VERSION between version bumps. It is derived at build time rather
# than stored in version.properties precisely so no manual step
# (update-version.sh) is needed to make dev builds updatable.
#
# Deliberately NOT APPLE_BUILD_NUMBER: that value is shared with iOS, where
# App Store submission limits CFBundleVersion to three integer components.
# The macOS app ships via Developer ID, where four components are fine.
virtue_mac_bundle_version() {
  if [[ -n "${VIRTUE_MAC_BUNDLE_VERSION:-}" ]]; then
    printf '%s\n' "${VIRTUE_MAC_BUNDLE_VERSION}"
    return 0
  fi

  printf '%s.%s\n' "$(virtue_base_version)" "$(virtue_commit_minutes)"
}

virtue_print_env() {
  printf 'VIRTUE_BASE_VERSION=%s\n' "$(virtue_base_version)"
  printf 'VIRTUE_ANDROID_VERSION_CODE=%s\n' "$(virtue_android_version_code)"
  printf 'VIRTUE_APPLE_BUILD_NUMBER=%s\n' "$(virtue_apple_build_number)"
  printf 'VIRTUE_BUILD_DATE=%s\n' "$(virtue_build_date)"
  printf 'VIRTUE_GIT_SHORT_HASH=%s\n' "$(virtue_git_short_hash)"
  printf 'VIRTUE_GIT_REF_NAME=%s\n' "$(virtue_git_ref_name)"
  printf 'VIRTUE_RELEASE_CHANNEL=%s\n' "$(virtue_release_channel)"
  printf 'VIRTUE_RELEASE_TAG=%s\n' "$(virtue_release_tag)"
  printf 'VIRTUE_BUILD_LABEL=%s\n' "$(virtue_build_label)"
  printf 'VIRTUE_COMMIT_MINUTES=%s\n' "$(virtue_commit_minutes)"
  printf 'VIRTUE_MAC_BUNDLE_VERSION=%s\n' "$(virtue_mac_bundle_version)"
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
