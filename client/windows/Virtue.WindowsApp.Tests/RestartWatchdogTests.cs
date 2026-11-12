using System.Xml.Linq;
using Virtue.WindowsApp.Core.Interop;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class RestartWatchdogTests
{
    private const string AliasPath = @"C:\Users\someone\AppData\Local\Microsoft\WindowsApps\virtue-initiative.exe";
    private static readonly XNamespace Ns = "http://schemas.microsoft.com/windows/2004/02/mit/task";

    private static XElement Build() =>
        XDocument.Parse(RestartWatchdog.BuildTaskXml(AliasPath, "--restarted-by-watchdog", new DateTime(2026, 8, 28, 15, 22, 43))).Root!;

    // CLIENT-014: while the alias exists the watchdog relaunches the app, and once it is gone
    // it deletes the device state and unregisters itself.
    [Fact]
    public void BuildScript_RelaunchesWhileTheAliasExists_AndCleansUpWhenItIsGone()
    {
        var script = RestartWatchdog.BuildScript(AliasPath, "--restarted-by-watchdog");

        Assert.Contains($"$a='{AliasPath}'", script);
        Assert.Contains("Start-Process -FilePath $a -ArgumentList '--restarted-by-watchdog'", script);
        Assert.Contains("Remove-Item -LiteralPath (Join-Path $env:ProgramData 'Virtue') -Recurse -Force", script);
        Assert.Contains("Unregister-ScheduledTask -TaskName 'VirtueResidentWatchdog'", script);
        // The cleanup branch re-checks the alias after a pause so an update swap is not mistaken
        // for an uninstall.
        Assert.True(script.IndexOf("Start-Sleep", StringComparison.Ordinal) < script.IndexOf("Remove-Item", StringComparison.Ordinal));
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
    }
}
