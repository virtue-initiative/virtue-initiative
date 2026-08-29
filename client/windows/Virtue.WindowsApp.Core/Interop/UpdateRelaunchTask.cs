using System.Globalization;
using System.Security;
using System.Text;

namespace Virtue.WindowsApp.Core.Interop;

/// <summary>
/// One-shot Scheduled Task that brings the app back a few seconds after a Store update
/// terminates it, instead of leaving the relaunch to <see cref="RestartWatchdog"/>'s per-minute
/// poll.
///
/// <para>
/// <b>Why not just restart ourselves.</b> Installing a Store update means the OS terminates this
/// process for the package swap, so there is no "after" in which to call anything —
/// <c>Microsoft.Windows.AppLifecycle.AppInstance.Restart</c> in particular cannot work here on
/// three counts: its helper agent relaunches by <c>CreateProcess</c> on the caller's executable
/// path, which for an MSIX app is the version-stamped directory the update deletes (the exact
/// failure <see cref="AppLaunchPath"/> exists to avoid); it must be called from a live process,
/// which we are not once the swap begins; and the automatic path runs window-closed, which risks
/// <c>AppRestartFailureReason.NotInForeground</c>. The relaunch has to be owned by something
/// outside the package, and Task Scheduler already is.
/// </para>
///
/// <para>
/// <b>Why a TimeTrigger and not the obvious alternatives.</b> Task Scheduler's well-known
/// one-minute floor is narrower than it looks: it binds <c>Repetition/Interval</c>, which the
/// schema pins at <c>minInclusive="PT1M"</c> (that is the floor <see cref="RestartWatchdog"/>
/// runs into), and it binds <c>schtasks /ST</c>, documented as <c>HH:mm</c> with no seconds
/// field. Neither binds a <c>TimeTrigger</c>'s <c>StartBoundary</c>, which is an
/// <c>xs:dateTime</c> — Microsoft's own Time Trigger example schedules Notepad 30 seconds out.
/// A <c>RegistrationTrigger</c> with a sub-minute <c>Delay</c> looks like a better fit (relative,
/// so it dodges clock skew) and its schema imposes no minimum, but measured on this repo's
/// <c>virtue-win11</c> VM it never fires at all when registered through
/// <c>schtasks /Create /XML</c>: the task registers, then sits at
/// <c>SCHED_S_TASK_HAS_NOT_RUN</c> (<c>267011</c>) forever. The <c>TimeTrigger</c> shape below
/// was measured firing at 15.86s for a <see cref="DefaultDelay"/> of 15s, with
/// <c>DeleteExpiredTaskAfter</c> removing the task on schedule afterwards.
/// </para>
///
/// <para>
/// <b>This is an optimization, never a guarantee.</b> <see cref="RestartWatchdog"/> stays
/// registered across the whole update and remains the actual safety net. If this task fires
/// while the package swap is still in flight, the launch simply fails and the single shot is
/// spent — the watchdog still picks the app back up within a minute, which is exactly the
/// behaviour that shipped before this existed. Every failure here degrades to that.
/// </para>
/// </summary>
public static class UpdateRelaunchTask
{
    private const string TaskName = "VirtueUpdateRelaunch";

    /// <summary>
    /// How long after scheduling the relaunch fires. Long enough for the package swap that
    /// follows to finish, short enough to beat the watchdog's per-minute poll by a wide margin.
    /// </summary>
    public static readonly TimeSpan DefaultDelay = TimeSpan.FromSeconds(15);

    /// <summary>
    /// How long after <see cref="DefaultDelay"/> the trigger stays live. Only reached when the
    /// task could not run at its start boundary (the machine was asleep, say); past it the task
    /// is expired and <c>DeleteExpiredTaskAfter</c> collects it.
    /// </summary>
    private static readonly TimeSpan TriggerWindow = TimeSpan.FromMinutes(1);

    /// <summary>
    /// Registers the one-shot relaunch. Call it immediately before an install attempt: the
    /// countdown starts now, not when the process actually dies.
    /// </summary>
    /// <param name="exePath">
    /// Launch path for the relaunched app — pass <see cref="AppLaunchPath"/>'s resolved alias,
    /// never <see cref="Environment.ProcessPath"/>, which the update is about to delete.
    /// </param>
    /// <param name="arguments">Command-line arguments for the relaunched process.</param>
    /// <param name="delay">Overrides <see cref="DefaultDelay"/>; for tests.</param>
    /// <returns>A human-readable outcome for the startup log.</returns>
    public static string Schedule(string exePath, string arguments, TimeSpan? delay = null)
    {
        var startAt = DateTime.Now + (delay ?? DefaultDelay);
        var xml = BuildTaskXml(exePath, arguments, startAt, TriggerWindow);

        // schtasks /XML reads the definition from a file; there is no way to pipe it in.
        var xmlPath = Path.Combine(Path.GetTempPath(), $"virtue-update-relaunch-{Guid.NewGuid():N}.xml");
        try
        {
            // UTF-16 with a BOM, matching the encoding the document declares. schtasks rejects
            // the file outright otherwise.
            File.WriteAllText(xmlPath, xml, Encoding.Unicode);
            var result = Schtasks.Run("/Create", "/F", "/TN", TaskName, "/XML", xmlPath);
            return result.Succeeded
                ? $"Update relaunch task scheduled for {startAt:HH:mm:ss} ({exePath})."
                : $"Update relaunch task could not be scheduled ({result}); falling back to the watchdog.";
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return $"Update relaunch task could not be scheduled ({ex.Message}); falling back to the watchdog.";
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
    /// Removes a pending relaunch. Safe to call when no task exists, and called unconditionally
    /// at startup: by the time the app is running the relaunch has either done its job or been
    /// made redundant by whatever else started us, and a task left armed would fire into a
    /// running app for no reason.
    /// </summary>
    public static void Cancel() => Schtasks.Run("/Delete", "/F", "/TN", TaskName);

    /// <summary>
    /// Builds the Task Scheduler 1.2 XML. Split out from <see cref="Schedule"/> so the wire
    /// format is unit-testable without registering anything.
    ///
    /// <c>Principal</c> deliberately carries no <c>UserId</c>: filling one in means resolving the
    /// current account to a SID, which <c>schtasks</c> does for us (verified on the VM: the
    /// registered task reports <c>Run As User: help</c>), and which this <c>net8.0</c> project
    /// could not do anyway without a Windows-only <c>WindowsIdentity</c> dependency it otherwise
    /// avoids.
    ///
    /// <c>StartBoundary</c> is written as a local time with no UTC offset, which is how Task
    /// Scheduler interprets a bare <c>xs:dateTime</c> and how the measured probe ran.
    /// </summary>
    public static string BuildTaskXml(string exePath, string arguments, DateTime startAt, TimeSpan triggerWindow)
    {
        var start = FormatBoundary(startAt);
        var end = FormatBoundary(startAt + triggerWindow);

        return $"""
            <?xml version="1.0" encoding="UTF-16"?>
            <Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
              <RegistrationInfo>
                <Description>Relaunches Virtue after a Microsoft Store package update. Created by the app just before it installs an update; removed automatically once it has run.</Description>
              </RegistrationInfo>
              <Triggers>
                <TimeTrigger>
                  <Enabled>true</Enabled>
                  <StartBoundary>{start}</StartBoundary>
                  <EndBoundary>{end}</EndBoundary>
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
                <AllowHardTerminate>true</AllowHardTerminate>
                <ExecutionTimeLimit>PT5M</ExecutionTimeLimit>
                <DeleteExpiredTaskAfter>PT30S</DeleteExpiredTaskAfter>
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
