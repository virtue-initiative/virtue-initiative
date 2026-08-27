param(
    [Parameter(Mandatory = $true)]
    [string]$PackagePath,
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

function Remove-StaleFlightSubmission {
    param(
        [string]$AppId,
        [string]$FlightId,
        [hashtable]$Headers
    )

    # There is no "list submissions for a flight" endpoint in the Store Submission
    # API; the only way to discover an in-progress submission is the flight
    # resource's pendingFlightSubmission field.
    $flightUri = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$AppId/flights/$FlightId"
    $flight = Invoke-RestMethod -Method Get -Uri $flightUri -Headers $Headers

    if (-not $flight.pendingFlightSubmission -or -not $flight.pendingFlightSubmission.id) {
        return
    }

    # Any pending submission blocks creating a new one, whatever stage it is at
    # (PendingCommit, CommitStarted, PreProcessing, Certification, ...), so cancel
    # it unconditionally rather than only in one status. The status fetch is for
    # logging; DELETE is the API's only cancel mechanism and returns 409 for the
    # states it genuinely refuses to drop.
    $pendingId = $flight.pendingFlightSubmission.id
    $statusUri = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$AppId/flights/$FlightId/submissions/$pendingId/status"
    $status = Invoke-RestMethod -Method Get -Uri $statusUri -Headers $Headers

    Write-Host "Found in-progress submission $pendingId (status: $($status.status)); deleting before creating a new one."
    $deleteUri = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$AppId/flights/$FlightId/submissions/$pendingId"
    try {
        Invoke-RestMethod -Method Delete -Uri $deleteUri -Headers $Headers -Body "{}" -ContentType "application/json" | Out-Null
    }
    catch {
        throw "A submission ($pendingId) is in progress for this flight and could not be deleted automatically (status was $($status.status)). Check Partner Center before retrying. Error: $($_.Exception.Message)"
    }
}

function New-FlightSubmission {
    param(
        [string]$AppId,
        [string]$FlightId,
        [hashtable]$Headers
    )

    $createUri = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$AppId/flights/$FlightId/submissions"
    # The API rejects a POST with no body/Content-Type ("Only JSON content is
    # accepted"); Invoke-RestMethod only attaches a Content-Type header when
    # -Body is present, so an explicit empty JSON body is required even though
    # the request logically carries no data.
    return Invoke-RestMethod -Method Post -Uri $createUri -Headers $Headers -Body "{}" -ContentType "application/json"
}

function Update-FlightSubmissionPackages {
    param(
        [PSCustomObject]$Submission,
        [string]$PackageFileName,
        [string]$AppId,
        [string]$FlightId,
        [hashtable]$Headers
    )

    foreach ($package in $Submission.flightPackages) {
        $package.fileStatus = "PendingDelete"
    }

    $newPackage = [PSCustomObject]@{
        fileName   = $PackageFileName
        fileStatus = "PendingUpload"
    }
    $Submission.flightPackages = @($Submission.flightPackages) + $newPackage

    $updateUri = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$AppId/flights/$FlightId/submissions/$($Submission.id)"
    $body = $Submission | ConvertTo-Json -Depth 20
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

function Start-FlightSubmissionCommit {
    param(
        [string]$AppId,
        [string]$FlightId,
        [string]$SubmissionId,
        [hashtable]$Headers
    )

    $commitUri = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$AppId/flights/$FlightId/submissions/$SubmissionId/commit"
    Invoke-RestMethod -Method Post -Uri $commitUri -Headers $Headers -Body "{}" -ContentType "application/json" | Out-Null
}

$terminalFailureStatuses = @("CommitFailed", "PreProcessingFailed", "CertificationFailed", "ReleaseFailed", "PublishFailed", "Canceled")

function Wait-FlightSubmissionStatus {
    param(
        [string]$AppId,
        [string]$FlightId,
        [string]$SubmissionId,
        [hashtable]$Headers,
        [int]$TimeoutSeconds,
        [int]$IntervalSeconds
    )

    $statusUri = "https://manage.devcenter.microsoft.com/v1.0/my/applications/$AppId/flights/$FlightId/submissions/$SubmissionId/status"
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
$StoreFlightId = Get-RequiredEnvVar -Name "STORE_FLIGHT_ID"

Write-Host "Requesting Store access token"
$accessToken = Get-StoreAccessToken -TenantId $StoreTenantId -ClientId $StoreClientId -ClientSecret $StoreClientSecret
$headers = Get-AuthHeaders -AccessToken $accessToken

Write-Host "Checking for a stale in-progress submission on flight $StoreFlightId"
Remove-StaleFlightSubmission -AppId $StoreAppId -FlightId $StoreFlightId -Headers $headers

Write-Host "Creating new flight submission"
$submission = New-FlightSubmission -AppId $StoreAppId -FlightId $StoreFlightId -Headers $headers
Write-Host "Created submission $($submission.id)"

$packageFileName = Split-Path -Leaf $PackagePath
$zipPath = [System.IO.Path]::ChangeExtension($PackagePath, ".zip")
if (Test-Path $zipPath) {
    Remove-Item -Force $zipPath
}
Compress-Archive -Path $PackagePath -DestinationPath $zipPath -Force

Write-Host "Updating submission package list with $packageFileName"
$submission = Update-FlightSubmissionPackages -Submission $submission -PackageFileName $packageFileName -AppId $StoreAppId -FlightId $StoreFlightId -Headers $headers

Write-Host "Uploading package zip to Store blob storage"
Send-PackageZipToBlob -ZipPath $zipPath -UploadUrl $submission.fileUploadUrl

Write-Host "Committing submission $($submission.id)"
Start-FlightSubmissionCommit -AppId $StoreAppId -FlightId $StoreFlightId -SubmissionId $submission.id -Headers $headers

Write-Host "Polling submission status (up to $PollTimeoutSeconds seconds)"
Wait-FlightSubmissionStatus -AppId $StoreAppId -FlightId $StoreFlightId -SubmissionId $submission.id -Headers $headers -TimeoutSeconds $PollTimeoutSeconds -IntervalSeconds $PollIntervalSeconds

Write-Host "Store flight submission complete"
