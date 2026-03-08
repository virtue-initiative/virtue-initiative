param(
    [string]$InstallRoot = "C:\virtue-build",
    [string]$CacheRoot = "C:\virtue-build\cache",
    [string]$ApiBaseUrl = "",
    [string]$AuthorizedKey = "",
    [switch]$SkipVsBuildTools,
    [switch]$SkipNsis,
    [switch]$SkipSccache,
    [switch]$SkipDefenderExclusions
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

function Write-Step {
    param([string]$Message)
    Write-Host ""
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Assert-Admin {
    $principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        throw "Run this script from an elevated PowerShell session (Run as Administrator)."
    }
}

function Add-ToPathIfMissing {
    param([string]$PathEntry)
    if (-not (Test-Path $PathEntry)) {
        return
    }
    $current = [Environment]::GetEnvironmentVariable("Path", "User")
    $entries = @()
    if ($current) {
        $entries = $current.Split(';') | Where-Object { $_ -and $_.Trim() -ne "" }
    }
    if ($entries -contains $PathEntry) {
        return
    }
    $newPath = ($entries + $PathEntry) -join ';'
    [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
    $env:Path = "$PathEntry;$env:Path"
}

function Install-OpenSshServer {
    Write-Step "Installing and configuring OpenSSH Server"

    $capability = Get-WindowsCapability -Online -Name "OpenSSH.Server*" | Select-Object -First 1
    if (-not $capability) {
        throw "Could not query OpenSSH Server capability."
    }
    if ($capability.State -ne "Installed") {
        Add-WindowsCapability -Online -Name $capability.Name | Out-Null
    }

    Set-Service -Name sshd -StartupType Automatic
    Start-Service -Name sshd

    if (-not (Get-NetFirewallRule -Name "sshd" -ErrorAction SilentlyContinue)) {
        New-NetFirewallRule -Name "sshd" -DisplayName "OpenSSH Server (sshd)" -Enabled True -Direction Inbound -Protocol TCP -Action Allow -LocalPort 22 | Out-Null
    }

    New-Item -Path "HKLM:\SOFTWARE\OpenSSH" -Force | Out-Null
    New-ItemProperty -Path "HKLM:\SOFTWARE\OpenSSH" -Name "DefaultShell" -Value "C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe" -PropertyType String -Force | Out-Null
}

function Configure-AuthorizedKey {
    param([string]$Key)
    if ([string]::IsNullOrWhiteSpace($Key)) {
        return
    }

    Write-Step "Configuring current-user authorized_keys"
    $sshDir = Join-Path $env:USERPROFILE ".ssh"
    $authKeys = Join-Path $sshDir "authorized_keys"

    New-Item -ItemType Directory -Force -Path $sshDir | Out-Null
    if (-not (Test-Path $authKeys)) {
        New-Item -ItemType File -Force -Path $authKeys | Out-Null
    }

    $existing = Get-Content -Path $authKeys -ErrorAction SilentlyContinue
    if (-not ($existing -contains $Key)) {
        Add-Content -Path $authKeys -Value $Key
    }

    & icacls $sshDir /inheritance:r /grant:r "$env:USERNAME:(F)" /grant:r "SYSTEM:(F)" | Out-Null
    & icacls $authKeys /inheritance:r /grant:r "$env:USERNAME:(F)" /grant:r "SYSTEM:(F)" | Out-Null
}

function Install-Git {
    Write-Step "Ensuring Git is installed"
    if (Get-Command git -ErrorAction SilentlyContinue) {
        return
    }

    $gitCmdPath = "C:\Program Files\Git\cmd"
    $gitBinPath = "C:\Program Files\Git\bin"
    if (Test-Path (Join-Path $gitCmdPath "git.exe")) {
        Add-ToPathIfMissing $gitCmdPath
        Add-ToPathIfMissing $gitBinPath
        if (Get-Command git -ErrorAction SilentlyContinue) {
            return
        }
    }

    if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
        throw "winget is required to install Git automatically."
    }

    $listOutput = (& winget list --exact --id Git.Git --source winget 2>&1 | Out-String)
    if ($listOutput -match "Git\.Git" -or $listOutput -match "(?i)Git\s+Git\.Git") {
        Add-ToPathIfMissing $gitCmdPath
        Add-ToPathIfMissing $gitBinPath
        if (Get-Command git -ErrorAction SilentlyContinue) {
            return
        }
    }

    & winget install --exact --id Git.Git --source winget --accept-package-agreements --accept-source-agreements --silent
    if ($LASTEXITCODE -ne 0) {
        $listOutput = (& winget list --exact --id Git.Git --source winget 2>&1 | Out-String)
        if ($listOutput -match "Git\.Git" -or $listOutput -match "(?i)Git\s+Git\.Git") {
            Add-ToPathIfMissing $gitCmdPath
            Add-ToPathIfMissing $gitBinPath
            if (Get-Command git -ErrorAction SilentlyContinue) {
                return
            }
        }
        throw "Failed to install Git with winget."
    }

    Add-ToPathIfMissing $gitCmdPath
    Add-ToPathIfMissing $gitBinPath
}

function Install-Rust {
    Write-Step "Ensuring Rust MSVC toolchain is installed"
    $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
    $rustupPath = Join-Path $cargoBin "rustup.exe"
    $rustup = Get-Command rustup -ErrorAction SilentlyContinue
    if (-not $rustup) {
        $tmp = Join-Path $env:TEMP "rustup-init.exe"
        Invoke-WebRequest -Uri "https://win.rustup.rs/x86_64" -OutFile $tmp
        & $tmp -y --default-toolchain stable-x86_64-pc-windows-msvc
        if ($LASTEXITCODE -ne 0) {
            throw "rustup installer failed with exit code $LASTEXITCODE"
        }
    }

    Add-ToPathIfMissing $cargoBin

    if (-not (Test-Path $rustupPath)) {
        $resolved = Get-Command rustup -ErrorAction SilentlyContinue
        if ($resolved) {
            $rustupPath = $resolved.Source
        } else {
            throw "rustup not found after installation."
        }
    }

    & $rustupPath toolchain install stable-x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) {
        throw "rustup toolchain install failed with exit code $LASTEXITCODE"
    }
    & $rustupPath default stable-x86_64-pc-windows-msvc
    if ($LASTEXITCODE -ne 0) {
        throw "rustup default failed with exit code $LASTEXITCODE"
    }
    & $rustupPath component add clippy
    if ($LASTEXITCODE -ne 0) {
        throw "rustup component add clippy failed with exit code $LASTEXITCODE"
    }
}

