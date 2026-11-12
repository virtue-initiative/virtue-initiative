using System.Globalization;
using System.Security;

namespace Virtue.WindowsApp.Core.Interop;

/// <summary>
/// Per-user Scheduled Task that periodically relaunches the resident app if it isn't
/// running, covering accidental crashes/hangs. Windows' Application Recovery and
/// Restart API (RegisterApplicationRestart) does not automatically relaunch
/// MSIX-packaged apps, so this repo uses a Scheduled Task instead.
///
/// The task simply relaunches the exe on a fixed interval; the app's existing
/// single-instance redirect (see `App.EnsureSingleInstanceAsync`) makes each
/// attempt a no-op when the app is already running, so no separate "is it
/// running" check is needed here.
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
        if (string.Equals(exePath, AppLaunchPath.ExecutionAliasPath, StringComparison.OrdinalIgnoreCase))
        {
            return RegisterWithCleanup(exePath, quietArg);
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

    private static string RegisterWithCleanup(string aliasPath, string quietArg)
    {
        // schtasks reads an XML definition only from a file, so it goes to the temp path and is
        // deleted right after.
        var xmlPath = Path.Combine(Path.GetTempPath(), $"virtue-watchdog-{Guid.NewGuid():N}.xml");
        try
        {
            // UTF-16 with a BOM, matching the encoding the document declares.
            File.WriteAllText(xmlPath, BuildTaskXml(aliasPath, quietArg, DateTime.Now), System.Text.Encoding.Unicode);
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
    /// The PowerShell the task runs each minute (CLIENT-014). While the alias exists it starts
    /// the app (a no-op when already running). When it is gone the package was uninstalled, so
    /// after a short re-check that rules out the moment an update swaps the package, it deletes
    /// the device state under ProgramData and unregisters the task.
    /// </summary>
    public static string BuildScript(string aliasPath, string quietArg)
    {
        var alias = aliasPath.Replace("'", "''");
        var arg = quietArg.Replace("'", "''");
        return $"$a='{alias}'; "
            + $"if (Test-Path -LiteralPath $a) {{ Start-Process -FilePath $a -ArgumentList '{arg}' }} "
            + "else { Start-Sleep -Seconds 20; "
            + "if (-not (Test-Path -LiteralPath $a)) { "
            + "Remove-Item -LiteralPath (Join-Path $env:ProgramData 'Virtue') -Recurse -Force -ErrorAction SilentlyContinue; "
            + $"Unregister-ScheduledTask -TaskName '{TaskName}' -Confirm:$false -ErrorAction SilentlyContinue }} }}";
    }

    /// <summary>
    /// Task Scheduler 1.2 XML for the watchdog. <c>conhost.exe --headless</c> hosts PowerShell
    /// so no console window flashes every minute.
    /// </summary>
    public static string BuildTaskXml(string aliasPath, string quietArg, DateTime startAt)
    {
        var start = startAt.ToString("yyyy-MM-ddTHH:mm:ss", CultureInfo.InvariantCulture);
        var arguments = "--headless powershell.exe -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "
            + "\"" + BuildScript(aliasPath, quietArg) + "\"";

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
    /// Removes the watchdog task so a deliberate process exit is not resurrected.
    /// </summary>
    public static void Unregister() => Schtasks.Run("/Delete", "/F", "/TN", TaskName);
}
