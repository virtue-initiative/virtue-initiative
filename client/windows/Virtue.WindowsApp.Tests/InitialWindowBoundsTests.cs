using Virtue.WindowsApp.Core.Infrastructure;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class InitialWindowBoundsTests
{
    [Fact]
    public void Compute_At100Percent_UsesThePreferredSizeCentered()
    {
        var bounds = InitialWindowBounds.Compute(96, new PixelRect(0, 0, 1920, 1032));

        Assert.Equal(new PixelRect(580, 196, 760, 640), bounds);
    }

    [Fact]
    public void Compute_At150Percent_ScalesToPhysicalPixels()
    {
        var bounds = InitialWindowBounds.Compute(144, new PixelRect(0, 0, 2560, 1392));

        Assert.Equal(1140, bounds.Width);
        Assert.Equal(960, bounds.Height);
    }

    [Fact]
    public void Compute_On1366x768_FitsWithRoomToSpare()
    {
        // 1366x768 at 100% with a 48px Windows 11 taskbar.
        var bounds = InitialWindowBounds.Compute(96, new PixelRect(0, 0, 1366, 720));

        Assert.Equal(760, bounds.Width);
        Assert.Equal(640, bounds.Height);
        Assert.True(bounds.Y >= 0 && bounds.Y + bounds.Height <= 720);
    }

    [Fact]
    public void Compute_WhenScaledSizeExceedsTheWorkArea_ClampsToIt()
    {
        // 1920x1080 at 150% leaves an effective work area of 1280x688.
        var bounds = InitialWindowBounds.Compute(144, new PixelRect(0, 0, 1920, 1032));

        Assert.Equal(1140, bounds.Width);
        Assert.Equal(928, bounds.Height);
        Assert.Equal(52, bounds.Y);
    }

    [Fact]
    public void Compute_OffsetsIntoASecondaryMonitorsWorkArea()
    {
        var bounds = InitialWindowBounds.Compute(96, new PixelRect(-1920, 40, 1920, 1040));

        Assert.Equal(-1920 + 580, bounds.X);
        Assert.Equal(40 + 200, bounds.Y);
    }

    [Fact]
    public void Compute_WithUnknownDpi_TreatsItAs100Percent()
    {
        var bounds = InitialWindowBounds.Compute(0, new PixelRect(0, 0, 1920, 1032));

        Assert.Equal(760, bounds.Width);
        Assert.Equal(640, bounds.Height);
    }
}
