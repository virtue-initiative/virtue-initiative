param(
    [string]$Target = "x86_64-pc-windows-msvc",
    [string]$Version = "",
    [string]$PackageVersion = "",
    [string]$PackagePublisher = "",
    [ValidateSet("Debug", "Release")]
    [string]$Profile = "Debug",
    [switch]$SkipBuild,
    [switch]$SkipSigning,
    [switch]$Clean,
    [string]$CacheRoot = "",
    [string]$SigningCertificatePath = "",
    [string]$SigningCertificatePassword = "",
    [string]$SigningTimestampUrl = ""
)

$ErrorActionPreference = "Stop"

function New-SetupLauncher {
    param([string]$OutputPath)

    $content = @'
@echo off
setlocal

powershell -NoProfile -ExecutionPolicy Bypass -Command "$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent()); if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { exit 42 } Start-Process -FilePath '%~f0' -Verb RunAs; exit 0"
if %ERRORLEVEL% EQU 42 goto elevated
exit /b %ERRORLEVEL%

:elevated
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0Install.ps1"
set EXITCODE=%ERRORLEVEL%
if not "%EXITCODE%"=="0" (
    echo.
    echo Virtue setup failed with exit code %EXITCODE%.
    pause
)
exit /b %EXITCODE%
'@

    Set-Content -Path $OutputPath -Value $content -Encoding ASCII
}

function New-SideloadInstallScript {
    param(
        [string]$OutputPath,
        [string]$PackageFileName,
        [string]$CertificateFileName
    )

    $content = @"
`$ErrorActionPreference = "Stop"

`$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not `$principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw "Run this script from an elevated PowerShell window."
}

function Remove-LegacyServiceIfPresent {
    param([string]`$ServiceName)

    `$service = Get-CimInstance -ClassName Win32_Service -Filter "Name='`$ServiceName'" -ErrorAction SilentlyContinue
    if (-not `$service) {
        return
    }

    Write-Host "Removing legacy service '`$ServiceName' (`$service.PathName)."
    sc.exe stop `$ServiceName | Out-Null
    Start-Sleep -Milliseconds 500
    sc.exe delete `$ServiceName | Out-Null
}

Remove-LegacyServiceIfPresent -ServiceName "BePureCaptureService"
Remove-LegacyServiceIfPresent -ServiceName "VirtueCaptureService"
Remove-LegacyServiceIfPresent -ServiceName "VirtueLifecycleService"

`$packagePath = Join-Path `$PSScriptRoot "$PackageFileName"
if (-not (Test-Path `$packagePath)) {
    throw "MSIX package not found at `$packagePath"
}

if (-not [string]::IsNullOrWhiteSpace("$CertificateFileName")) {
    `$certificatePath = Join-Path `$PSScriptRoot "$CertificateFileName"
    if (-not (Test-Path `$certificatePath)) {
        throw "Package signing certificate not found at `$certificatePath"
    }

    `$certificate = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2(`$certificatePath)
    `$thumbprint = `$certificate.Thumbprint
    `$existingCertificate = Get-ChildItem -Path Cert:\LocalMachine\TrustedPeople | Where-Object Thumbprint -eq `$thumbprint | Select-Object -First 1
    if (-not `$existingCertificate) {
        Import-Certificate -FilePath `$certificatePath -CertStoreLocation Cert:\LocalMachine\TrustedPeople | Out-Null
        Write-Host "Imported package signing certificate into LocalMachine\\TrustedPeople."
    } else {
        Write-Host "Package signing certificate already trusted."
    }
}

`$generatedInstallScript = Join-Path `$PSScriptRoot "Install-AppDevPackage.ps1"
if (Test-Path `$generatedInstallScript) {
    & `$generatedInstallScript
    if (`$LASTEXITCODE -ne 0) {
        throw "Generated MSIX install script failed with exit code `$LASTEXITCODE"
    }
    Write-Host "Installed MSIX package using generated app-dev installer."
} else {
    Add-AppxPackage -Path `$packagePath
    Write-Host "Installed MSIX package from `$packagePath"
}
"@

    Set-Content -Path $OutputPath -Value $content -Encoding ASCII
}

