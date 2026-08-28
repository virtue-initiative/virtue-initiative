using Virtue.WindowsApp.Core.Interop;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class AppLaunchPathTests
{
    private const string AliasPath = @"C:\Users\someone\AppData\Local\Microsoft\WindowsApps\virtue-initiative.exe";
    private const string ProcessPath = @"C:\Program Files\WindowsApps\Virtue_0.1.1.0_x64__abc\Virtue.WindowsApp.exe";

    [Fact]
    public void Resolve_PrefersTheExecutionAlias_SoTheWatchdogSurvivesAPackageUpdate()
    {
        Assert.Equal(AliasPath, AppLaunchPath.Resolve(AliasPath, ProcessPath, path => path == AliasPath));
    }

    [Fact]
    public void Resolve_FallsBackToTheProcessPath_WhenNoAliasStubIsDeployed()
    {
        Assert.Equal(ProcessPath, AppLaunchPath.Resolve(AliasPath, ProcessPath, _ => false));
    }

    [Fact]
    public void Resolve_FallsBackToTheProcessPath_WhenNoAliasPathIsKnown()
    {
        Assert.Equal(ProcessPath, AppLaunchPath.Resolve(null, ProcessPath, _ => true));
    }

    [Fact]
    public void Resolve_ReturnsNull_WhenNeitherIsAvailable()
    {
        Assert.Null(AppLaunchPath.Resolve(null, null, _ => false));
        Assert.Null(AppLaunchPath.Resolve("   ", "  ", _ => true));
    }

    [Fact]
    public void ExecutionAliasPath_UsesTheAliasDeclaredInTheManifest()
    {
        Assert.EndsWith(
            Path.Combine("Microsoft", "WindowsApps", AppLaunchPath.ExecutionAliasFileName),
            AppLaunchPath.ExecutionAliasPath);
    }
}
