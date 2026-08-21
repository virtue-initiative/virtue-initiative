using System.Diagnostics;

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
    public static void Register(string exePath, string quietArg)
    {
        RunSchtasks(new[]
        {
            "/Create", "/F",
            "/SC", "MINUTE", "/MO", "1",
            "/TN", TaskName,
            "/TR", $"\"{exePath}\" {quietArg}",
            "/RL", "LIMITED",
        });
    }

    /// <summary>
    /// Removes the watchdog task so a deliberate process exit is not resurrected.
    /// </summary>
    public static void Unregister()
    {
        RunSchtasks(new[] { "/Delete", "/F", "/TN", TaskName });
    }

    private static void RunSchtasks(IEnumerable<string> arguments)
    {
        try
        {
            using var process = new Process();
            process.StartInfo.FileName = "schtasks.exe";
            process.StartInfo.UseShellExecute = false;
            process.StartInfo.CreateNoWindow = true;
            foreach (var arg in arguments)
            {
                process.StartInfo.ArgumentList.Add(arg);
            }

            process.Start();
            process.WaitForExit();
        }
        catch
        {
            // Best-effort: a failure to (un)register the watchdog should not block startup/exit.
        }
    }
}
