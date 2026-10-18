#!/usr/bin/env bash
# Build, install, and launch the app on the emulator. Usage: install.sh [--release] [--play]
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
need adb
emulator_running || warn "no emulator detected — 'start-emulator.sh' first if install fails"
v="$(variant_suffix)"
step "Building + installing app ($v)"
gradle ":app:install${v}"
ok "installed $VIRTUE_PACKAGE"
step "Launching app"
adb shell am start -n "$VIRTUE_ACTIVITY" >/dev/null
ok "launched $VIRTUE_ACTIVITY"
info "logs: ./logs.sh --follow"
