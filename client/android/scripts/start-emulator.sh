#!/usr/bin/env bash
# Start the emulator and wait for boot. Usage: start-emulator.sh [--headless]
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/lib.sh"

HEADLESS=0
[ "${1:-}" = "--headless" ] && HEADLESS=1

need emulator
need adb

if emulator_running; then
  ok "emulator already running"
  wait_for_boot
  exit 0
fi

emulator -list-avds | grep -qx "$VIRTUE_AVD" \
  || die "AVD '$VIRTUE_AVD' not found. Create it (see client/android/README.md) or set VIRTUE_AVD."

extra=()
[ "$HEADLESS" = 1 ] && extra+=(-no-window -no-audio) && info "headless mode"

step "Starting emulator '$VIRTUE_AVD'"
# Detached so it survives this script; logs to a temp file.
log="${TMPDIR:-/tmp}/virtue-emulator.log"
nohup emulator -avd "$VIRTUE_AVD" -no-snapshot "${extra[@]}" >"$log" 2>&1 &
info "emulator pid $! (log: $log)"
wait_for_boot
