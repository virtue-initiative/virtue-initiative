using Virtue.WindowsApp.Core.Interop;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class DebugUpdateSentinelTests : IDisposable
{
    private readonly string _directory = Path.Combine(
        Path.GetTempPath(),
        $"virtue-sentinel-tests-{Guid.NewGuid():N}");

    private string WriteSentinel(string contents)
    {
        Directory.CreateDirectory(_directory);
        var path = Path.Combine(_directory, "debug-stage-update");
        File.WriteAllText(path, contents);
        return path;
    }

    public void Dispose()
    {
        if (Directory.Exists(_directory))
        {
            Directory.Delete(_directory, recursive: true);
        }
    }

    [Fact]
    public void TryConsume_MissingFile_ReturnsFalse()
    {
        var path = Path.Combine(_directory, "debug-stage-update");

        Assert.False(DebugUpdateSentinel.TryConsume(path, out var deferralOverride));
        Assert.Null(deferralOverride);
    }

    [Fact]
    public void TryConsume_DeletesTheFile_SoTheUpdateStagesOnlyOnce()
    {
        var path = WriteSentinel(string.Empty);

        Assert.True(DebugUpdateSentinel.TryConsume(path, out _));
        Assert.False(File.Exists(path));
        Assert.False(DebugUpdateSentinel.TryConsume(path, out _));
    }

    [Theory]
    [InlineData("5m", 300)]
    [InlineData("90s", 90)]
    [InlineData("2h", 7200)]
    [InlineData(" 5m \n", 300)]
    [InlineData("5M", 300)]
    [InlineData("3", 180)]
    public void TryConsume_ParsesDurationContents(string contents, int expectedSeconds)
    {
        var path = WriteSentinel(contents);

        Assert.True(DebugUpdateSentinel.TryConsume(path, out var deferralOverride));
        Assert.Equal(TimeSpan.FromSeconds(expectedSeconds), deferralOverride);
    }

    [Theory]
    [InlineData("")]
    [InlineData("   ")]
    [InlineData("soon")]
    [InlineData("-5m")]
    [InlineData("0m")]
    [InlineData("5d")]
    public void TryConsume_UnusableContents_StillStagesButWithoutAnOverride(string contents)
    {
        var path = WriteSentinel(contents);

        Assert.True(DebugUpdateSentinel.TryConsume(path, out var deferralOverride));
        Assert.Null(deferralOverride);
    }
}
