#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 2 ]]; then
  echo "Usage: $(basename "$0") <base-version-properties> <head-version-properties>" >&2
  exit 1
fi

BASE_VERSION_FILE="$1"
HEAD_VERSION_FILE="$2"

if [[ ! -f "${BASE_VERSION_FILE}" ]]; then
  echo "Base version file not found: ${BASE_VERSION_FILE}" >&2
  exit 1
fi

if [[ ! -f "${HEAD_VERSION_FILE}" ]]; then
  echo "Head version file not found: ${HEAD_VERSION_FILE}" >&2
  exit 1
fi

version_property() {
  local file="$1"
  local key="$2"

  awk -F= -v search_key="${key}" '
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
  ' "${file}"
}

semver_to_numeric() {
  local version="$1"

  if [[ ! "${version}" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]]; then
    echo "Invalid semantic version: ${version}" >&2
    exit 1
  fi

  printf '%09d%09d%09d\n' "${BASH_REMATCH[1]}" "${BASH_REMATCH[2]}" "${BASH_REMATCH[3]}"
}

assert_increased_number() {
  local key="$1"
  local base_value="$2"
  local head_value="$3"

  if [[ ! "${base_value}" =~ ^[0-9]+$ ]]; then
    echo "${key} in ${BASE_VERSION_FILE} must be an integer, found: ${base_value}" >&2
    exit 1
  fi

  if [[ ! "${head_value}" =~ ^[0-9]+$ ]]; then
    echo "${key} in ${HEAD_VERSION_FILE} must be an integer, found: ${head_value}" >&2
    exit 1
  fi

  if (( head_value <= base_value )); then
    echo "${key} must increase for pull requests into main. Base=${base_value}, head=${head_value}." >&2
    exit 1
  fi
}

base_version="$(version_property "${BASE_VERSION_FILE}" "VERSION")"
head_version="$(version_property "${HEAD_VERSION_FILE}" "VERSION")"
base_android_version_code="$(version_property "${BASE_VERSION_FILE}" "ANDROID_VERSION_CODE")"
head_android_version_code="$(version_property "${HEAD_VERSION_FILE}" "ANDROID_VERSION_CODE")"
base_apple_build_number="$(version_property "${BASE_VERSION_FILE}" "APPLE_BUILD_NUMBER")"
head_apple_build_number="$(version_property "${HEAD_VERSION_FILE}" "APPLE_BUILD_NUMBER")"

if [[ "$(semver_to_numeric "${head_version}")" -le "$(semver_to_numeric "${base_version}")" ]]; then
  echo "VERSION must increase for pull requests into main. Base=${base_version}, head=${head_version}." >&2
  exit 1
fi

assert_increased_number "ANDROID_VERSION_CODE" "${base_android_version_code}" "${head_android_version_code}"
assert_increased_number "APPLE_BUILD_NUMBER" "${base_apple_build_number}" "${head_apple_build_number}"

echo "Version bump check passed: ${base_version} -> ${head_version}"
