using Virtue.WindowsApp.Core.Interop;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class RelaunchWindowFlagTests : IDisposable
{
    private readonly string _directory = Path.Combine(Path.GetTempPath(), Path.GetRandomFileName());

    private string FlagPath => Path.Combine(_directory, "show-window-on-next-launch");

    [Fact]
    public void TryConsume_WithNoFlag_ReturnsFalse()
    {
        Assert.False(RelaunchWindowFlag.TryConsume(FlagPath));
    }

    [Fact]
    public void TryConsume_AfterSet_ReturnsTrueExactlyOnce()
    {
        RelaunchWindowFlag.Set(FlagPath);

        Assert.True(RelaunchWindowFlag.TryConsume(FlagPath));
        Assert.False(RelaunchWindowFlag.TryConsume(FlagPath));
    }

    [Fact]
    public void Set_CreatesTheParentDirectory()
    {
        RelaunchWindowFlag.Set(FlagPath);

        Assert.True(File.Exists(FlagPath));
    }

    public void Dispose()
    {
        if (Directory.Exists(_directory))
        {
            Directory.Delete(_directory, recursive: true);
        }
    }
}
