using System.Globalization;
using System.Security;

namespace Virtue.WindowsApp.Core.Interop;

/// <summary>
/// Per-user Scheduled Task that periodically relaunches the resident app if it isn't
/// running, covering accidental crashes/hangs. Windows' Application Recovery and
/// Restart API (RegisterApplicationRestart) does not automatically relaunch
/// MSIX-packaged apps, so this repo uses a Scheduled Task instead.
///
/// When the task targets the app execution alias it is a PowerShell script (CLIENT-014)
/// that, once a minute, relaunches the app while the package is installed and cleans up
/// after an uninstall. The app's existing single-instance redirect (see
/// `App.EnsureSingleInstanceAsync`) makes each relaunch a no-op when the app is already
/// running, so no separate "is it running" check is needed. After a deliberate Exit the
/// same task is re-registered cleanup-only (<see cref="StopRelaunching"/>). A task aimed at
/// any other path (an unpackaged or dev run) just launches that exe every minute.
///
/// 1 minute is Task Scheduler's floor for a repeating trigger — the schema pins
/// `Repetition/Interval` at `minInclusive="PT1M"`, so schtasks.exe rejects a
/// sub-minute `Interval` (e.g. `PT45S`) as out of range even via an XML task
/// definition. That floor is specific to *repetition*: a one-shot `TimeTrigger`
/// can be aimed seconds out, which is how <see cref="UpdateRelaunchTask"/> beats
/// this poll after an update. This task stays the general-purpose safety net.
/// </summary>
public static class RestartWatchdog
{
    private const string TaskName = "VirtueResidentWatchdog";
    private const string PackageName = "VirtueInitiative.VirtueAccountability";

    /// <summary>
    /// Creates (or refreshes) the per-user watchdog task. Safe to call repeatedly.
    /// </summary>
    /// <param name="exePath">Full path to the resident app's executable.</param>
    /// <param name="quietArg">
    /// Command-line arg the relaunched process should receive, so it comes back
    /// into the tray quietly rather than popping a window.
    /// </param>
    /// <returns>A human-readable outcome for the startup log.</returns>
    public static string Register(string exePath, string quietArg)
    {
        // CLIENT-014: when the task launches the execution alias it also cleans up after an
        // uninstall (MSIX runs no app code then), so it needs the script form. A task aimed at
        // anything else (an unpackaged or dev run) keeps the plain per-minute launch.
        if (string.Equals(exePath, AppLaunchPath.ExecutionAliasPath, StringComparison.OrdinalIgnoreCase) || IsPackaged())
        {
            // Packaged with the alias switched off: the script still guards the cleanup and
            // simply does not relaunch until the alias is back.
            return RegisterWithCleanup(AppLaunchPath.ExecutionAliasPath, quietArg, relaunch: true);
        }

        var result = Schtasks.Run(
            "/Create", "/F",
            "/SC", "MINUTE", "/MO", "1",
            "/TN", TaskName,
            "/TR", $"\"{exePath}\" {quietArg}",
            "/RL", "LIMITED");
        return result.Succeeded
            ? $"Restart watchdog registered ({exePath})."
            : $"Restart watchdog registration failed ({result}).";
    }

    /// <summary>
    /// Called on a deliberate Exit. A packaged install keeps the task but only for the
    /// CLIENT-014 uninstall cleanup, so the app is not relaunched yet an uninstall still
    /// removes the device state. Any other task is deleted, as it has nothing to clean up.
    /// </summary>
    public static void StopRelaunching(string quietArg)
    {
        if (IsPackaged())
        {
            RegisterWithCleanup(AppLaunchPath.ExecutionAliasPath, quietArg, relaunch: false);
        }
        else
        {
            Unregister();
        }
    }

