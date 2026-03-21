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

replace_lockfile_version "${CLIENT_ROOT}/Cargo.lock" "virtue-core"
replace_lockfile_version "${CLIENT_ROOT}/Cargo.lock" "virtue-linux"
replace_lockfile_version "${CLIENT_ROOT}/Cargo.lock" "virtue-mac"
replace_lockfile_version "${CLIENT_ROOT}/Cargo.lock" "virtue-windows"
replace_lockfile_version "${CLIENT_ROOT}/android/rust/Cargo.lock" "virtue-android"
replace_lockfile_version "${CLIENT_ROOT}/android/rust/Cargo.lock" "virtue-core"

replace_line \
  "${CLIENT_ROOT}/ios/project.yml" \
  '^    MARKETING_VERSION: ".*"$' \
  "    MARKETING_VERSION: \"${BASE_VERSION}\""

replace_line \
  "${CLIENT_ROOT}/ios/VirtueIOS.xcodeproj/project.pbxproj" \
  'MARKETING_VERSION = [^;]+;' \
  "MARKETING_VERSION = ${BASE_VERSION};"

replace_line \
  "${CLIENT_ROOT}/ios/app/SafariWebExtension/Resources/manifest.json" \
  '^  "version": ".*",$' \
  "  \"version\": \"${BASE_VERSION}\","

replace_line \
  "${CLIENT_ROOT}/windows/packaging/nsis/installer.nsi" \
  '^!define PRODUCT_VERSION ".*"$' \
  "!define PRODUCT_VERSION \"${BASE_VERSION}\""

replace_line \
  "${CLIENT_ROOT}/windows/scripts/remote-windows-build.sh" \
  '^  --version <version>             Installer version\. Default: .*$' \
  "  --version <version>             Installer version. Default: ${DEV_VERSION}"

replace_line \
  "${CLIENT_ROOT}/windows/scripts/remote-windows-build.sh" \
  '^VERSION=".*"$' \
  "VERSION=\"${DEV_VERSION}\""

replace_line \
  "${CLIENT_ROOT}/windows/README.md" \
  'build-installer\.sh -Version [0-9]+\.[0-9]+\.[0-9]+ -Profile Debug' \
  "build-installer.sh -Version ${BASE_VERSION} -Profile Debug"

replace_line \
  "${CLIENT_ROOT}/windows/README.md" \
  'build-installer\.ps1 -Version [0-9]+\.[0-9]+\.[0-9]+ -Profile Debug' \
  "build-installer.ps1 -Version ${BASE_VERSION} -Profile Debug"

replace_line \
  "${CLIENT_ROOT}/windows/README.md" \
  '[0-9]+\.[0-9]+\.[0-9]+-dev' \
  "${DEV_VERSION}"

replace_line \
  "${CLIENT_ROOT}/windows/VM_SETUP.md" \
  '[0-9]+\.[0-9]+\.[0-9]+-dev' \
  "${DEV_VERSION}"

echo "Synchronized versioned files to ${BASE_VERSION}"
