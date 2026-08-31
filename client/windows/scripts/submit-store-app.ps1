# Submits a release MSIX to the app's MAIN Microsoft Store listing.
#
# The sibling submit-store-flight.ps1 pushes staging builds to a package flight,
# which reaches only that flight's testers. This script drives the app-level
# resource instead, which is what Store customers actually install. The two are
# separate resources in the Store Submission API with separate submission
# queues: `/applications/{id}/submissions` here versus
# `/applications/{id}/flights/{flightId}/submissions` there, and the package
# list is `applicationPackages` rather than `flightPackages`.
#
# Unlike a flight submission, an app submission is created as a clone of the
# last published one, so the store listing, screenshots, age ratings and
# certification notes carry over untouched and only the package is swapped.

param(
    [Parameter(Mandatory = $true)]
    [string]$PackagePath,
    # Immediate is what makes "merge to main" mean "shipped". If the cloned
    # submission carried Manual, a release would otherwise stall at "ready to
    # publish" in Partner Center with nothing in CI reporting that it had.
    [ValidateSet("Immediate", "Manual")]
    [string]$TargetPublishMode = "Immediate",
    [int]$PollTimeoutSeconds = 900,
    [int]$PollIntervalSeconds = 20
)

$ErrorActionPreference = "Stop"

function Get-RequiredEnvVar {
    param([string]$Name)

    $value = [Environment]::GetEnvironmentVariable($Name)
    if ([string]::IsNullOrWhiteSpace($value)) {
        throw "$Name is required"
    }

    return $value
}

function Get-StoreAccessToken {
    param(
        [string]$TenantId,
        [string]$ClientId,
        [string]$ClientSecret
    )

    $tokenUri = "https://login.microsoftonline.com/$TenantId/oauth2/token"
    $body = @{
        grant_type    = "client_credentials"
        client_id     = $ClientId
        client_secret = $ClientSecret
        resource      = "https://manage.devcenter.microsoft.com"
    }

    $response = Invoke-RestMethod -Method Post -Uri $tokenUri -Body $body -ContentType "application/x-www-form-urlencoded"
    return $response.access_token
}

function Get-AuthHeaders {
    param([string]$AccessToken)

    return @{
        Authorization = "Bearer $AccessToken"
        Accept        = "application/json"
    }
}

function Remove-StaleAppSubmission {
    param(
        [string]$AppId,
        [hashtable]$Headers
    )

    # Same shape as the flight case: there is no "list submissions" endpoint, so
    # the app resource's pendingApplicationSubmission field is the only way to
    # discover an in-progress submission, and any pending submission blocks
    # creating a new one whatever stage it is at.
    $appUri = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$AppId"
    $app = Invoke-RestMethod -Method Get -Uri $appUri -Headers $Headers

    if (-not $app.pendingApplicationSubmission -or -not $app.pendingApplicationSubmission.id) {
        return
    }

    $pendingId = $app.pendingApplicationSubmission.id
    $statusUri = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$AppId/submissions/$pendingId/status"
    $status = Invoke-RestMethod -Method Get -Uri $statusUri -Headers $Headers

    Write-Host "Found in-progress submission $pendingId (status: $($status.status)); deleting before creating a new one."
    $deleteUri = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$AppId/submissions/$pendingId"
    try {
        Invoke-RestMethod -Method Delete -Uri $deleteUri -Headers $Headers -Body "{}" -ContentType "application/json" | Out-Null
    }
    catch {
        throw "A submission ($pendingId) is in progress for this app and could not be deleted automatically (status was $($status.status)). Check Partner Center before retrying. Error: $($_.Exception.Message)"
    }
}

function New-AppSubmission {
    param(
        [string]$AppId,
        [hashtable]$Headers
    )

    $createUri = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$AppId/submissions"
    # Unlike the flight endpoint, this one parses the request body as the
    # submission to create and validates it, so the flight script's "{}" is
    # rejected here with "The size of Listings must be 1 or more" — an empty
    # object is a submission with no listings. The docs say to send no body at
    # all; what the API actually requires is a zero-length one with a JSON
    # Content-Type (a POST carrying neither header is refused outright). An
    # explicit empty byte array pins that wire shape — Content-Length: 0 plus
    # the content type — rather than relying on how a given PowerShell version
    # frames an omitted -Body.
    return Invoke-RestMethod -Method Post -Uri $createUri -Headers $Headers -Body ([byte[]]::new(0)) -ContentType "application/json"
}

function Update-AppSubmissionPackages {
    param(
        [PSCustomObject]$Submission,
        [string]$PackageFileName,
        [string]$AppId,
        [hashtable]$Headers,
        [string]$PublishMode
    )

    $existingPackages = @()
    if ($null -ne $Submission.applicationPackages) {
        # Guarded rather than wrapped blindly in @(): @($null) yields a
        # one-element array holding $null, which serializes as a null entry in
        # applicationPackages and is rejected on commit.
        $existingPackages = @($Submission.applicationPackages)
    }

    foreach ($package in $existingPackages) {
        $package.fileStatus = "PendingDelete"
    }

    $newPackage = [PSCustomObject]@{
        fileName   = $PackageFileName
        fileStatus = "PendingUpload"
    }
    $Submission.applicationPackages = $existingPackages + $newPackage

    if ($Submission.targetPublishMode -ne $PublishMode) {
        Write-Host "Changing targetPublishMode from '$($Submission.targetPublishMode)' to '$PublishMode'."
        $Submission.targetPublishMode = $PublishMode
    }
    # A stale targetPublishDate cloned from a previous SpecificDate submission
    # is rejected once the mode is no longer SpecificDate.
    if ($Submission.PSObject.Properties.Name -contains "targetPublishDate") {
        $Submission.targetPublishDate = $null
    }

    $updateUri = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$AppId/submissions/$($Submission.id)"
    # Depth 100, not the flight script's 20. An app submission nests much
    # deeper than a flight one (listings -> per-locale -> baseListing ->
    # images/trailers, plus platformOverrides), and this PUT rewrites the whole
    # submission: anything mis-serialized here is what the listing becomes.
    # Past its depth limit ConvertTo-Json degrades the offending node to a
    # string rather than erroring — an array becomes "" and an object becomes
    # "@{images=System.Object[]; ...}" — so an under-set depth silently wipes
    # store listing content instead of failing the job. Real listings sit well
    # under 20, so this is headroom, not a fix for a known overflow.
    $body = $Submission | ConvertTo-Json -Depth 100
    return Invoke-RestMethod -Method Put -Uri $updateUri -Headers $Headers -Body $body -ContentType "application/json"
}