    // A packaged run lives under ...\WindowsApps\. Used instead of the alias, which the user
    // can switch off while the app stays installed.
    private static bool IsPackaged() =>
        Environment.ProcessPath?.Contains(@"\WindowsApps\", StringComparison.OrdinalIgnoreCase) == true;

    private static string RegisterWithCleanup(string aliasPath, string quietArg, bool relaunch)
    {
        // schtasks reads an XML definition only from a file, so it goes to the temp path and is
        // deleted right after.
        var xmlPath = Path.Combine(Path.GetTempPath(), $"virtue-watchdog-{Guid.NewGuid():N}.xml");
        try
        {
            // UTF-16 with a BOM, matching the encoding the document declares.
            File.WriteAllText(xmlPath, BuildTaskXml(aliasPath, quietArg, DateTime.Now, relaunch), System.Text.Encoding.Unicode);
            var result = Schtasks.Run("/Create", "/F", "/TN", TaskName, "/XML", xmlPath);
            return result.Succeeded
                ? $"Restart watchdog registered with uninstall cleanup ({aliasPath})."
                : $"Restart watchdog registration failed ({result}).";
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return $"Restart watchdog registration failed ({ex.Message}).";
        }
        finally
        {
            try
            {
                File.Delete(xmlPath);
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                // A leftover temp file is harmless.
            }
        }
    }

    /// <summary>
    /// The PowerShell the task runs each minute (CLIENT-014). While the package is installed it
    /// starts the app through the alias (a no-op when already running), or does nothing if the
    /// alias is switched off. When the package is gone, after a short re-check that rules out the
    /// moment an update swaps the package, it deletes the device state under ProgramData and
    /// unregisters the task. With <paramref name="relaunch"/> false it never starts the app.
    /// </summary>
    public static string BuildScript(string aliasPath, string quietArg, bool relaunch = true)
    {
        var alias = aliasPath.Replace("'", "''");
        var arg = quietArg.Replace("'", "''");
        // A failed query counts as installed, so only a clean empty answer can delete anything.
        var installed = $"$(try {{ [bool](Get-AppxPackage -Name '{PackageName}' -ErrorAction Stop) }} catch {{ $true }})";
        var start = relaunch
            ? "if (Test-Path -LiteralPath $a) { Start-Process -FilePath $a -ArgumentList '" + arg + "' } "
            : "";
        return $"$a='{alias}'; "
            + $"if ({installed}) {{ {start}}} "
            + "else { Start-Sleep -Seconds 20; "
            + $"if (-not {installed}) {{ "
            + "Remove-Item -LiteralPath (Join-Path $env:ProgramData 'Virtue') -Recurse -Force -ErrorAction SilentlyContinue; "
            + $"Unregister-ScheduledTask -TaskName '{TaskName}' -Confirm:$false -ErrorAction SilentlyContinue }} }}";
    }

    /// <summary>
    /// Task Scheduler 1.2 XML for the watchdog. <c>conhost.exe --headless</c> hosts PowerShell
    /// so no console window flashes every minute.
    /// </summary>
    public static string BuildTaskXml(string aliasPath, string quietArg, DateTime startAt, bool relaunch = true)
    {
        var start = startAt.ToString("yyyy-MM-ddTHH:mm:ss", CultureInfo.InvariantCulture);
        var arguments = "--headless powershell.exe -NoProfile -NonInteractive -Command "
            + "\"" + BuildScript(aliasPath, quietArg, relaunch) + "\"";

        return $"""
            <?xml version="1.0" encoding="UTF-16"?>
            <Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
              <RegistrationInfo>
                <Description>Relaunches Virtue if it is not running. Removes itself and its device state once Virtue is uninstalled.</Description>
              </RegistrationInfo>
              <Triggers>
                <TimeTrigger>
                  <Repetition>
                    <Interval>PT1M</Interval>
                  </Repetition>
                  <StartBoundary>{start}</StartBoundary>
                  <Enabled>true</Enabled>
                </TimeTrigger>
              </Triggers>
              <Principals>
                <Principal id="Author">
                  <LogonType>InteractiveToken</LogonType>
                  <RunLevel>LeastPrivilege</RunLevel>
                </Principal>
              </Principals>
              <Settings>
                <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
                <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
                <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
                <StartWhenAvailable>true</StartWhenAvailable>
                <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
                <AllowStartOnDemand>true</AllowStartOnDemand>
                <Enabled>true</Enabled>
                <Hidden>true</Hidden>
                <ExecutionTimeLimit>PT5M</ExecutionTimeLimit>
                <Priority>7</Priority>
              </Settings>
              <Actions Context="Author">
                <Exec>
                  <Command>conhost.exe</Command>
                  <Arguments>{SecurityElement.Escape(arguments)}</Arguments>
                </Exec>
              </Actions>
            </Task>
            """;
    }

    /// <summary>
    /// Deletes the watchdog task (a non-packaged run's deliberate exit; see <see cref="StopRelaunching"/>).
    /// </summary>
    public static void Unregister() => Schtasks.Run("/Delete", "/F", "/TN", TaskName);
}
