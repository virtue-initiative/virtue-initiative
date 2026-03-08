#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../../.." && pwd)"

usage() {
  cat <<'EOF'
Run Windows CI smoke checks or build an installer from Linux via SSH to a Windows VM.

Usage:
  remote-windows-build.sh --build-host <ssh-host> [options]

Options:
  --mode <smoke|installer>        Default: smoke
  --build-host <ssh-host>         SSH host/alias for the Windows VM (required)
  --build-root <win-path>         Remote workspace root. Default: C:/virtue-build
  --cache-root <win-path>         Remote cache root. Default: C:/virtue-build/cache
  --target <triple>               Rust target for installer mode. Default: x86_64-pc-windows-msvc
  --profile <Debug|Release>       Installer profile. Default: Debug
  --version <version>             Installer version. Default: 0.1.0-dev
  --clean                         Run cargo clean before installer build
  --skip-sync                     Reuse remote source tree without uploading local client/
  --log-dir <dir>                 Local directory for full remote run logs.
                                  Default: client/windows/dist/remote-logs
  --copy-installer-to-linux       Copy built installer from Windows VM back to Linux.
                                  Default is off (installer remains on Windows VM).
  --local-dist <dir>              Linux destination when --copy-installer-to-linux is used.
                                  Default: client/windows/dist/remote
  -h, --help                      Show this help
EOF
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

win_to_scp_path() {
  local path="${1//\\//}"
  if [[ "$path" =~ ^([A-Za-z]):(.*)$ ]]; then
    local drive="${BASH_REMATCH[1]}"
    local rest="${BASH_REMATCH[2]}"
    if [[ -z "$rest" ]]; then
      printf '/%s:/' "$drive"
    elif [[ "${rest:0:1}" == "/" ]]; then
      printf '/%s:%s' "$drive" "$rest"
    else
      printf '/%s:/%s' "$drive" "$rest"
    fi
    return
  fi
  printf '%s' "$path"
}

ps_quote() {
  sed "s/'/''/g" <<<"$1"
}

MODE="smoke"
BUILD_HOST=""
BUILD_ROOT="C:/virtue-build"
CACHE_ROOT="C:/virtue-build/cache"
TARGET="x86_64-pc-windows-msvc"
PROFILE="Debug"
VERSION="0.1.0-dev"
CLEAN=0
SKIP_SYNC=0
LOCAL_DIST="$REPO_ROOT/client/windows/dist/remote"
LOG_DIR="$REPO_ROOT/client/windows/dist/remote-logs"
COPY_INSTALLER_TO_LINUX=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      MODE="${2:-}"
      shift 2
      ;;
    --build-host)
      BUILD_HOST="${2:-}"
      shift 2
      ;;
    --build-root)
      BUILD_ROOT="${2:-}"
      shift 2
      ;;
    --cache-root)
      CACHE_ROOT="${2:-}"
      shift 2
      ;;
    --target)
      TARGET="${2:-}"
      shift 2
      ;;
    --profile)
      PROFILE="${2:-}"
      shift 2
      ;;
    --version)
      VERSION="${2:-}"
      shift 2
      ;;
    --clean)
      CLEAN=1
      shift
      ;;
    --skip-sync)
      SKIP_SYNC=1
      shift
      ;;
    --local-dist)
      LOCAL_DIST="${2:-}"
      shift 2
      ;;
    --log-dir)
      LOG_DIR="${2:-}"
      shift 2
      ;;
    --copy-installer-to-linux)
      COPY_INSTALLER_TO_LINUX=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "$BUILD_HOST" ]]; then
  echo "--build-host is required" >&2
  usage >&2
  exit 1
fi

if [[ "$MODE" != "smoke" && "$MODE" != "installer" ]]; then
  echo "--mode must be smoke or installer" >&2
  exit 1
fi

if [[ "$PROFILE" != "Debug" && "$PROFILE" != "Release" ]]; then
  echo "--profile must be Debug or Release" >&2
  exit 1
fi

require_cmd ssh
require_cmd scp
require_cmd tar

mkdir -p "$LOG_DIR"
LOG_STAMP="$(date +%Y%m%d-%H%M%S)"
LOG_FILE="$LOG_DIR/remote-windows-${MODE}-${LOG_STAMP}.log"
exec > >(tee -a "$LOG_FILE") 2>&1
echo "Logging to $LOG_FILE"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

REMOTE_ARCHIVE_NAME="virtue-client-src.tgz"
REMOTE_SCRIPT_NAME="virtue-remote-build.ps1"

if [[ $SKIP_SYNC -eq 0 ]]; then
  ARCHIVE_PATH="$TMP_DIR/$REMOTE_ARCHIVE_NAME"
  tar -C "$REPO_ROOT" \
    --exclude='client/target' \
    --exclude='client/**/target' \
    --exclude='client/windows/dist' \
    --exclude='client/android/.gradle' \
    --exclude='client/android/**/build' \
    -czf "$ARCHIVE_PATH" \
    client
  scp -q "$ARCHIVE_PATH" "$BUILD_HOST:$REMOTE_ARCHIVE_NAME"
fi

CLEAN_BOOL='$false'
if [[ $CLEAN -eq 1 ]]; then
  CLEAN_BOOL='$true'
fi

cat >"$TMP_DIR/$REMOTE_SCRIPT_NAME" <<EOF
\$ErrorActionPreference = "Stop"

