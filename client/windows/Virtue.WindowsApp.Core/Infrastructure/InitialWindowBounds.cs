namespace Virtue.WindowsApp.Core.Infrastructure;

/// <summary>
/// Picks the main window's first-show position and size. <c>AppWindow</c> sizes
/// in physical pixels while the XAML content lays out in effective pixels, so a
/// fixed pixel size shrinks on every display scaled above 100%. This scales a
/// size given in effective pixels by the window's DPI, keeps it inside the
/// monitor's work area, and centers it there.
/// </summary>
public static class InitialWindowBounds
{
    /// <summary>Preferred outer size (title bar included), in effective pixels.</summary>
    public const int PreferredWidth = 760;

    public const int PreferredHeight = 640;

    /// <summary>
    /// The largest share of the work area the window may take, so there is
    /// always some desktop visible around it on a small screen.
    /// </summary>
    public const double MaxWorkAreaFraction = 0.9;

    private const double DefaultDpi = 96;

    /// <param name="dpi">The window's DPI (96 at 100% scaling). A non-positive value is treated as 96.</param>
    /// <param name="workArea">The monitor's work area in physical pixels.</param>
    /// <returns>Outer window bounds in physical pixels.</returns>
    public static PixelRect Compute(uint dpi, PixelRect workArea)
    {
        var scale = (dpi > 0 ? dpi : DefaultDpi) / DefaultDpi;
        var width = Fit(PreferredWidth * scale, workArea.Width);
        var height = Fit(PreferredHeight * scale, workArea.Height);

        return new PixelRect(
            workArea.X + ((workArea.Width - width) / 2),
            workArea.Y + ((workArea.Height - height) / 2),
            width,
            height);
    }

    private static int Fit(double preferred, int available)
    {
        var limit = (int)Math.Floor(available * MaxWorkAreaFraction);
        return Math.Max(1, Math.Min((int)Math.Round(preferred), limit));
    }
}

public readonly record struct PixelRect(int X, int Y, int Width, int Height);
