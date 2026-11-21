using Virtue.WindowsApp.Core.Interop;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class AppDataDirectoryTests : IDisposable
{
    private readonly string _root = Path.Combine(Path.GetTempPath(), $"virtue-appdata-{Guid.NewGuid():N}");
    private readonly List<string> _log = new();

    private string Legacy => Path.Combine(_root, "ProgramData", "Virtue");

    private string Package => Path.Combine(_root, "LocalState", "Virtue");

    public AppDataDirectoryTests()
    {
        Directory.CreateDirectory(Path.Combine(_root, "LocalState"));
    }

    public void Dispose()
    {
        if (Directory.Exists(_root))
        {
            Directory.Delete(_root, recursive: true);
        }
    }

    private void WriteLegacyState()
    {
        Directory.CreateDirectory(Path.Combine(Legacy, "config"));
        Directory.CreateDirectory(Path.Combine(Legacy, "data", "logs"));
        File.WriteAllText(Path.Combine(Legacy, "config", "ui_state.json"), "{\"email\":\"a@b.c\"}");
        File.WriteAllText(Path.Combine(Legacy, "data", "event_state.json"), "{}");
        File.WriteAllText(Path.Combine(Legacy, "data", "logs", "virtue.2026-11-20.log"), "log");
        File.WriteAllText(Path.Combine(Legacy, "ui-startup.log"), "startup");
    }

    // CLIENT-002: a fresh packaged install uses the package's folder.
    [Fact]
    public void Prepare_WithNoOldState_UsesThePackageFolder()
    {
        Assert.Equal(Package, AppDataDirectory.Prepare(Legacy, Package, _log.Add));
        Assert.False(Directory.Exists(Legacy));
    }

    // CLIENT-002: an older version's ProgramData folder is moved into the package, whole, and
    // the old folder is deleted.
    [Fact]
    public void Prepare_MovesTheOldStateIntoThePackage_AndDeletesTheOldFolder()
    {
        WriteLegacyState();

        var used = AppDataDirectory.Prepare(Legacy, Package, _log.Add);

        Assert.Equal(Package, used);
        Assert.Equal("{\"email\":\"a@b.c\"}", File.ReadAllText(Path.Combine(Package, "config", "ui_state.json")));
        Assert.Equal("{}", File.ReadAllText(Path.Combine(Package, "data", "event_state.json")));
        Assert.Equal("log", File.ReadAllText(Path.Combine(Package, "data", "logs", "virtue.2026-11-20.log")));
        Assert.Equal("startup", File.ReadAllText(Path.Combine(Package, "ui-startup.log")));
        Assert.False(Directory.Exists(Legacy));
        // No temporary folder is left beside the package folder.
        Assert.Equal(new[] { Package }, Directory.GetDirectories(Path.Combine(_root, "LocalState")));
    }

    // CLIENT-002: once the package folder exists, an old folder is never copied over it.
    [Fact]
    public void Prepare_WhenThePackageFolderExists_LeavesBothAlone()
    {
        WriteLegacyState();
        Directory.CreateDirectory(Path.Combine(Package, "config"));
        File.WriteAllText(Path.Combine(Package, "config", "ui_state.json"), "{\"email\":\"new@b.c\"}");

        Assert.Equal(Package, AppDataDirectory.Prepare(Legacy, Package, _log.Add));
        Assert.Equal("{\"email\":\"new@b.c\"}", File.ReadAllText(Path.Combine(Package, "config", "ui_state.json")));
        Assert.True(File.Exists(Path.Combine(Legacy, "data", "event_state.json")));
    }

    // CLIENT-002: when the copy fails, the launch keeps using the old folder and nothing is
    // lost or half-moved.
    [Fact]
    public void Prepare_WhenTheCopyFails_KeepsUsingTheOldFolder()
    {
        WriteLegacyState();
        // The package folder's parent is missing and can't be created: a file stands in its way.
        var blockedPackage = Path.Combine(_root, "blocked", "Virtue");
        File.WriteAllText(Path.Combine(_root, "blocked"), "not a folder");

        var used = AppDataDirectory.Prepare(Legacy, blockedPackage, _log.Add);

        Assert.Equal(Legacy, used);
        Assert.True(File.Exists(Path.Combine(Legacy, "data", "event_state.json")));
        Assert.Contains(_log, line => line.StartsWith("Could not move app data", StringComparison.Ordinal));
    }
}
