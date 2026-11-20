using System.Xml.Linq;
using Virtue.WindowsApp.Core.Interop;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class RestartWatchdogTests
{
    private const string AliasPath = @"C:\Users\someone\AppData\Local\Microsoft\WindowsApps\virtue-initiative.exe";
    private static readonly DateTime RegisteredAt = new(2026, 8, 28, 15, 22, 43);
    private static readonly XNamespace Ns = "http://schemas.microsoft.com/windows/2004/02/mit/task";

    private static XElement Build() =>
        XDocument.Parse(RestartWatchdog.BuildTaskXml(AliasPath, "--restarted-by-watchdog", RegisteredAt)).Root!;

    // CLIENT-028: the task runs the app's own executable directly, not a script or shell.
    [Fact]
    public void BuildTaskXml_RunsTheAppDirectly_NotAScript()
    {
        var actions = Build().Element(Ns + "Actions")!.Elements(Ns + "Exec").ToList();

        var exec = Assert.Single(actions);
        Assert.Equal(AliasPath, (string)exec.Element(Ns + "Command")!);
        Assert.Equal("--restarted-by-watchdog", (string)exec.Element(Ns + "Arguments")!);
    }

    // CLIENT-028: the task repeats every minute and its trigger ends a day after registration.
    [Fact]
    public void BuildTaskXml_RepeatsEveryMinute_UntilADayAfterRegistration()
    {
        var trigger = Build().Element(Ns + "Triggers")!.Element(Ns + "TimeTrigger")!;

        Assert.Equal("PT1M", (string)trigger.Element(Ns + "Repetition")!.Element(Ns + "Interval")!);
        Assert.Equal("2026-08-28T15:22:43", (string)trigger.Element(Ns + "StartBoundary")!);
        Assert.Equal("2026-08-29T15:22:43", (string)trigger.Element(Ns + "EndBoundary")!);
    }

    // CLIENT-014, CLIENT-028: once expired, Task Scheduler deletes the task, so nothing is left
    // behind a day after an uninstall.
    [Fact]
    public void BuildTaskXml_AsksTheSchedulerToDeleteTheTaskOnceExpired()
    {
        var settings = Build().Element(Ns + "Settings")!;

        Assert.Equal("PT0S", (string)settings.Element(Ns + "DeleteExpiredTaskAfter")!);
    }

    // CLIENT-028: the app the task starts is the task's own process, so the task has no run
    // time limit that would stop it.
    [Fact]
    public void BuildTaskXml_HasNoRunTimeLimit()
    {
        var settings = Build().Element(Ns + "Settings")!;

        Assert.Equal("PT0S", (string)settings.Element(Ns + "ExecutionTimeLimit")!);
    }

    // CLIENT-028: the app renews the task well before it expires.
    [Fact]
    public void RenewInterval_IsWellInsideTheLifetime()
    {
        Assert.Equal(TimeSpan.FromDays(1), RestartWatchdog.Lifetime);
        Assert.Equal(TimeSpan.FromHours(6), RestartWatchdog.RenewInterval);
    }

    // CLIENT-028: the task runs with least privilege as the signed-in user.
    [Fact]
    public void BuildTaskXml_RunsWithLeastPrivilege_AsTheCurrentUser()
    {
        var principal = Build().Element(Ns + "Principals")!.Element(Ns + "Principal")!;

        Assert.Null(principal.Element(Ns + "UserId"));
        Assert.Equal("InteractiveToken", (string)principal.Element(Ns + "LogonType")!);
        Assert.Equal("LeastPrivilege", (string)principal.Element(Ns + "RunLevel")!);
    }

    // CLIENT-028: a path with an ampersand cannot corrupt the task definition.
    [Fact]
    public void BuildTaskXml_EscapesThePath()
    {
        const string path = @"C:\Users\R&D\AppData\Local\Microsoft\WindowsApps\virtue-initiative.exe";
        var root = XDocument.Parse(RestartWatchdog.BuildTaskXml(path, "--restarted-by-watchdog", RegisteredAt)).Root!;

        Assert.Equal(path, (string)root.Element(Ns + "Actions")!.Element(Ns + "Exec")!.Element(Ns + "Command")!);
    }
}
