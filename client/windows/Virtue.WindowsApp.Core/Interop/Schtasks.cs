using System.Diagnostics;

namespace Virtue.WindowsApp.Core.Interop;

/// <summary>
/// Thin wrapper around <c>schtasks.exe</c>, shared by <see cref="RestartWatchdog"/> and
/// <see cref="UpdateRelaunchTask"/>.
///
/// Task Scheduler is the only relaunch mechanism available to this app that survives a package
/// update: the OS terminates every process carrying the package identity for the swap, and a
/// helper we spawned ourselves would inherit that identity and be terminated along with us.
/// The scheduler service is outside the package entirely.
///
/// Every call is best-effort — a failure to register or delete a task must never block startup,
/// exit, or an update install — but the exit code and output are returned rather than swallowed
/// so callers can log them. <c>ui-startup.log</c> is the only observability channel on a Store
/// install, and a silently-failing task registration is exactly the class of bug that made
/// auto-update look dead for so long.
/// </summary>
internal static class Schtasks
{
    internal readonly record struct Result(bool Started, int ExitCode, string Output)
    {
        internal bool Succeeded => Started && ExitCode == 0;

        public override string ToString() =>
            Started ? $"exit {ExitCode}: {Output.Trim()}" : $"could not run schtasks.exe: {Output}";
    }

    internal static Result Run(params string[] arguments)
    {
        try
        {
            using var process = new Process();
            process.StartInfo.FileName = "schtasks.exe";
            process.StartInfo.UseShellExecute = false;
            process.StartInfo.CreateNoWindow = true;
            process.StartInfo.RedirectStandardOutput = true;
            process.StartInfo.RedirectStandardError = true;
            foreach (var argument in arguments)
            {
                process.StartInfo.ArgumentList.Add(argument);
            }

            process.Start();
            // Read before waiting: schtasks' output is small, but a full pipe buffer would
            // deadlock a WaitForExit-first ordering.
            var output = process.StandardOutput.ReadToEnd() + process.StandardError.ReadToEnd();
            process.WaitForExit();
            return new Result(Started: true, process.ExitCode, output);
        }
        catch (Exception ex)
        {
            return new Result(Started: false, ExitCode: -1, Output: ex.Message);
        }
    }
}
