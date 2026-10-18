#!/usr/bin/env bash
# Build the APK (+ Rust JNI core) only. Usage: build.sh [--release] [--play]
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/lib.sh"

RELEASE=0
PLAY=0
for arg in "$@"; do
  case "$arg" in
    --release) RELEASE=1 ;;
    --play) PLAY=1 ;;
    *) die "unknown option: $arg" ;;
  esac
done

[ -x "$GRADLEW" ] || die "gradlew not found at $GRADLEW"
v="$(variant_suffix)"
step "Building app ($v) + Rust JNI core"
gradle ":app:assemble${v}"
ok "build complete"
flavor="$(variant_flavor | tr '[:upper:]' '[:lower:]')"
type="$(variant_type | tr '[:upper:]' '[:lower:]')"
info "APK: $ANDROID_DIR/app/build/outputs/apk/$flavor/$type/app-$flavor-$type.apk"
