param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$Version = "0.1.0",
    [switch]$SkipBuild
)

$ErrorActionPreference = "Stop"

$ProjectRoot = Split-Path -Parent $PSScriptRoot
$NsisScript = Join-Path $ProjectRoot "packaging\nsis\installer.nsi"
$DistDir = Join-Path $ProjectRoot "dist"
$OutFile = Join-Path $DistDir "virtue-windows-installer-$Version.exe"
$BuildTargetDir = Join-Path $env:TEMP "virtue-target"
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

    $env:CARGO_INCREMENTAL = "0"
    $env:CARGO_TARGET_DIR = $BuildTargetDir

    if (-not $SkipBuild) {
        & $cargo build --release --target $Target --bin virtue-service --bin virtue-tray
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

    & $makensis "/DPRODUCT_VERSION=$Version" "/DOUTFILE=$LocalOutFile" "/DBUILD_TARGET_DIR=$BuildTargetDir" $NsisScript
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
