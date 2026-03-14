param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$Version = "",
    [ValidateSet("Debug", "Release")]
    [string]$Profile = "Debug",
    [switch]$SkipBuild,
    [switch]$Clean,
    [string]$CacheRoot = ""
)

$ErrorActionPreference = "Stop"

$VersionHelper = Join-Path $PSScriptRoot "Get-VersionInfo.ps1"
. $VersionHelper

$VersionInfo = Get-VirtueVersionInfo
if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = $VersionInfo.BuildLabel
}

if ([string]::IsNullOrWhiteSpace($CacheRoot)) {
    $CacheRoot = Join-Path $env:LOCALAPPDATA "VirtueBuildCache"
}

$ProfileLower = $Profile.ToLowerInvariant()
$ProjectRoot = Split-Path -Parent $PSScriptRoot
$WorkspaceRoot = Split-Path -Parent $ProjectRoot
$WorkspaceTargetDir = Join-Path $WorkspaceRoot "target"
$NsisScript = Join-Path $ProjectRoot "packaging\nsis\installer.nsi"
$DistDir = Join-Path $ProjectRoot "dist"
$OutFile = Join-Path $DistDir "virtue-windows-installer-$Version.exe"

if (-not [string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
    $BuildTargetDir = $env:CARGO_TARGET_DIR
} elseif ($env:GITHUB_ACTIONS -eq "true") {
    # Reuse the target dir restored by rust-cache in CI.
    $BuildTargetDir = $WorkspaceTargetDir
} else {
    $BuildTargetDir = Join-Path $CacheRoot "cargo-target"
}

$SccacheDir = Join-Path $CacheRoot "sccache"
$LocalOutFile = Join-Path $BuildTargetDir "virtue-windows-installer-$Version.exe"

Push-Location $ProjectRoot
try {
    $cargo = (Get-Command cargo -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if (-not $cargo) {
        $candidate = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
        if (Test-Path $candidate) {
            $cargo = $candidate
        }
    }
    if (-not $cargo) {
        throw "cargo not found. Install Rust toolchain for Windows."
    }

    New-Item -ItemType Directory -Force -Path $CacheRoot | Out-Null
    New-Item -ItemType Directory -Force -Path $BuildTargetDir | Out-Null
    New-Item -ItemType Directory -Force -Path $SccacheDir | Out-Null

    $env:CARGO_TARGET_DIR = $BuildTargetDir
    $sccacheEnabled = $false

    Remove-Item Env:RUSTC_WRAPPER -ErrorAction SilentlyContinue
    Remove-Item Env:SCCACHE_DIR -ErrorAction SilentlyContinue

    $sccache = (Get-Command sccache -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if ($sccache) {
        $env:RUSTC_WRAPPER = $sccache
        $env:SCCACHE_DIR = $SccacheDir
        if (-not $env:SCCACHE_CACHE_SIZE) {
            $env:SCCACHE_CACHE_SIZE = "10G"
        }
        & $sccache --start-server | Out-Null
        Write-Host "Using sccache: $sccache"
        $sccacheEnabled = $true
    } else {
        Write-Warning "sccache not found; proceeding without compiler cache."
    }

    if ($sccacheEnabled) {
        # sccache is incompatible with incremental mode in this setup.
        $env:CARGO_INCREMENTAL = "0"
    } else {
        $env:CARGO_INCREMENTAL = if ($Profile -eq "Debug") { "1" } else { "0" }
    }

    if (-not $SkipBuild) {
        if ($Clean) {
            & $cargo clean --target $Target
            if ($LASTEXITCODE -ne 0) {
                throw "cargo clean failed with exit code $LASTEXITCODE"
            }
        }

        $buildArgs = @(
            "build",
            "--target", $Target,
            "--bin", "virtue-service",
            "--bin", "virtue-tray",
            "--bin", "virtue-auth-ui"
        )
        if ($Profile -eq "Release") {
            $buildArgs += "--release"
        }

        & $cargo @buildArgs
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed with exit code $LASTEXITCODE"
        }
    }

    New-Item -ItemType Directory -Force -Path $DistDir | Out-Null

    $makensis = (Get-Command makensis -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if (-not $makensis) {
        $candidates = @(
            "$env:ProgramFiles\NSIS\makensis.exe",
            "${env:ProgramFiles(x86)}\NSIS\makensis.exe"
        )
        foreach ($candidate in $candidates) {
            if (Test-Path $candidate) {
                $makensis = $candidate
                break
            }
        }
    }

    if (-not $makensis) {
        throw "makensis not found. Install NSIS or add makensis.exe to PATH."
    }

    if (Test-Path $LocalOutFile) {
        Remove-Item -Force $LocalOutFile
    }

    & $makensis "/DPRODUCT_VERSION=$Version" "/DOUTFILE=$LocalOutFile" "/DBUILD_TARGET_DIR=$BuildTargetDir" "/DBUILD_TARGET=$Target" "/DBUILD_PROFILE=$ProfileLower" $NsisScript
    if ($LASTEXITCODE -ne 0) {
        throw "makensis failed with exit code $LASTEXITCODE"
    }

    if (-not (Test-Path $LocalOutFile)) {
        throw "Installer build did not produce expected file: $LocalOutFile"
    }

    if (Test-Path $OutFile) {
        Remove-Item -Force $OutFile
    }

    Copy-Item -Force $LocalOutFile $OutFile

    if (-not (Test-Path $OutFile)) {
        throw "Installer build did not produce expected file: $OutFile"
    }

    Write-Host "Built installer: $OutFile"
}
finally {
    Pop-Location
}
