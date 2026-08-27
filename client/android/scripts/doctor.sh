#!/usr/bin/env bash
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck disable=SC1091
. "$SCRIPT_DIR/env.sh"

require_bin() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "missing: $cmd"
    return 1
  fi
  echo "ok: $cmd -> $(command -v "$cmd")"
}

echo "== Android doctor =="
echo "ANDROID_SDK_ROOT=$ANDROID_SDK_ROOT"

require_bin java
require_bin javac
require_bin cargo
require_bin rustup
require_bin sdkmanager
require_bin avdmanager
require_bin adb
require_bin emulator

echo
echo "== Versions =="
java -version 2>&1 | head -n 2
javac -version
cargo --version
rustc --version
sdkmanager --version
adb version | head -n 1
emulator -version | head -n 2

echo
echo "== Rust Android targets =="
rustup target list --installed | rg 'android' || true

echo
echo "== Installed SDK packages =="
sdkmanager --list_installed | sed -n '1,80p'

echo
echo "== Available AVDs =="
emulator -list-avds || true

echo
echo "== Emulator acceleration =="
emulator -accel-check || true

echo
echo "doctor complete"