function Install-VsBuildTools {
    if ($SkipVsBuildTools) {
        return
    }

    Write-Step "Ensuring Visual Studio Build Tools (C++) is installed"
    $bootstrapper = Join-Path $env:TEMP "vs_BuildTools.exe"
    Invoke-WebRequest -Uri "https://aka.ms/vs/17/release/vs_BuildTools.exe" -OutFile $bootstrapper

    $args = @(
        "--quiet",
        "--wait",
        "--norestart",
        "--nocache",
        "--add", "Microsoft.VisualStudio.Workload.VCTools",
        "--includeRecommended"
    )
    $proc = Start-Process -FilePath $bootstrapper -ArgumentList $args -Wait -PassThru
    if ($proc.ExitCode -ne 0 -and $proc.ExitCode -ne 3010) {
        throw "VS Build Tools install failed with exit code $($proc.ExitCode)"
    }
}

function Install-Nsis {
    if ($SkipNsis) {
        return
    }
    Write-Step "Ensuring NSIS (makensis) is installed"
    if (Get-Command makensis -ErrorAction SilentlyContinue) {
        return
    }
    if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
        throw "winget is required to install NSIS automatically."
    }

    $attempts = @("NSIS.NSIS", "Nullsoft.NSIS")
    $ok = $false
    foreach ($id in $attempts) {
        & winget install --exact --id $id --source winget --accept-package-agreements --accept-source-agreements --silent
        if ($LASTEXITCODE -eq 0) {
            $ok = $true
            break
        }
    }
    if (-not $ok) {
        throw "Failed to install NSIS with winget. Install NSIS manually and rerun."
    }
}

