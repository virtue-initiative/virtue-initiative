#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLIENT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

source "${SCRIPT_DIR}/version.sh"

BASE_VERSION="$(virtue_base_version)"
DEV_VERSION="${BASE_VERSION}-dev"

replace_line() {
  local file="$1"
  local pattern="$2"
  local replacement="$3"

  perl -0pi -e "s/${pattern}/${replacement}/gm" "$file"
}

replace_lockfile_version() {
  local file="$1"
  local package_name="$2"
  local tmp_file

  tmp_file="$(mktemp)"
  awk -v package_name="${package_name}" -v version="${BASE_VERSION}" '
    /^\[\[package\]\]$/ {
      in_package = 1
      package_matches = 0
      print
      next
    }

    in_package && /^name = "/ {
      package_matches = ($0 == "name = \"" package_name "\"")
      print
      next
    }

    in_package && package_matches && /^version = "/ {
      print "version = \"" version "\""
      package_matches = 0
      next
    }

    { print }
  ' "$file" > "${tmp_file}"
  mv "${tmp_file}" "$file"
}

replace_package_version() {
  local file="$1"
  local tmp_file

  tmp_file="$(mktemp)"
  awk -v version="${BASE_VERSION}" '
    /^\[package\]$/ {
      in_package = 1
      print
      next
    }

    /^\[/ && $0 != "[package]" {
      in_package = 0
    }

    in_package && !updated && /^version = "/ {
      print "version = \"" version "\""
      updated = 1
      next
    }

    { print }
  ' "$file" > "${tmp_file}"
  mv "${tmp_file}" "$file"
}

cargo_files=(
  "${CLIENT_ROOT}/core/Cargo.toml"
  "${CLIENT_ROOT}/linux/Cargo.toml"
  "${CLIENT_ROOT}/mac/Cargo.toml"
  "${CLIENT_ROOT}/windows/Cargo.toml"
  "${CLIENT_ROOT}/android/rust/Cargo.toml"
  "${CLIENT_ROOT}/ios/rust/Cargo.toml"
)

for cargo_file in "${cargo_files[@]}"; do
  replace_package_version "$cargo_file"
done

# One lockfile for every member: android/rust used to carry its own, but it was
# folded into the client workspace, and pointing at the old path made this script
# die under `set -e` before it reached the Xcode/manifest/API-version sync below.
# virtue-mac-ffi and virtue-text-detection are deliberately absent — they carry
# their own independent versions and are not in cargo_files either.
replace_lockfile_version "${CLIENT_ROOT}/Cargo.lock" "virtue-core"
replace_lockfile_version "${CLIENT_ROOT}/Cargo.lock" "virtue-linux"
replace_lockfile_version "${CLIENT_ROOT}/Cargo.lock" "virtue-mac"
replace_lockfile_version "${CLIENT_ROOT}/Cargo.lock" "virtue-windows"
replace_lockfile_version "${CLIENT_ROOT}/Cargo.lock" "virtue-android"
replace_lockfile_version "${CLIENT_ROOT}/Cargo.lock" "virtue-ios"

# Quote-agnostic on purpose: project.yml writes this value single-quoted, and a
# pattern that assumed double quotes matched nothing and failed silently — perl
# reports no error for a substitution that never fires. project.yml is the
# source xcodegen regenerates the .xcodeproj from, so a missed bump here quietly
# reverts MARKETING_VERSION the next time anyone runs generate-project.sh.
replace_line \
  "${CLIENT_ROOT}/ios/project.yml" \
  '^    MARKETING_VERSION: .*$' \
  "    MARKETING_VERSION: '${BASE_VERSION}'"

replace_line \
  "${CLIENT_ROOT}/ios/VirtueIOS.xcodeproj/project.pbxproj" \
  'MARKETING_VERSION = [^;]+;' \
  "MARKETING_VERSION = ${BASE_VERSION};"

replace_line \
  "${CLIENT_ROOT}/ios/app/SafariWebExtension/Resources/manifest.json" \
  '^  "version": ".*",$' \
  "  \"version\": \"${BASE_VERSION}\","

replace_line \
  "${CLIENT_ROOT}/windows/scripts/remote-windows-build.sh" \
  '^  --version <version>             Artifact label\. Default: .*$' \
  "  --version <version>             Artifact label. Default: ${DEV_VERSION}"

replace_line \
  "${CLIENT_ROOT}/windows/scripts/remote-windows-build.sh" \
  '^VERSION=".*"$' \
  "VERSION=\"${DEV_VERSION}\""

replace_line \
  "${CLIENT_ROOT}/windows/README.md" \
  '[0-9]+\.[0-9]+\.[0-9]+-dev' \
  "${DEV_VERSION}"

replace_line \
  "${CLIENT_ROOT}/windows/VM_SETUP.md" \
  '[0-9]+\.[0-9]+\.[0-9]+-dev' \
  "${DEV_VERSION}"

# --- API version sync -------------------------------------------------------
# The whole codebase — main API, hash server, and this client — shares one version,
# BASE_VERSION above. This derives its `/vX`/`/vX.Y` URL-prefix form (api/SPEC.md
# API-005, HASH-004: "For versions before v1, use v0.x") and
# writes it into every file that has to hardcode a copy because it can't import
# shared-web/api-version.ts directly (Rust, and hash-server is a separate deployable).
# shared-web/api-version.ts is the copy web/ and api/ actually import; it's written
# here too so it never has to be hand-edited.

api_version_prefix() {
  local version="$1"
  local major="${version%%.*}"
  local minor="${version#*.}"
  minor="${minor%%.*}"

  if (( major < 1 )); then
    printf 'v%s.%s\n' "${major}" "${minor}"
  else
    printf 'v%s\n' "${major}"
  fi
}

API_VERSION_PREFIX="$(api_version_prefix "${BASE_VERSION}")"

replace_line \
  "${VIRTUE_REPO_ROOT}/shared-web/api-version.ts" \
  "^export const CURRENT_API_VERSION = '.*';\$" \
  "export const CURRENT_API_VERSION = '${API_VERSION_PREFIX}';"

replace_line \
  "${VIRTUE_REPO_ROOT}/hash-server/src/api_version.rs" \
  '^const CURRENT_API_VERSION: &str = ".*";$' \
  "const CURRENT_API_VERSION: \&str = \"${API_VERSION_PREFIX}\";"

replace_line \
  "${CLIENT_ROOT}/core/src/api.rs" \
  '^const API_VERSION: &str = ".*";$' \
  "const API_VERSION: \&str = \"${API_VERSION_PREFIX}\";"

echo "Synchronized versioned files to ${BASE_VERSION}"
echo "Synchronized API version prefix to ${API_VERSION_PREFIX}"
