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
        throw "git not found and no VIRTUE_GIT_SHORT_HASH or GITHUB_SHA override was provided."
    }

    $hash = & $git -C $RepoRoot rev-parse --short HEAD
    if ($LASTEXITCODE -ne 0) {
        throw "git rev-parse failed with exit code $LASTEXITCODE"
    }

    return $hash.Trim()
}

function Get-VirtueVersionInfo {
    param(
        [string]$ClientRoot = (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)),
        [string]$RepoRoot = (Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)))
    )

    $baseVersion = Get-VersionProperty -ClientRoot $ClientRoot -Key "VERSION"
    $androidVersionCode = [int](Get-VersionProperty -ClientRoot $ClientRoot -Key "ANDROID_VERSION_CODE")
    $appleBuildNumber = [int](Get-VersionProperty -ClientRoot $ClientRoot -Key "APPLE_BUILD_NUMBER")
    $gitShortHash = Get-GitShortHash -RepoRoot $RepoRoot
    $buildLabel = "$baseVersion-$gitShortHash"

    [pscustomobject]@{
        BaseVersion = $baseVersion
        AndroidVersionCode = $androidVersionCode
        AppleBuildNumber = $appleBuildNumber
        GitShortHash = $gitShortHash
        BuildLabel = $buildLabel
    }
}
