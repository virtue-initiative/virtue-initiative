using System.Xml.Linq;
using Virtue.WindowsApp.Core.Interop;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class UpdateRelaunchTaskTests
{
    private const string AliasPath = @"C:\Users\someone\AppData\Local\Microsoft\WindowsApps\virtue-initiative.exe";
    private static readonly DateTime StartAt = new(2026, 8, 28, 15, 22, 43);
    private static readonly XNamespace Ns = "http://schemas.microsoft.com/windows/2004/02/mit/task";

    private static XElement Build(string exePath = AliasPath, string arguments = "--restarted-by-watchdog") =>
        XDocument.Parse(UpdateRelaunchTask.BuildTaskXml(exePath, arguments, StartAt, TimeSpan.FromMinutes(1))).Root!;

    [Fact]
    public void BuildTaskXml_UsesATimeTriggerWithSecondsPrecision_NotARepetitionInterval()
    {
        var trigger = Build().Element(Ns + "Triggers")!.Element(Ns + "TimeTrigger")!;

        // The seconds field is the whole point: schtasks' own /ST is HH:mm, and a Repetition
        // Interval is floored at PT1M, so neither can express a sub-minute relaunch.
        Assert.Equal("2026-08-28T15:22:43", (string)trigger.Element(Ns + "StartBoundary")!);
        Assert.Equal("2026-08-28T15:23:43", (string)trigger.Element(Ns + "EndBoundary")!);
        Assert.Null(trigger.Element(Ns + "Repetition"));
    }

    [Fact]
    public void BuildTaskXml_LaunchesTheGivenPathWithItsArguments()
    {
        var exec = Build().Element(Ns + "Actions")!.Element(Ns + "Exec")!;

        Assert.Equal(AliasPath, (string)exec.Element(Ns + "Command")!);
        Assert.Equal("--restarted-by-watchdog", (string)exec.Element(Ns + "Arguments")!);
    }

    [Fact]
    public void BuildTaskXml_OmitsUserId_SoSchtasksResolvesTheCurrentAccountItself()
    {
        var principal = Build().Element(Ns + "Principals")!.Element(Ns + "Principal")!;

        Assert.Null(principal.Element(Ns + "UserId"));
        Assert.Equal("InteractiveToken", (string)principal.Element(Ns + "LogonType")!);
        Assert.Equal("LeastPrivilege", (string)principal.Element(Ns + "RunLevel")!);
    }

    [Fact]
    public void BuildTaskXml_AsksTheSchedulerToCollectTheTaskOnceItHasExpired()
    {
        var settings = Build().Element(Ns + "Settings")!;

        Assert.Equal("PT30S", (string)settings.Element(Ns + "DeleteExpiredTaskAfter")!);
        // A missed start boundary (asleep at the wrong moment) should still relaunch, up to the
        // trigger's end boundary.
        Assert.Equal("true", (string)settings.Element(Ns + "StartWhenAvailable")!);
    }

    [Fact]
    public void BuildTaskXml_EscapesPathsSoAnAmpersandCannotCorruptTheDefinition()
    {
        var exec = Build(exePath: @"C:\Users\a & b\virtue-initiative.exe")
            .Element(Ns + "Actions")!
            .Element(Ns + "Exec")!;

        Assert.Equal(@"C:\Users\a & b\virtue-initiative.exe", (string)exec.Element(Ns + "Command")!);
    }

    [Fact]
    public void DefaultDelay_BeatsTheWatchdogPollByAWideMargin()
    {
        Assert.True(UpdateRelaunchTask.DefaultDelay < TimeSpan.FromMinutes(1));
        // Long enough for the package swap that follows the install call to finish.
        Assert.True(UpdateRelaunchTask.DefaultDelay >= TimeSpan.FromSeconds(10));
    }
}
