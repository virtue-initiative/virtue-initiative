function Get-VersionProperty {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ClientRoot,
        [Parameter(Mandatory = $true)]
        [string]$Key
    )

    $versionFile = Join-Path $ClientRoot "version.properties"
    if (-not (Test-Path $versionFile)) {
        throw "Missing version file: $versionFile"
    }

    foreach ($line in Get-Content $versionFile) {
        if ($line -match "^\s*$Key=(.+)$") {
            return $Matches[1].Trim()
        }
    }

    throw "Missing $Key in $versionFile"
}

function Get-GitShortHash {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepoRoot
    )

    if ($env:VIRTUE_GIT_SHORT_HASH) {
        return $env:VIRTUE_GIT_SHORT_HASH
    }

    if ($env:GITHUB_SHA) {
        return $env:GITHUB_SHA.Substring(0, [Math]::Min(7, $env:GITHUB_SHA.Length))
    }

    $git = (Get-Command git -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if (-not $git) {
        return "unknown"
    }

    try {
        $hash = & $git -C $RepoRoot rev-parse --short HEAD 2>$null
    } catch {
        return "unknown"
    }

    if ($LASTEXITCODE -ne 0) {
        return "unknown"
    }

    return $hash.Trim()
}

function Get-BuildDate {
    if ($env:VIRTUE_BUILD_DATE) {
        return $env:VIRTUE_BUILD_DATE
    }

    return [DateTime]::UtcNow.ToString("yyyy-MM-dd")
}

function Get-GitRefName {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepoRoot
    )

    if ($env:VIRTUE_GIT_REF_NAME) {
        return $env:VIRTUE_GIT_REF_NAME
    }

    if ($env:GITHUB_REF_NAME) {
        return $env:GITHUB_REF_NAME
    }

    $git = (Get-Command git -ErrorAction SilentlyContinue | Select-Object -First 1).Source
    if (-not $git) {
        return "detached"
    }

    try {
        $branchName = & $git -C $RepoRoot rev-parse --abbrev-ref HEAD 2>$null
    } catch {
        return "detached"
    }

    if ($LASTEXITCODE -ne 0) {
        return "detached"
    }

    $branchName = $branchName.Trim()
    if ($branchName -and $branchName -ne "HEAD") {
        return $branchName
    }

    return "detached"
}

function Get-ReleaseChannel {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepoRoot
    )

    if ($env:VIRTUE_RELEASE_CHANNEL) {
        if ($env:VIRTUE_RELEASE_CHANNEL -in @("stable", "dev")) {
            return $env:VIRTUE_RELEASE_CHANNEL
        }

        throw "Unsupported VIRTUE_RELEASE_CHANNEL: $($env:VIRTUE_RELEASE_CHANNEL)"
    }

    if ((Get-GitRefName -RepoRoot $RepoRoot) -eq "main") {
        return "stable"
    }

    return "dev"
}

function Get-VirtueVersionInfo {
    param(
        [string]$ClientRoot = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)),
        [string]$RepoRoot = (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)))
    )

    $baseVersion = Get-VersionProperty -ClientRoot $ClientRoot -Key "VERSION"
    $androidVersionCode = [int](Get-VersionProperty -ClientRoot $ClientRoot -Key "ANDROID_VERSION_CODE")
    $appleBuildNumber = [int](Get-VersionProperty -ClientRoot $ClientRoot -Key "APPLE_BUILD_NUMBER")
    $buildDate = Get-BuildDate
    $gitShortHash = Get-GitShortHash -RepoRoot $RepoRoot
    $gitRefName = Get-GitRefName -RepoRoot $RepoRoot
    $releaseChannel = Get-ReleaseChannel -RepoRoot $RepoRoot
    $releaseTag = if ($releaseChannel -eq "stable") { $baseVersion } else { "$baseVersion-dev" }
    $buildLabel = "$releaseTag-$buildDate-$gitShortHash"

    [pscustomobject]@{
        BaseVersion = $baseVersion
        AndroidVersionCode = $androidVersionCode
        AppleBuildNumber = $appleBuildNumber
        BuildDate = $buildDate
        GitShortHash = $gitShortHash
        GitRefName = $gitRefName
        ReleaseChannel = $releaseChannel
        ReleaseTag = $releaseTag
        BuildLabel = $buildLabel
    }
}