function Send-PackageZipToBlob {
    param(
        [string]$ZipPath,
        [string]$UploadUrl
    )

    $headers = @{ "x-ms-blob-type" = "BlockBlob" }
    Invoke-RestMethod -Method Put -Uri $UploadUrl -Headers $headers -InFile $ZipPath -ContentType "application/zip"
}

function Start-AppSubmissionCommit {
    param(
        [string]$AppId,
        [string]$SubmissionId,
        [hashtable]$Headers
    )

    $commitUri = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$AppId/submissions/$SubmissionId/commit"
    Invoke-RestMethod -Method Post -Uri $commitUri -Headers $Headers -Body "{}" -ContentType "application/json" | Out-Null
}

$terminalFailureStatuses = @("CommitFailed", "PreProcessingFailed", "CertificationFailed", "ReleaseFailed", "PublishFailed", "Canceled")

function Wait-AppSubmissionStatus {
    param(
        [string]$AppId,
        [string]$SubmissionId,
        [hashtable]$Headers,
        [int]$TimeoutSeconds,
        [int]$IntervalSeconds
    )

    $statusUri = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$AppId/submissions/$SubmissionId/status"
    $deadline = (Get-Date).AddSeconds($TimeoutSeconds)

    while ($true) {
        $status = Invoke-RestMethod -Method Get -Uri $statusUri -Headers $Headers
        Write-Host "Submission status: $($status.status)"

        if ($terminalFailureStatuses -contains $status.status) {
            $errorDetails = if ($status.statusDetails -and $status.statusDetails.errors) {
                ($status.statusDetails.errors | ForEach-Object { "$($_.code): $($_.details)" }) -join "; "
            } else {
                "(no error details returned)"
            }
            throw "Store submission failed with status $($status.status): $errorDetails"
        }

        if ($status.status -notin @("None", "PendingCommit", "CommitStarted", "PreProcessing")) {
            Write-Host "Submission passed initial processing with status: $($status.status)"
            return
        }

        if ((Get-Date) -ge $deadline) {
            Write-Warning "Timed out after $TimeoutSeconds seconds waiting for submission to clear initial processing (last status: $($status.status)). It is continuing to process in Partner Center; check there for final status."
            return
        }

        Start-Sleep -Seconds $IntervalSeconds
    }
}

if (-not (Test-Path $PackagePath)) {
    throw "Package not found at $PackagePath"
}

$StoreTenantId = Get-RequiredEnvVar -Name "STORE_TENANT_ID"
$StoreClientId = Get-RequiredEnvVar -Name "STORE_CLIENT_ID"
$StoreClientSecret = Get-RequiredEnvVar -Name "STORE_CLIENT_SECRET"
$StoreAppId = Get-RequiredEnvVar -Name "STORE_APP_ID"

Write-Host "Requesting Store access token"
$accessToken = Get-StoreAccessToken -TenantId $StoreTenantId -ClientId $StoreClientId -ClientSecret $StoreClientSecret
$headers = Get-AuthHeaders -AccessToken $accessToken

Write-Host "Checking for a stale in-progress submission on app $StoreAppId"
Remove-StaleAppSubmission -AppId $StoreAppId -Headers $headers

Write-Host "Creating new app submission"
$submission = New-AppSubmission -AppId $StoreAppId -Headers $headers
Write-Host "Created submission $($submission.id)"

$packageFileName = Split-Path -Leaf $PackagePath
$zipPath = [System.IO.Path]::ChangeExtension($PackagePath, ".zip")
if (Test-Path $zipPath) {
    Remove-Item -Force $zipPath
}
Compress-Archive -Path $PackagePath -DestinationPath $zipPath -Force

Write-Host "Updating submission package list with $packageFileName"
$submission = Update-AppSubmissionPackages -Submission $submission -PackageFileName $packageFileName -AppId $StoreAppId -Headers $headers -PublishMode $TargetPublishMode

Write-Host "Uploading package zip to Store blob storage"
Send-PackageZipToBlob -ZipPath $zipPath -UploadUrl $submission.fileUploadUrl

Write-Host "Committing submission $($submission.id)"
Start-AppSubmissionCommit -AppId $StoreAppId -SubmissionId $submission.id -Headers $headers

Write-Host "Polling submission status (up to $PollTimeoutSeconds seconds)"
Wait-AppSubmissionStatus -AppId $StoreAppId -SubmissionId $submission.id -Headers $headers -TimeoutSeconds $PollTimeoutSeconds -IntervalSeconds $PollIntervalSeconds

Write-Host "Store app submission complete"
