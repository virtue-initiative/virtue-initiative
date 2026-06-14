#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd -- "$SCRIPT_DIR/../../.." && pwd)"

usage() {
  cat <<'EOF'
Run Windows CI smoke checks or build a Windows MSIX package from Linux via SSH to a Windows VM.

Usage:
  remote-windows-build.sh --build-host <ssh-host> [options]

Options:
  --mode <smoke|msix>             Default: smoke.
  --build-host <ssh-host>         SSH host/alias for the Windows VM (required)
  --build-root <win-path>         Remote workspace root. Default: C:/virtue-build
  --cache-root <win-path>         Remote cache root. Default: C:/virtue-build/cache
  --target <triple>               Rust target for packaging modes. Default: x86_64-pc-windows-msvc
  --profile <Debug|Release>       Packaging profile. Default: Debug
  --version <version>             Artifact label. Default: 0.0.7-dev
  --clean                         Run cargo clean before packaging
  --skip-sync                     Reuse remote source tree without uploading local client/
  --log-dir <dir>                 Local directory for full remote run logs.
                                  Default: client/windows/dist/remote-logs
  -h, --help                      Show this help
EOF
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
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
VERSION="0.0.7-dev"
CLEAN=0
SKIP_SYNC=0
LOG_DIR="$REPO_ROOT/client/windows/dist/remote-logs"
SIGNING_CERT_PATH=""
SIGNING_CERT_PASS=""

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
    --signing-cert-path)
      SIGNING_CERT_PATH="${2:-}"
      shift 2
      ;;
    --signing-cert-pass)
      SIGNING_CERT_PASS="${2:-}"
      shift 2
      ;;
    --log-dir)
      LOG_DIR="${2:-}"
      shift 2
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

if [[ "$MODE" != "smoke" && "$MODE" != "msix" ]]; then
  echo "--mode must be smoke or msix" >&2
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
\$signingCertPath = '$(ps_quote "$SIGNING_CERT_PATH")'
\$signingCertPass = '$(ps_quote "$SIGNING_CERT_PASS")'

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
        \$windowsAppDir = Join-Path \$clientDir "windows"
        \$windowsAppProject = Join-Path \$windowsAppDir "Virtue.WindowsApp\\Virtue.WindowsApp.csproj"
        \$windowsCoreProject = Join-Path \$windowsAppDir "Virtue.WindowsApp.Core\\Virtue.WindowsApp.Core.csproj"
        \$windowsTestsProject = Join-Path \$windowsAppDir "Virtue.WindowsApp.Tests\\Virtue.WindowsApp.Tests.csproj"
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

        cargo build -p virtue-core
        if (\$LASTEXITCODE -ne 0) {
            throw "cargo build -p virtue-core failed with exit code \$LASTEXITCODE"
        }

        cargo build -p virtue-windows
        if (\$LASTEXITCODE -ne 0) {
            throw "cargo build -p virtue-windows failed with exit code \$LASTEXITCODE"
        }

        cargo clippy -p virtue-core --all-targets -- -D warnings
        if (\$LASTEXITCODE -ne 0) {
            throw "cargo clippy -p virtue-core failed with exit code \$LASTEXITCODE"
        }

        cargo clippy -p virtue-windows --all-targets -- -D warnings
        if (\$LASTEXITCODE -ne 0) {
            throw "cargo clippy -p virtue-windows failed with exit code \$LASTEXITCODE"
        }

        dotnet restore \$windowsAppProject
        if (\$LASTEXITCODE -ne 0) {
            throw "dotnet restore for Virtue.WindowsApp failed with exit code \$LASTEXITCODE"
        }

        dotnet build \$windowsCoreProject -c \$buildProfile
        if (\$LASTEXITCODE -ne 0) {
            throw "dotnet build for Virtue.WindowsApp.Core failed with exit code \$LASTEXITCODE"
        }

        dotnet test \$windowsTestsProject -c \$buildProfile
        if (\$LASTEXITCODE -ne 0) {
            throw "dotnet test for Virtue.WindowsApp.Tests failed with exit code \$LASTEXITCODE"
        }

        dotnet build \$windowsAppProject -c \$buildProfile -p:Platform=x64 -p:AppxPackageSigningEnabled=false -p:GenerateAppxPackageOnBuild=false
        if (\$LASTEXITCODE -ne 0) {
            throw "dotnet build for Virtue.WindowsApp failed with exit code \$LASTEXITCODE"
        }
    } elseif (\$mode -eq "msix") {
        \$script = Join-Path \$clientDir "windows\\scripts\\build-msix.ps1"
        \$msixArgs = @{
            Version = \$version
            Target = \$target
            Profile = \$buildProfile
            CacheRoot = \$cacheRoot
        }
        if (\$clean) { \$msixArgs['Clean'] = \$true }
        if (-not [string]::IsNullOrWhiteSpace(\$signingCertPath)) {
            \$msixArgs['SigningCertificatePath'] = \$signingCertPath
        }
        if (-not [string]::IsNullOrWhiteSpace(\$signingCertPass)) {
            \$msixArgs['SigningCertificatePassword'] = \$signingCertPass
        }
        & \$script @msixArgs
        if (\$LASTEXITCODE -ne 0) {
            throw "build-msix.ps1 failed with exit code \$LASTEXITCODE"
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

if [[ "$MODE" == "msix" ]]; then
  REMOTE_ARTIFACT_WIN="${BUILD_ROOT%/}/src/client/windows/dist/virtue-windows-$VERSION.msix"
  REMOTE_SETUP_ZIP_WIN="${BUILD_ROOT%/}/src/client/windows/dist/virtue-windows-$VERSION-setup.zip"
  echo "MSIX package built on VM at: $REMOTE_ARTIFACT_WIN"
  echo "Setup bundle built on VM at: $REMOTE_SETUP_ZIP_WIN"
fi
