#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PS_SCRIPT="$SCRIPT_DIR/build-installer.ps1"
WIN_PS_SCRIPT="$(wslpath -w "$PS_SCRIPT")"

if [[ $# -eq 0 ]]; then
  ARGS=()
elif [[ "${1:-}" != -* ]]; then
  ARGS=(-Version "$1" "${@:2}")
else
  ARGS=("$@")
fi

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$WIN_PS_SCRIPT" "${ARGS[@]}"
