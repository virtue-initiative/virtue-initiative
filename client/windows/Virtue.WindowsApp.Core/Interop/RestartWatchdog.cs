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
    /// Removes the watchdog task so a deliberate process exit is not resurrected.
    /// </summary>
    public static void Unregister() => Schtasks.Run("/Delete", "/F", "/TN", TaskName);
}
