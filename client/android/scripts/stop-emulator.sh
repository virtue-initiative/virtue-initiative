#!/usr/bin/env bash
# Kill the running emulator.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/lib.sh"

need adb
if ! emulator_running; then
  ok "no emulator running"
  exit 0
fi
step "Stopping emulator"
adb emu kill >/dev/null 2>&1 || die "failed to stop emulator"
ok "emulator stopped"
