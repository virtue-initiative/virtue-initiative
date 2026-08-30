#!/usr/bin/env bash
# Build the APK (+ Rust JNI core) only. Usage: build.sh [--release]
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/lib.sh"

RELEASE=0
[ "${1:-}" = "--release" ] && RELEASE=1

[ -x "$GRADLEW" ] || die "gradlew not found at $GRADLEW"
v="$(variant_suffix)"
step "Building app ($v) + Rust JNI core"
gradle ":app:assemble${v}"
ok "build complete"
info "APK: $ANDROID_DIR/app/build/outputs/apk/$(echo "$v" | tr '[:upper:]' '[:lower:]')/app-$(echo "$v" | tr '[:upper:]' '[:lower:]').apk"
