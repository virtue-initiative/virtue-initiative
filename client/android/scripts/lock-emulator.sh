#!/usr/bin/env bash
# Lock the emulator's screen.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/lib.sh"

need adb
emulator_running || die "no emulator running"
step "Locking screen"
adb shell input keyevent KEYCODE_SLEEP >/dev/null  # screen off -> keyguard
ok "screen locked (use KEYCODE_WAKEUP / start-emulator.sh to wake)"