function Get-AppPackagePublisher {
    param([string]$ManifestPath)

    $manifestText = Get-Content -Path $ManifestPath -Raw
    $publisherMatch = [regex]::Match($manifestText, 'Publisher="([^"]+)"')
    if (-not $publisherMatch.Success) {
        throw "Missing package publisher in manifest: $ManifestPath"
    }

    $publisher = $publisherMatch.Groups[1].Value
    if ([string]::IsNullOrWhiteSpace($publisher)) {
        throw "Missing package publisher in manifest: $ManifestPath"
    }

    return $publisher
}

function Set-AppPackageIdentityInManifest {
    param(
        [string]$ManifestPath,
        [string]$PackageVersion,
        [string]$Publisher
    )

    $manifestText = Get-Content -Path $ManifestPath -Raw
    $document = New-Object System.Xml.XmlDocument
    $document.PreserveWhitespace = $true
    $document.LoadXml($manifestText)

    $namespaceUri = $document.DocumentElement.NamespaceURI
    $namespaceManager = New-Object System.Xml.XmlNamespaceManager($document.NameTable)
    $namespaceManager.AddNamespace("appx", $namespaceUri)

    $identityNode = $document.SelectSingleNode("/appx:Package/appx:Identity", $namespaceManager)
    if ($null -eq $identityNode) {
        throw "Failed to find Identity element in manifest: $ManifestPath"
    }

    $versionAttribute = $identityNode.Attributes["Version"]
    if ($null -eq $versionAttribute) {
        throw "Failed to find Identity Version attribute in manifest: $ManifestPath"
    }

    $publisherAttribute = $identityNode.Attributes["Publisher"]
    if ($null -eq $publisherAttribute) {
        throw "Failed to find Identity Publisher attribute in manifest: $ManifestPath"
    }

    $versionAttribute.Value = $PackageVersion
    $publisherAttribute.Value = $Publisher
    $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText($ManifestPath, $document.OuterXml, $utf8NoBom)
    return $manifestText
}

function New-CodeSigningPassword {
    $passwordBytes = New-Object byte[] 18
    $rng = [System.Security.Cryptography.RandomNumberGenerator]::Create()
    try {
        $rng.GetBytes($passwordBytes)
    }
    finally {
        $rng.Dispose()
    }
    return [Convert]::ToBase64String($passwordBytes)
}

