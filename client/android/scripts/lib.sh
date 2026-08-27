# Shared helpers for the Android dev scripts (build.sh, install.sh, etc).
# Not runnable on its own — source it, don't execute it.

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ANDROID_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
GRADLEW="$ANDROID_DIR/gradlew"

# shellcheck disable=SC1091
. "$SCRIPT_DIR/env.sh"

if [ -t 1 ]; then
  C_RESET=$'\033[0m'; C_BOLD=$'\033[1m'; C_BLUE=$'\033[34m'
  C_GREEN=$'\033[32m'; C_YELLOW=$'\033[33m'; C_RED=$'\033[31m'; C_DIM=$'\033[2m'
else
  C_RESET=; C_BOLD=; C_BLUE=; C_GREEN=; C_YELLOW=; C_RED=; C_DIM=
fi
step() { printf '%s==>%s %s%s%s\n' "$C_BLUE$C_BOLD" "$C_RESET" "$C_BOLD" "$*" "$C_RESET"; }
info() { printf '%s  - %s%s\n' "$C_DIM" "$*" "$C_RESET"; }
ok()   { printf '%s✓%s %s\n' "$C_GREEN" "$C_RESET" "$*"; }
warn() { printf '%s!%s %s\n' "$C_YELLOW" "$C_RESET" "$*" >&2; }
die()  { printf '%s✗%s %s\n' "$C_RED" "$C_RESET" "$*" >&2; exit 1; }

need() { command -v "$1" >/dev/null 2>&1 || die "missing tool: $1 (check $SCRIPT_DIR/env.sh / ANDROID_SDK_ROOT)"; }

gradle() { ( cd "$ANDROID_DIR" && "$GRADLEW" "$@" ); }

# Maps a --release flag onto Gradle task name casing.
variant_suffix() { [ "${RELEASE:-0}" = 1 ] && echo "Release" || echo "Debug"; }

emulator_running() { adb devices | grep -q '^emulator-.*device$'; }

wait_for_boot() {
  step "Waiting for device to boot"
  adb wait-for-device
  until [ "$(adb shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ]; do
    sleep 1
  done
  adb shell input keyevent 82 >/dev/null 2>&1 || true  # dismiss keyguard
  ok "device booted"
}
