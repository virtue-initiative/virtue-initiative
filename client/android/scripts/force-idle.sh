#!/usr/bin/env bash
# Force the device into Doze (deep idle), to test background survival.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/lib.sh"

need adb
emulator_running || die "no emulator running"
step "Forcing Doze (deep idle)"
adb shell dumpsys battery unplug >/dev/null
adb shell dumpsys deviceidle enable >/dev/null
adb shell dumpsys deviceidle force-idle >/dev/null
ok "device forced into Doze"
info "restore with: adb shell dumpsys deviceidle unforce && adb shell dumpsys battery reset"
