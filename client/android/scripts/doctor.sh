#!/usr/bin/env bash
set -euo pipefail

SDK_ROOT="${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}"

if [[ -d "$SDK_ROOT/cmdline-tools/latest/bin" ]]; then
  export PATH="$SDK_ROOT/cmdline-tools/latest/bin:$PATH"
fi
if [[ -d "$SDK_ROOT/platform-tools" ]]; then
  export PATH="$SDK_ROOT/platform-tools:$PATH"
fi
if [[ -d "$SDK_ROOT/emulator" ]]; then
  export PATH="$SDK_ROOT/emulator:$PATH"
fi

require_bin() {
  local cmd="$1"
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "missing: $cmd"
    return 1
  fi
  echo "ok: $cmd -> $(command -v "$cmd")"
}

echo "== Android doctor =="
echo "ANDROID_SDK_ROOT=$SDK_ROOT"

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
