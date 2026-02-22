#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
PS_SCRIPT="$SCRIPT_DIR/build-installer.ps1"
WIN_PS_SCRIPT="$(wslpath -w "$PS_SCRIPT")"

VERSION="${1:-0.1.0}"

powershell.exe -NoProfile -ExecutionPolicy Bypass -File "$WIN_PS_SCRIPT" -Version "$VERSION"
