using System.Xml.Linq;
using Virtue.WindowsApp.Core.Interop;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class RestartWatchdogTests
{
    private static readonly XNamespace Ns = "http://schemas.microsoft.com/windows/2004/02/mit/task";

    private static XElement Build() =>
        XDocument.Parse(RestartWatchdog.BuildTaskXml("--restarted-by-watchdog", new DateTime(2026, 8, 28, 15, 22, 43))).Root!;

    // CLIENT-014: the watchdog relaunches from the package's install location while it is
    // installed, and once it is gone (re-checked after a pause) it deletes the device state and
    // unregisters itself.
    [Fact]
    public void BuildScript_RelaunchesWhileThePackageIsInstalled_AndCleansUpWhenItIsGone()
    {
        var script = RestartWatchdog.BuildScript("--restarted-by-watchdog");

        Assert.Contains(
            "Start-Process -FilePath (Join-Path $p.InstallLocation 'Virtue.WindowsApp.exe') -ArgumentList '--restarted-by-watchdog'",
            script);
        Assert.Contains("Get-AppxPackage -Name 'VirtueInitiative.VirtueAccountability'", script);
        Assert.Contains("Remove-Item -LiteralPath (Join-Path $env:ProgramData 'Virtue') -Recurse -Force", script);
        Assert.Contains("Unregister-ScheduledTask -TaskName 'VirtueResidentWatchdog'", script);
        // The package is queried again after the pause, so an update swap is not an uninstall.
        var sleep = script.IndexOf("Start-Sleep", StringComparison.Ordinal);
        Assert.True(script.IndexOf("Get-AppxPackage", sleep, StringComparison.Ordinal) > sleep);
        Assert.True(sleep < script.IndexOf("Remove-Item", StringComparison.Ordinal));
        // Nothing depends on the execution alias, which the user can switch off.
        Assert.DoesNotContain("virtue-initiative.exe", script);
    }

    // CLIENT-014: after a deliberate Exit the task only cleans up and never launches the app.
    [Fact]
    public void BuildScript_CleanupOnly_NeverStartsTheApp_ButStillCleansUp()
    {
        var script = RestartWatchdog.BuildScript("--restarted-by-watchdog", relaunch: false);

        Assert.DoesNotContain("Start-Process", script);
        Assert.Contains("Remove-Item", script);
        Assert.Contains("Unregister-ScheduledTask", script);
    }

    // CLIENT-014: the task keeps its once-a-minute cadence and runs without a console window.
    [Fact]
    public void BuildTaskXml_RepeatsEveryMinute_InAHeadlessConsole()
    {
        var root = Build();
        var interval = root.Element(Ns + "Triggers")!.Element(Ns + "TimeTrigger")!.Element(Ns + "Repetition")!.Element(Ns + "Interval")!;
        var exec = root.Element(Ns + "Actions")!.Element(Ns + "Exec")!;

        Assert.Equal("PT1M", (string)interval);
        Assert.Equal("conhost.exe", (string)exec.Element(Ns + "Command")!);
        Assert.StartsWith("--headless powershell.exe", (string)exec.Element(Ns + "Arguments")!);
        Assert.DoesNotContain("ExecutionPolicy", (string)exec.Element(Ns + "Arguments")!);
    }
}
