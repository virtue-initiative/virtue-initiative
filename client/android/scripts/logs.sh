#!/usr/bin/env bash
# Show app logs (buffered by default). Usage: logs.sh [--follow]
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/lib.sh"

FOLLOW=0
[ "${1:-}" = "--follow" ] && FOLLOW=1

need adb
emulator_running || die "no emulator running"
pid="$(adb shell pidof -s "$VIRTUE_PACKAGE" 2>/dev/null | tr -d '\r' || true)"
if [ "$FOLLOW" = 1 ]; then
  if [ -n "$pid" ]; then
    step "Following logs for $VIRTUE_PACKAGE (pid $pid)"
    adb logcat --pid "$pid"
  else
    warn "app not running — following unfiltered logcat instead"
    adb logcat
  fi
else
  step "Dumping buffered logs for $VIRTUE_PACKAGE"
  if [ -n "$pid" ]; then
    adb logcat -d --pid "$pid"
  else
    warn "app not running — showing package-matched lines from the full buffer"
    adb logcat -d | grep -i "$VIRTUE_PACKAGE" || true
  fi
fi
