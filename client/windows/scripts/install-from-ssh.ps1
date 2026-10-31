# Install a setup bundle and open the app in the desktop session, for use over SSH. Add-AppxPackage fails
# with 0x80070005 in an SSH logon and a GUI app started there has no desktop, so both run from one-off
# scheduled tasks in the interactive session.
param(
    [Parameter(Mandatory = $true)]
    [string]$SetupDir
)

$ErrorActionPreference = "Stop"

$taskName = "VirtueInstallFromSsh"
$logPath = Join-Path $env:TEMP "virtue-install-from-ssh.log"

function Invoke-InDesktopSession {
    param([string]$Execute, [string]$Argument = "", [string]$RunLevel = "Limited", [switch]$NoWait)

    $action = if ($Argument) {
        New-ScheduledTaskAction -Execute $Execute -Argument $Argument
    } else {
        New-ScheduledTaskAction -Execute $Execute
    }
    $principal = New-ScheduledTaskPrincipal -UserId $env:USERNAME -LogonType Interactive -RunLevel $RunLevel
    Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Force | Out-Null
    try {
        Start-ScheduledTask -TaskName $taskName
        Start-Sleep -Seconds 2
        $deadline = (Get-Date).AddMinutes(5)
        while (-not $NoWait -and (Get-ScheduledTask -TaskName $taskName).State -eq "Running") {
            if ((Get-Date) -gt $deadline) {
                throw "$Execute was still running after 5 minutes. See $logPath."
            }
            Start-Sleep -Seconds 2
        }
    }
    finally {
        Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
    }
}

$installScript = Join-Path $SetupDir "Install-AppDevPackage.ps1"
if (-not (Test-Path $installScript)) {
    throw "Missing $installScript"
}
Remove-Item -Force $logPath -ErrorAction SilentlyContinue

# The installer can't replace the package while the app is open. Its watchdog task reopens it within a
# minute, which is plenty for the install to start. -Force stops the installer waiting for someone to
# press Enter at the end.
$command = "Stop-Process -Name Virtue.WindowsApp -Force -ErrorAction SilentlyContinue; & '$installScript' -Force *> '$logPath'"
Invoke-InDesktopSession -Execute "powershell.exe" `
    -Argument "-NoProfile -ExecutionPolicy Bypass -WindowStyle Hidden -Command `"$command`"" -RunLevel Highest

if (Test-Path $logPath) {
    Get-Content $logPath
}

# Check the outcome, not the installer's exit code: when it replaces a package whose app was recently
# open, it can exit 1 with the new version installed. WindowsApps is read with `dir` because
# Get-AppxPackage sees nothing from an SSH logon and Get-ChildItem is denied there.
$msix = Get-ChildItem -Path $SetupDir -Filter "*.msix" | Select-Object -First 1
$version = $msix.Name.Split("_")[1]
$installed = cmd /c dir "$env:ProgramFiles\WindowsApps" /b | Where-Object { $_ -like "VirtueInitiative.*_${version}_*" }
if (-not $installed) {
    throw "Version $version of the app isn't installed. See the installer's output above."
}
Write-Host "Installed $installed"

# The app execution alias from the package's manifest.
Invoke-InDesktopSession -Execute (Join-Path $env:LOCALAPPDATA "Microsoft\WindowsApps\virtue-initiative.exe") -NoWait
Write-Host "Opened the app."