function Install-Sccache {
    if ($SkipSccache) {
        return
    }
    Write-Step "Ensuring sccache is installed"
    if (Get-Command sccache -ErrorAction SilentlyContinue) {
        return
    }
    $cargoPath = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (-not (Test-Path $cargoPath)) {
        $cargo = Get-Command cargo -ErrorAction SilentlyContinue
        if ($cargo) {
            $cargoPath = $cargo.Source
        }
    }
    if (-not (Test-Path $cargoPath)) {
        throw "cargo not found; cannot install sccache"
    }
    & $cargoPath install sccache --locked
    if ($LASTEXITCODE -ne 0) {
        Write-Warning "sccache install failed. You can continue without it."
    }
}

function Configure-DevPaths {
    Write-Step "Configuring build/cache directories"
    New-Item -ItemType Directory -Force -Path $InstallRoot | Out-Null
    New-Item -ItemType Directory -Force -Path $CacheRoot | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $InstallRoot "src") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $CacheRoot "cargo-target") | Out-Null
    New-Item -ItemType Directory -Force -Path (Join-Path $CacheRoot "sccache") | Out-Null
}

function Configure-DefenderExclusions {
    if ($SkipDefenderExclusions) {
        return
    }
    Write-Step "Adding Windows Defender exclusions for build cache paths"
    $paths = @(
        $CacheRoot,
        (Join-Path $InstallRoot "src\client\target"),
        (Join-Path $env:USERPROFILE ".cargo\registry"),
        (Join-Path $env:USERPROFILE ".cargo\git")
    )
    foreach ($path in $paths) {
        try {
            Add-MpPreference -ExclusionPath $path -ErrorAction Stop
        } catch {
            Write-Warning "Could not add Defender exclusion for $path ($($_.Exception.Message))"
        }
    }
}

function Configure-ServiceOverride {
    param([string]$BaseUrl)
    if ([string]::IsNullOrWhiteSpace($BaseUrl)) {
        return
    }
    Write-Step "Writing service.dev.env for Virtue runtime API override"
    $configDir = Join-Path $env:ProgramData "Virtue\config"
    New-Item -ItemType Directory -Force -Path $configDir | Out-Null
    $file = Join-Path $configDir "service.dev.env"
    @(
        "VIRTUE_BASE_API_URL=$BaseUrl"
    ) | Set-Content -Path $file -Encoding UTF8
}

function Enable-LongPaths {
    Write-Step "Enabling Win32 long paths"
    New-ItemProperty -Path "HKLM:\SYSTEM\CurrentControlSet\Control\FileSystem" -Name "LongPathsEnabled" -PropertyType DWord -Value 1 -Force | Out-Null
}

Assert-Admin

Write-Host "Bootstrap starting for Windows build/test VM..."

Install-OpenSshServer
Configure-AuthorizedKey -Key $AuthorizedKey
Install-Git
Install-Rust
Install-VsBuildTools
Install-Nsis
Install-Sccache
Configure-DevPaths
Configure-DefenderExclusions
Configure-ServiceOverride -BaseUrl $ApiBaseUrl
Enable-LongPaths

Write-Step "Bootstrap complete"
Write-Host "Recommended next checks:"
Write-Host "  1) powershell -Command `"Get-Service sshd`""
Write-Host "  2) powershell -Command `"cargo --version`""
Write-Host "  3) powershell -Command `"makensis /VERSION`""
Write-Host "  4) Reboot once if VS Build Tools installer requested it"
