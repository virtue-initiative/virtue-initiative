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
  --build-root <win-path>         Remote workspace root. Default: a per-worktree dir
                                  on the VM's copy-on-write Dev Drive if it has one
                                  (see below), else C:/virtue-build
  --cache-root <win-path>         Remote cache root (sccache, signing cert).
                                  Default: C:/virtue-build/cache
  --target-dir <win-path>         Remote CARGO_TARGET_DIR. Default: <cache-root>/cargo-target,
                                  or <build-root>/cargo-target on the Dev Drive
  --source-rev <git-rev>          Upload this commit's client/ instead of the working tree
  --no-cow                        Don't use the Dev Drive even if the VM has one
  --target <triple>               Rust target for packaging modes. Default: x86_64-pc-windows-msvc
  --profile <Debug|Release>       Packaging profile. Default: Debug
  --version <version>             Artifact label. Default: 0.1.6-dev
  --clean                         Run cargo clean before packaging
  --skip-sync                     Reuse remote source tree without uploading local client/
  --log-dir <dir>                 Local directory for full remote run logs.
                                  Default: client/windows/dist/remote-logs
  -h, --help                      Show this help

Copy-on-write Dev Drive: if the VM has a base build at $VIRTUE_WIN_COW_ROOT/base
(default V:/virtue/base, made by `scripts/cow-cache.sh warm-windows`), each
worktree gets its own build root under $VIRTUE_WIN_COW_ROOT/worktrees/, so
builds from different worktrees can run at once without clobbering each other.
Its cargo target dir starts as a block-cloned copy of the base's, so only the
workspace's own crates rebuild.
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
BUILD_ROOT=""
CACHE_ROOT="C:/virtue-build/cache"
TARGET_DIR=""
SEED_TARGET_DIR=""
SOURCE_REV=""
USE_COW=1
WIN_COW_ROOT="${VIRTUE_WIN_COW_ROOT:-V:/virtue}"
TARGET="x86_64-pc-windows-msvc"
PROFILE="Debug"
VERSION="0.1.6-dev"
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
    --target-dir)
      TARGET_DIR="${2:-}"
      shift 2
      ;;
    --source-rev)
      SOURCE_REV="${2:-}"
      shift 2
      ;;
    --no-cow)
      USE_COW=0
      shift
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

# Same key scripts/cow-cache.sh uses for this worktree's Linux cache: basename
# plus a hash of the absolute path, since basenames aren't unique.
WORKTREE_KEY="$(basename "$REPO_ROOT")-$(printf '%s' "$REPO_ROOT" | sha256sum | cut -c1-8)"

if [[ -z "$BUILD_ROOT" ]]; then
  BUILD_ROOT="C:/virtue-build"
  if [[ $USE_COW -eq 1 ]] && [[ "$(ssh "$BUILD_HOST" "powershell -NoProfile -Command Test-Path '$WIN_COW_ROOT/base/info'" | tr -d '\r')" == "True" ]]; then
    BUILD_ROOT="$WIN_COW_ROOT/worktrees/$WORKTREE_KEY"
    TARGET_DIR="${TARGET_DIR:-$BUILD_ROOT/cargo-target}"
    SEED_TARGET_DIR="$WIN_COW_ROOT/base/cargo-target"
    echo "Using per-worktree build root on the Dev Drive: $BUILD_ROOT"
  fi
fi
TARGET_DIR="${TARGET_DIR:-$CACHE_ROOT/cargo-target}"

# Per-worktree names, so concurrent runs from different worktrees don't
# overwrite each other's upload.
REMOTE_ARCHIVE_NAME="virtue-client-src-$WORKTREE_KEY.tgz"
REMOTE_SCRIPT_NAME="virtue-remote-build-$WORKTREE_KEY.ps1"

if [[ $SKIP_SYNC -eq 0 ]]; then
  ARCHIVE_PATH="$TMP_DIR/$REMOTE_ARCHIVE_NAME"
  if [[ -n "$SOURCE_REV" ]]; then
    git -C "$REPO_ROOT" archive --format=tar.gz -o "$ARCHIVE_PATH" "$SOURCE_REV" client
  else
    tar -C "$REPO_ROOT" \
      --exclude='client/target' \
      --exclude='client/**/target' \
      --exclude='client/windows/dist' \
      --exclude='client/android/.gradle' \
      --exclude='client/android/**/build' \
      -czf "$ARCHIVE_PATH" \
      client
  fi
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
\$targetDir = '$(ps_quote "$TARGET_DIR")'
\$seedTargetDir = '$(ps_quote "$SEED_TARGET_DIR")'
\$worktreeSource = '$(ps_quote "$REPO_ROOT")'

\$repoRoot = Join-Path \$buildRoot "src"
\$clientDir = Join-Path \$repoRoot "client"

New-Item -ItemType Directory -Force -Path \$buildRoot | Out-Null
New-Item -ItemType Directory -Force -Path \$repoRoot | Out-Null
# Lets cow-cache.sh prune-windows find build roots whose worktree is gone.
Set-Content -Path (Join-Path \$buildRoot "source") -Value \$worktreeSource

# First build in this worktree: start from a copy of the base build's target
# dir. On the ReFS Dev Drive robocopy block-clones, so this is near-instant
# and takes no extra space until files diverge.
if (\$seedTargetDir -and -not (Test-Path \$targetDir) -and (Test-Path \$seedTargetDir)) {
    Write-Host "Seeding \$targetDir from \$seedTargetDir"
    robocopy \$seedTargetDir \$targetDir /E /COPY:DAT /DCOPY:DAT /MT:16 /NFL /NDL /NJH /NJS /NP | Out-Null
    if (\$LASTEXITCODE -ge 8) {
        throw "robocopy seeding failed with exit code \$LASTEXITCODE"
    }
}

if (-not \$skipSync) {
    \$archivePath = Join-Path \$HOME "$(ps_quote "$REMOTE_ARCHIVE_NAME")"
    if (-not (Test-Path \$archivePath)) {
        throw "Missing archive at \$archivePath"
    }

    # Replace the source but keep MSBuild's bin/ and obj/ dirs, so the C#
    # projects build incrementally (tar keeps the original mtimes, so
    # unchanged files still look unchanged).
    if (Test-Path \$clientDir) {
        Get-ChildItem -LiteralPath \$clientDir -Recurse -File -Force |
            Where-Object { \$_.FullName -notmatch '\\\\(bin|obj)\\\\' } |
            Remove-Item -Force
    }
    tar -xf \$archivePath -C \$repoRoot
    if (\$LASTEXITCODE -ne 0) {
        throw "tar extraction failed with exit code \$LASTEXITCODE"
    }
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
        # build-msix.ps1 uses CARGO_TARGET_DIR over its CacheRoot default.
        \$env:CARGO_TARGET_DIR = \$targetDir
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
