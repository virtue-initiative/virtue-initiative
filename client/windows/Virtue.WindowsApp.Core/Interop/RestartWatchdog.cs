using System.Globalization;
using System.Security;

namespace Virtue.WindowsApp.Core.Interop;

/// <summary>
/// Per-user Scheduled Task that periodically relaunches the resident app if it isn't
/// running, covering accidental crashes/hangs. Windows' Application Recovery and
/// Restart API (RegisterApplicationRestart) does not automatically relaunch
/// MSIX-packaged apps, so this repo uses a Scheduled Task instead (CLIENT-028).
///
/// The task runs the app's own executable directly. The app's single-instance redirect (see
/// `App.EnsureSingleInstanceAsync`) makes each relaunch a no-op when the app is already
/// running, so no separate "is it running" check is needed. An earlier version ran a hidden
/// PowerShell script here that also deleted the device state after an uninstall, and
/// behavior-based antivirus (Bitdefender) quarantined it as malware. The state now lives inside
/// the package, which Windows deletes itself (CLIENT-002, CLIENT-014).
///
/// The task is the one thing the package does not own, so it expires: its trigger ends
/// <see cref="Lifetime"/> after registration, and Task Scheduler deletes it once expired. The
/// app registers it again at every launch and every <see cref="RenewInterval"/> while running,
/// so it never expires while Virtue is installed and in use, and it disappears on its own within
/// about a day of an uninstall.
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

    /// <summary>How long after registration the task's trigger ends (CLIENT-028).</summary>
    public static readonly TimeSpan Lifetime = TimeSpan.FromDays(1);

    /// <summary>How often the running app registers the task again (CLIENT-028).</summary>
    public static readonly TimeSpan RenewInterval = TimeSpan.FromHours(6);

    /// <summary>
    /// How often the running app checks whether <see cref="IsRenewalDue"/>. A timer does not
    /// count time the PC is asleep, so a 6-hour timer could leave a laptop that slept for a day
    /// waking to an expired, deleted task and waiting hours more for the next renewal. Checking
    /// wall-clock time every minute renews it within a minute of waking.
    /// </summary>
    public static readonly TimeSpan RenewalCheckInterval = TimeSpan.FromMinutes(1);

    /// <summary>
    /// Whether the task should be registered again (CLIENT-028): once <see cref="RenewInterval"/>
    /// of wall-clock time has passed since <paramref name="lastRegisteredAt"/>, including time
    /// asleep, or when the clock has moved back before it.
    /// </summary>
    public static bool IsRenewalDue(DateTime lastRegisteredAt, DateTime now) =>
        now < lastRegisteredAt || now - lastRegisteredAt >= RenewInterval;

    /// <summary>
    /// Creates or replaces the per-user watchdog task, with a fresh expiry. Safe to call
    /// repeatedly. Replacing it also turns a task left by an older version (the PowerShell
    /// form) into this one.
    /// </summary>
    /// <param name="exePath">Full path to the resident app's executable.</param>
    /// <param name="quietArg">
    /// Command-line arg the relaunched process should receive, so it comes back
    /// into the tray quietly rather than popping a window.
    /// </param>
    /// <returns>A human-readable outcome for the startup log.</returns>
    public static string Register(string exePath, string quietArg)
    {
        var registeredAt = DateTime.Now;
        var result = Schtasks.CreateFromXml(TaskName, BuildTaskXml(exePath, quietArg, registeredAt));
        return result.Succeeded
            ? $"Restart watchdog registered until {registeredAt + Lifetime:yyyy-MM-dd HH:mm} ({exePath})."
            : $"Restart watchdog registration failed ({result}).";
    }

    /// <summary>
    /// Deletes the watchdog task (a deliberate Exit, CLIENT-011).
    /// </summary>
    public static void Unregister() => Schtasks.Run("/Delete", "/F", "/TN", TaskName);

    /// <summary>
    /// Task Scheduler 1.2 XML for the watchdog: every minute from <paramref name="registeredAt"/>
    /// until <see cref="Lifetime"/> later, then deleted. Like <see cref="UpdateRelaunchTask"/>,
    /// the boundaries are local times with no offset and the principal has no <c>UserId</c>.
    ///
    /// <c>ExecutionTimeLimit</c> is <c>PT0S</c> (none) because the process the task starts is the
    /// resident app itself, and Task Scheduler stops a task's process at its limit: with the
    /// <c>PT5M</c> an earlier draft had, the VM showed the app killed five minutes after every
    /// watchdog relaunch.
    /// </summary>
    public static string BuildTaskXml(string exePath, string arguments, DateTime registeredAt)
    {
        var start = FormatBoundary(registeredAt);
        var end = FormatBoundary(registeredAt + Lifetime);

        return $"""
            <?xml version="1.0" encoding="UTF-16"?>
            <Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
              <RegistrationInfo>
                <Description>Relaunches Virtue if it is not running. Virtue renews this task while it runs; it expires and is deleted a day after Virtue stops renewing it.</Description>
              </RegistrationInfo>
              <Triggers>
                <TimeTrigger>
                  <Repetition>
                    <Interval>PT1M</Interval>
                  </Repetition>
                  <StartBoundary>{start}</StartBoundary>
                  <EndBoundary>{end}</EndBoundary>
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
                <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
                <DeleteExpiredTaskAfter>PT0S</DeleteExpiredTaskAfter>
                <Priority>7</Priority>
              </Settings>
              <Actions Context="Author">
                <Exec>
                  <Command>{Escape(exePath)}</Command>
                  <Arguments>{Escape(arguments)}</Arguments>
                </Exec>
              </Actions>
            </Task>
            """;
    }

    private static string FormatBoundary(DateTime value) =>
        value.ToString("yyyy-MM-ddTHH:mm:ss", CultureInfo.InvariantCulture);

    private static string Escape(string value) => SecurityElement.Escape(value) ?? string.Empty;
}