\$mode = '$(ps_quote "$MODE")'
\$buildRoot = '$(ps_quote "$BUILD_ROOT")'
\$cacheRoot = '$(ps_quote "$CACHE_ROOT")'
\$target = '$(ps_quote "$TARGET")'
\$buildProfile = '$(ps_quote "$PROFILE")'
\$version = '$(ps_quote "$VERSION")'
\$clean = $CLEAN_BOOL
\$skipSync = $( [[ $SKIP_SYNC -eq 1 ]] && echo '$true' || echo '$false' )

\$repoRoot = Join-Path \$buildRoot "src"
\$clientDir = Join-Path \$repoRoot "client"

New-Item -ItemType Directory -Force -Path \$buildRoot | Out-Null
New-Item -ItemType Directory -Force -Path \$repoRoot | Out-Null

if (-not \$skipSync) {
    \$archivePath = Join-Path \$HOME "$(ps_quote "$REMOTE_ARCHIVE_NAME")"
    if (-not (Test-Path \$archivePath)) {
        throw "Missing archive at \$archivePath"
    }

    if (Test-Path \$clientDir) {
        Remove-Item -Recurse -Force \$clientDir
    }
    tar -xf \$archivePath -C \$repoRoot
}

if (-not (Test-Path \$clientDir)) {
    throw "Missing client workspace at \$clientDir"
}

Push-Location \$clientDir
try {
    if (\$mode -eq "smoke") {
        \$targetDir = Join-Path \$cacheRoot "cargo-target"
        \$sccacheDir = Join-Path \$cacheRoot "sccache"
        New-Item -ItemType Directory -Force -Path \$cacheRoot | Out-Null
        New-Item -ItemType Directory -Force -Path \$targetDir | Out-Null
        New-Item -ItemType Directory -Force -Path \$sccacheDir | Out-Null
        \$env:CARGO_TARGET_DIR = \$targetDir

        Remove-Item Env:RUSTC_WRAPPER -ErrorAction SilentlyContinue
        Remove-Item Env:SCCACHE_DIR -ErrorAction SilentlyContinue

        \$sccacheEnabled = \$false
        \$sccache = (Get-Command sccache -ErrorAction SilentlyContinue | Select-Object -First 1).Source
        if (\$sccache) {
            \$env:RUSTC_WRAPPER = \$sccache
            \$env:SCCACHE_DIR = \$sccacheDir
            if (-not \$env:SCCACHE_CACHE_SIZE) {
                \$env:SCCACHE_CACHE_SIZE = "10G"
            }
            & \$sccache --start-server | Out-Null
            Write-Host "Using sccache: \$sccache"
            \$sccacheEnabled = \$true
        } else {
            Write-Warning "sccache not found; proceeding without compiler cache."
        }

        if (\$sccacheEnabled) {
            \$env:CARGO_INCREMENTAL = "0"
        } else {
            \$env:CARGO_INCREMENTAL = "1"
        }

        cargo build -p virtue-client-core
        if (\$LASTEXITCODE -ne 0) {
            throw "cargo build -p virtue-client-core failed with exit code \$LASTEXITCODE"
        }

        cargo build -p virtue-windows-client
        if (\$LASTEXITCODE -ne 0) {
            throw "cargo build -p virtue-windows-client failed with exit code \$LASTEXITCODE"
        }

        cargo clippy -p virtue-client-core --all-targets -- -D warnings
        if (\$LASTEXITCODE -ne 0) {
            throw "cargo clippy -p virtue-client-core failed with exit code \$LASTEXITCODE"
        }

        cargo clippy -p virtue-windows-client --all-targets -- -D warnings
        if (\$LASTEXITCODE -ne 0) {
            throw "cargo clippy -p virtue-windows-client failed with exit code \$LASTEXITCODE"
        }
    } elseif (\$mode -eq "installer") {
        \$script = Join-Path \$clientDir "windows\\scripts\\build-installer.ps1"
        if (\$clean) {
            & \$script -Version \$version -Target \$target -Profile \$buildProfile -CacheRoot \$cacheRoot -Clean
        } else {
            & \$script -Version \$version -Target \$target -Profile \$buildProfile -CacheRoot \$cacheRoot
        }
        if (\$LASTEXITCODE -ne 0) {
            throw "build-installer.ps1 failed with exit code \$LASTEXITCODE"
        }
    } else {
        throw "Unsupported mode '\$mode'"
    }
}
finally {
    Pop-Location
}
EOF

scp -q "$TMP_DIR/$REMOTE_SCRIPT_NAME" "$BUILD_HOST:$REMOTE_SCRIPT_NAME"
ssh "$BUILD_HOST" "powershell -NoProfile -ExecutionPolicy Bypass -File $REMOTE_SCRIPT_NAME"

if [[ "$MODE" == "installer" ]]; then
  REMOTE_ARTIFACT_WIN="${BUILD_ROOT%/}/src/client/windows/dist/virtue-windows-installer-$VERSION.exe"
  echo "Installer built on VM at: $REMOTE_ARTIFACT_WIN"

  if [[ "$COPY_INSTALLER_TO_LINUX" == "1" ]]; then
    mkdir -p "$LOCAL_DIST"
    REMOTE_ARTIFACT_SCP="$(win_to_scp_path "$REMOTE_ARTIFACT_WIN")"
    LOCAL_ARTIFACT="$LOCAL_DIST/virtue-windows-installer-$VERSION.exe"
    scp -q "$BUILD_HOST:$REMOTE_ARTIFACT_SCP" "$LOCAL_ARTIFACT"
    echo "Installer copied to Linux at: $LOCAL_ARTIFACT"
  fi
fi