function Ensure-PackageSigningCertificate {
    param(
        [string]$Publisher,
        [string]$CertificateRoot
    )

    $friendlyName = "Virtue MSIX Dev Signing"
    $storePath = "Cert:\LocalMachine\My"
    $existing = Get-ChildItem -Path $storePath |
        Where-Object {
            $_.Subject -eq $Publisher -and
            $_.HasPrivateKey -and
            $_.NotAfter -gt (Get-Date)
        } |
        Sort-Object NotAfter -Descending |
        Select-Object -First 1

    if (-not $existing) {
        $existing = New-SelfSignedCertificate `
            -Type Custom `
            -KeyUsage DigitalSignature `
            -KeyAlgorithm RSA `
            -KeyLength 2048 `
            -HashAlgorithm sha256 `
            -KeyExportPolicy Exportable `
            -CertStoreLocation $storePath `
            -TextExtension @("2.5.29.37={text}1.3.6.1.5.5.7.3.3", "2.5.29.19={text}") `
            -Subject $Publisher `
            -FriendlyName $friendlyName `
            -NotAfter (Get-Date).AddYears(2)
    }

    New-Item -ItemType Directory -Force -Path $CertificateRoot | Out-Null
    $pfxPath = Join-Path $CertificateRoot "virtue-windows-dev-signing.pfx"
    $cerPath = Join-Path $CertificateRoot "virtue-windows-dev-signing.cer"
    $plainPassword = New-CodeSigningPassword
    $securePassword = ConvertTo-SecureString -String $plainPassword -Force -AsPlainText

    Export-PfxCertificate -Cert $existing.PSPath -FilePath $pfxPath -Password $securePassword | Out-Null
    Export-Certificate -Cert $existing.PSPath -FilePath $cerPath -Type CERT | Out-Null

    [pscustomobject]@{
        Certificate = $existing
        Thumbprint  = $existing.Thumbprint
        PfxPath     = $pfxPath
        CerPath     = $cerPath
        Password    = $plainPassword
    }
}

function Import-PfxToMachineStore {
    param(
        [string]$CertificatePath,
        [string]$CertificatePassword
    )

    if ([string]::IsNullOrWhiteSpace($CertificatePath)) {
        throw "Signing certificate path is required."
    }
    if (-not (Test-Path $CertificatePath)) {
        throw "Signing certificate not found at $CertificatePath"
    }

    $flags = [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::MachineKeySet `
           -bor [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::PersistKeySet `
           -bor [System.Security.Cryptography.X509Certificates.X509KeyStorageFlags]::Exportable
    $cert = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2(
        $CertificatePath, $CertificatePassword, $flags)

    if (-not $cert.HasPrivateKey) {
        throw "Signing certificate at $CertificatePath does not include a private key."
    }
    if ($cert.NotAfter -le (Get-Date)) {
        throw "Signing certificate at $CertificatePath is expired."
    }

    $store = New-Object System.Security.Cryptography.X509Certificates.X509Store("My", "LocalMachine")
    $store.Open("ReadWrite")
    $store.Add($cert)
    $store.Close()

    return $cert
}

function Resolve-SigningConfiguration {
    param(
        [string]$ManifestPublisher,
        [string]$CertificateRoot,
        [string]$SigningCertificatePath,
        [string]$SigningCertificatePassword,
        [string]$SigningTimestampUrl
    )

    if (-not [string]::IsNullOrWhiteSpace($SigningCertificatePath)) {
        $certificate = Import-PfxToMachineStore `
            -CertificatePath $SigningCertificatePath `
            -CertificatePassword $SigningCertificatePassword

        return [pscustomobject]@{
            Mode                       = "Trusted"
            Publisher                  = $certificate.Subject
            Certificate                = $certificate
            Thumbprint                 = $certificate.Thumbprint
            PfxPath                    = $SigningCertificatePath
            Password                   = $SigningCertificatePassword
            CerPath                    = $null
            TimestampUrl               = $SigningTimestampUrl
            RequiresCertificateBootstrap = $false
            ImportedToMachineStore     = $true
        }
    }

    $devCertificate = Ensure-PackageSigningCertificate -Publisher $ManifestPublisher -CertificateRoot $CertificateRoot
    return [pscustomobject]@{
        Mode                       = "Dev"
        Publisher                  = $ManifestPublisher
        Certificate                = $devCertificate.Certificate
        Thumbprint                 = $devCertificate.Thumbprint
        PfxPath                    = $devCertificate.PfxPath
        Password                   = $devCertificate.Password
        CerPath                    = $devCertificate.CerPath
        TimestampUrl               = $null
        RequiresCertificateBootstrap = $true
        ImportedToMachineStore     = $false
    }
}

function Ensure-TrustedCertificate {
    param([string]$CertificatePath)

    try {
        $certificate = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($CertificatePath)
        $existing = Get-ChildItem -Path Cert:\LocalMachine\TrustedPeople -ErrorAction Stop |
            Where-Object Thumbprint -eq $certificate.Thumbprint |
            Select-Object -First 1

        if (-not $existing) {
            Import-Certificate -FilePath $CertificatePath -CertStoreLocation Cert:\LocalMachine\TrustedPeople -ErrorAction Stop | Out-Null
            Write-Host "Trusted package signing certificate in LocalMachine\\TrustedPeople."
        } else {
            Write-Host "Package signing certificate already trusted in LocalMachine\\TrustedPeople."
        }
    }
    catch {
        Write-Warning "Unable to trust package signing certificate automatically: $($_.Exception.Message)"
    }
}

function Resolve-Cargo {
    $cargo = (Get-Command cargo -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if ($cargo) {
        return $cargo
    }

    $candidate = Join-Path $env:USERPROFILE ".cargo\bin\cargo.exe"
    if (Test-Path $candidate) {
        return $candidate
    }

    throw "cargo not found. Install the Rust Windows MSVC toolchain."
}

function Resolve-DotNet {
    $dotnet = (Get-Command dotnet -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if (-not $dotnet) {
        throw "dotnet not found. Install the .NET SDK."
    }

    return $dotnet
}

function Resolve-MSBuild {
    $msbuild = (Get-Command msbuild -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if ($msbuild) {
        return $msbuild
    }

    $candidates = @(
        "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\MSBuild.exe",
        "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\MSBuild\Current\Bin\amd64\MSBuild.exe"
    )

    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) {
            return $candidate
        }
    }

    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $resolved = & $vswhere -latest -requires Microsoft.Component.MSBuild -find "MSBuild\**\Bin\MSBuild.exe" | Select-Object -First 1
        if ($resolved) {
            return $resolved.Trim()
        }
    }

    throw "MSBuild not found. Install Visual Studio Build Tools with managed desktop support."
}

function Resolve-SignTool {
    $signTool = (Get-Command signtool -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if ($signTool) {
        return $signTool
    }

    $kitRoot = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    if (Test-Path $kitRoot) {
        $candidate = Get-ChildItem -Path $kitRoot -Filter signtool.exe -Recurse -ErrorAction SilentlyContinue |
            Sort-Object FullName -Descending |
            Select-Object -First 1
        if ($candidate) {
            return $candidate.FullName
        }
    }

    throw "signtool not found. Install the Windows SDK signing tools."
}

function Convert-ToMsixVersion {
    param([string]$Value)

    $parts = $Value.Split('.')
    while ($parts.Count -lt 4) {
        $parts += "0"
    }

    return ($parts[0..3] -join '.')
}

function Get-AppPackageName {
    param([string]$ManifestPath)

    $manifestText = Get-Content -Path $ManifestPath -Raw
    $nameMatch = [regex]::Match($manifestText, 'Identity\s+Name="([^"]+)"')
    if (-not $nameMatch.Success) {
        throw "Missing package identity name in manifest: $ManifestPath"
    }

    $name = $nameMatch.Groups[1].Value
    if ([string]::IsNullOrWhiteSpace($name)) {
        throw "Missing package identity name in manifest: $ManifestPath"
    }

    return $name
}

function Get-InstalledPackageRevision {
    param([string]$PackageName)

    try {
        $installed = Get-AppxPackage -Name $PackageName -ErrorAction Stop |
            Sort-Object Version -Descending |
            Select-Object -First 1
    }
    catch {
        return 0
    }

    if (-not $installed) {
        return 0
    }

    $versionText = $installed.Version.ToString()
    $versionParts = $versionText.Split('.')
    if ($versionParts.Count -lt 4) {
        return 0
    }

    return [int]$versionParts[3]
}

function Get-CommitsSinceVersionBump {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepoRoot
    )

    $git = (Get-Command git -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if (-not $git) {
        throw "git not found; cannot compute commits since the last version bump."
    }

    $bumpCommit = & $git -C $RepoRoot log -1 --format=%H -- client/version.properties 2>$null
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($bumpCommit)) {
        throw "Unable to locate a commit that touched client/version.properties. Ensure the checkout has full history (fetch-depth: 0)."
    }
    $bumpCommit = $bumpCommit.Trim()

    $countText = & $git -C $RepoRoot rev-list --count "$bumpCommit..HEAD" 2>$null
    if ($LASTEXITCODE -ne 0) {
        throw "git rev-list failed while counting commits since the last version bump."
    }

    return [int]$countText.Trim()
}

function New-DevMsixRevision {
    param(
        [string]$PackageName,
        [string]$RepoRoot
    )

    if ($env:GITHUB_ACTIONS -eq "true") {
        $nextRevision = Get-CommitsSinceVersionBump -RepoRoot $RepoRoot
    } else {
        $nextRevision = (Get-InstalledPackageRevision -PackageName $PackageName) + 1
    }

    if ($nextRevision -gt 65535) {
        throw "Computed MSIX revision $nextRevision exceeds the Appx limit of 65535."
    }

    return $nextRevision
}

$VersionHelper = Join-Path $PSScriptRoot "Get-VersionInfo.ps1"
. $VersionHelper

$VersionInfo = Get-VirtueVersionInfo
$ProfileLower = $Profile.ToLowerInvariant()
$WindowsRoot = Split-Path -Parent $PSScriptRoot
$ClientRoot = Split-Path -Parent $WindowsRoot
$RepoRoot = Split-Path -Parent $ClientRoot
$WindowsAppRoot = Join-Path $RepoRoot "client\windows\Virtue.WindowsApp"
$WindowsAppProject = Join-Path $WindowsAppRoot "Virtue.WindowsApp.csproj"
$WindowsAppManifest = Join-Path $WindowsAppRoot "Package.appxmanifest"
$PackageName = Get-AppPackageName -ManifestPath $WindowsAppManifest
$WindowsTestsRoot = Join-Path $RepoRoot "client\windows\Virtue.WindowsApp.Tests"
$WindowsTestsProject = Join-Path $WindowsTestsRoot "Virtue.WindowsApp.Tests.csproj"
$DistDir = Join-Path $WindowsRoot "dist"
$OutFile = Join-Path $DistDir "virtue-windows-$Version.msix"
$InstallScriptFile = Join-Path $DistDir "install-virtue-msix-$Version.ps1"
$CertificateFile = Join-Path $DistDir "virtue-windows-$Version.cer"
$SetupBundleDir = Join-Path $DistDir "virtue-windows-$Version-setup"
$SetupBundleZip = Join-Path $DistDir "virtue-windows-$Version-setup.zip"
$WorkspaceTargetDir = Join-Path $ClientRoot "target"

if ([string]::IsNullOrWhiteSpace($SigningCertificatePath) -and -not [string]::IsNullOrWhiteSpace($env:VIRTUE_WINDOWS_SIGNING_CERT_PATH)) {
    $SigningCertificatePath = $env:VIRTUE_WINDOWS_SIGNING_CERT_PATH
}
if ([string]::IsNullOrWhiteSpace($SigningCertificatePassword) -and -not [string]::IsNullOrWhiteSpace($env:VIRTUE_WINDOWS_SIGNING_CERT_PASSWORD)) {
    $SigningCertificatePassword = $env:VIRTUE_WINDOWS_SIGNING_CERT_PASSWORD
}
if ([string]::IsNullOrWhiteSpace($SigningTimestampUrl) -and -not [string]::IsNullOrWhiteSpace($env:VIRTUE_WINDOWS_SIGNING_TIMESTAMP_URL)) {
    $SigningTimestampUrl = $env:VIRTUE_WINDOWS_SIGNING_TIMESTAMP_URL
}
if ([string]::IsNullOrWhiteSpace($PackagePublisher) -and -not [string]::IsNullOrWhiteSpace($env:VIRTUE_WINDOWS_PACKAGE_PUBLISHER)) {
    $PackagePublisher = $env:VIRTUE_WINDOWS_PACKAGE_PUBLISHER
}

if ([string]::IsNullOrWhiteSpace($Version)) {
    $Version = $VersionInfo.BuildLabel
}
if ([string]::IsNullOrWhiteSpace($PackageVersion)) {
    $PackageVersion = Convert-ToMsixVersion -Value $VersionInfo.BaseVersion
    if ($VersionInfo.ReleaseChannel -eq "dev") {
        $versionParts = $PackageVersion.Split('.')
        $versionParts[3] = [string](New-DevMsixRevision -PackageName $PackageName -RepoRoot $RepoRoot)
        $PackageVersion = $versionParts -join '.'
    }
}
if ([string]::IsNullOrWhiteSpace($CacheRoot)) {
    $CacheRoot = Join-Path $env:LOCALAPPDATA "VirtueBuildCache"
}

if (-not [string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
    $BuildTargetDir = $env:CARGO_TARGET_DIR
} elseif ($env:GITHUB_ACTIONS -eq "true") {
    $BuildTargetDir = $WorkspaceTargetDir
} else {
    $BuildTargetDir = Join-Path $CacheRoot "cargo-target"
}

$SccacheDir = Join-Path $CacheRoot "sccache"
$RustOutputDir = Join-Path $BuildTargetDir "$Target\$ProfileLower"
$PackageOutputDir = Join-Path $WindowsAppRoot "AppPackages"
$CertificateCacheDir = Join-Path $CacheRoot "signing"

Push-Location $ClientRoot
$originalManifestText = $null
$signingCertificate = $null
try {
    $cargo = Resolve-Cargo
    $dotnet = Resolve-DotNet
    $msbuild = Resolve-MSBuild
    $signTool = $null
    if (-not $SkipSigning) {
        $signTool = Resolve-SignTool
    }

    New-Item -ItemType Directory -Force -Path $CacheRoot | Out-Null
    New-Item -ItemType Directory -Force -Path $BuildTargetDir | Out-Null
    New-Item -ItemType Directory -Force -Path $SccacheDir | Out-Null
    New-Item -ItemType Directory -Force -Path $DistDir | Out-Null
    New-Item -ItemType Directory -Force -Path $PackageOutputDir | Out-Null

    $env:CARGO_TARGET_DIR = $BuildTargetDir
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
        $env:CARGO_INCREMENTAL = "0"
    } else {
        Write-Warning "sccache not found; proceeding without compiler cache."
        $env:CARGO_INCREMENTAL = if ($Profile -eq "Debug") { "1" } else { "0" }
    }

    & $dotnet restore $WindowsAppProject
    if ($LASTEXITCODE -ne 0) {
        throw "dotnet restore for WinUI app failed with exit code $LASTEXITCODE"
    }
    & $dotnet restore $WindowsTestsProject
    if ($LASTEXITCODE -ne 0) {
        throw "dotnet restore for WinUI tests failed with exit code $LASTEXITCODE"
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
            "--package", "virtue-windows",
            "--lib"
        )
        if ($Profile -eq "Release") {
            $buildArgs += "--release"
        }

        & $cargo @buildArgs
        if ($LASTEXITCODE -ne 0) {
            throw "cargo build failed with exit code $LASTEXITCODE"
        }
    }

    if (-not (Test-Path (Join-Path $RustOutputDir "virtue_windows.dll"))) {
        throw "Missing Rust interop DLL at $(Join-Path $RustOutputDir 'virtue_windows.dll')"
    }

    & $dotnet test $WindowsTestsProject -c $Profile --no-restore
    if ($LASTEXITCODE -ne 0) {
        throw "dotnet test failed with exit code $LASTEXITCODE"
    }

    $manifestPublisher = Get-AppPackagePublisher -ManifestPath $WindowsAppManifest
    if ([string]::IsNullOrWhiteSpace($PackagePublisher)) {
        $PackagePublisher = $manifestPublisher
    }
    if (-not $SkipSigning) {
        $signingCertificate = Resolve-SigningConfiguration `
            -ManifestPublisher $PackagePublisher `
            -CertificateRoot $CertificateCacheDir `
            -SigningCertificatePath $SigningCertificatePath `
            -SigningCertificatePassword $SigningCertificatePassword `
            -SigningTimestampUrl $SigningTimestampUrl
        $PackagePublisher = $signingCertificate.Publisher
    }
    $originalManifestText = Set-AppPackageIdentityInManifest `
        -ManifestPath $WindowsAppManifest `
        -PackageVersion $PackageVersion `
        -Publisher $PackagePublisher
    if ($null -ne $signingCertificate -and $signingCertificate.RequiresCertificateBootstrap) {
        Ensure-TrustedCertificate -CertificatePath $signingCertificate.CerPath
    }

    $msbuildArgs = @(
        $WindowsAppProject,
        "/restore",
        "/p:Configuration=$Profile",
        "/p:Platform=x64",
        "/p:GenerateAppxPackageOnBuild=true",
        "/p:AppxPackageSigningEnabled=false",
        "/p:AppxBundle=Never",
        "/p:AppxPackageDir=$PackageOutputDir\",
        "/p:VirtueRustBinariesDir=$RustOutputDir",
        "/p:AppxPackageVersion=$PackageVersion"
    )

    & $msbuild @msbuildArgs
    if ($LASTEXITCODE -ne 0) {
        throw "MSBuild packaging failed with exit code $LASTEXITCODE"
    }

    $package = Get-ChildItem -Path $PackageOutputDir -Filter *.msix -Recurse |
        Sort-Object LastWriteTime -Descending |
        Select-Object -First 1

    if (-not $package) {
        throw "MSIX build did not produce an .msix artifact in $PackageOutputDir"
    }

    if (-not $SkipSigning) {
        $signArgs = @(
            "sign",
            "/fd", "SHA256",
            "/sha1", $signingCertificate.Thumbprint,
            "/sm"
        )
        if (-not [string]::IsNullOrWhiteSpace($signingCertificate.TimestampUrl)) {
            $signArgs += @("/tr", $signingCertificate.TimestampUrl, "/td", "SHA256")
        }
        $signArgs += $package.FullName

        & $signTool @signArgs
        if ($LASTEXITCODE -ne 0) {
            throw "signtool failed to sign $($package.FullName) with exit code $LASTEXITCODE"
        }
    }

    if (Test-Path $OutFile) {
        Remove-Item -Force $OutFile
    }
    Copy-Item -Force $package.FullName $OutFile
    if ($null -ne $signingCertificate -and $signingCertificate.RequiresCertificateBootstrap) {
        Copy-Item -Force $signingCertificate.CerPath $CertificateFile
    }
    New-SideloadInstallScript `
        -OutputPath $InstallScriptFile `
        -PackageFileName (Split-Path -Leaf $OutFile) `
        -CertificateFileName $(if ($null -ne $signingCertificate -and $signingCertificate.RequiresCertificateBootstrap) { Split-Path -Leaf $CertificateFile } else { "" })

    $packageLayoutDir = $package.Directory.FullName
    if (-not (Test-Path (Join-Path $packageLayoutDir "Install.ps1"))) {
        throw "Missing Install.ps1 in generated package layout at $packageLayoutDir"
    }

    if (Test-Path $SetupBundleDir) {
        Remove-Item -Recurse -Force $SetupBundleDir
    }
    New-Item -ItemType Directory -Force -Path $SetupBundleDir | Out-Null
    Copy-Item -Path (Join-Path $packageLayoutDir "*") -Destination $SetupBundleDir -Recurse -Force
    if ($null -ne $signingCertificate -and $signingCertificate.RequiresCertificateBootstrap) {
        Copy-Item -Force $signingCertificate.CerPath (Join-Path $SetupBundleDir (Split-Path -Leaf $CertificateFile))
    }
    $generatedInstallScript = Join-Path $SetupBundleDir "Install-AppDevPackage.ps1"
    Move-Item -Force (Join-Path $SetupBundleDir "Install.ps1") $generatedInstallScript
    $setupBundleInstallScript = Join-Path $SetupBundleDir "Install.ps1"
    $friendlyInstallScript = Join-Path $SetupBundleDir "Install-Virtue-MSIX.ps1"

    New-SetupLauncher -OutputPath (Join-Path $SetupBundleDir "Install-Virtue.cmd")
    New-SideloadInstallScript `
        -OutputPath $setupBundleInstallScript `
        -PackageFileName (Split-Path -Leaf $package.FullName) `
        -CertificateFileName $(if ($null -ne $signingCertificate -and $signingCertificate.RequiresCertificateBootstrap) { Split-Path -Leaf $CertificateFile } else { "" })
    Copy-Item -Force $setupBundleInstallScript $friendlyInstallScript

    if (Test-Path $SetupBundleZip) {
        Remove-Item -Force $SetupBundleZip
    }
    Compress-Archive -Path $SetupBundleDir -DestinationPath $SetupBundleZip -Force

    Write-Host "Built MSIX package: $OutFile"
    Write-Host "Package version: $PackageVersion"
    Write-Host "Package publisher: $PackagePublisher"
    if ($SkipSigning) {
        Write-Host "Skipped package signing; package artifacts are unsigned."
    } elseif ($signingCertificate.RequiresCertificateBootstrap) {
        Write-Host "Exported signing certificate: $CertificateFile"
    } else {
        Write-Host "Signed with trusted certificate subject: $($signingCertificate.Publisher)"
        if (-not [string]::IsNullOrWhiteSpace($signingCertificate.TimestampUrl)) {
            Write-Host "Timestamp authority: $($signingCertificate.TimestampUrl)"
        }
    }
    Write-Host "Built unsigned-install script: $InstallScriptFile"
    Write-Host "Built setup bundle: $SetupBundleZip"
}
finally {
    if ($null -ne $originalManifestText) {
        $utf8NoBom = New-Object System.Text.UTF8Encoding($false)
        [System.IO.File]::WriteAllText($WindowsAppManifest, $originalManifestText, $utf8NoBom)
    }
    if ($null -ne $signingCertificate -and $signingCertificate.ImportedToMachineStore -and $signingCertificate.Mode -eq "Trusted") {
        try {
            $store = New-Object System.Security.Cryptography.X509Certificates.X509Store("My", "LocalMachine")
            $store.Open("ReadWrite")
            $toRemove = $store.Certificates | Where-Object { $_.Thumbprint -eq $signingCertificate.Thumbprint } | Select-Object -First 1
            if ($toRemove) { $store.Remove($toRemove) }
            $store.Close()
        } catch {}
    }
    Pop-Location
}
