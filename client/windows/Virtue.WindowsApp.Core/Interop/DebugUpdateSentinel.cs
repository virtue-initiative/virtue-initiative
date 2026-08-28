using System.Globalization;

namespace Virtue.WindowsApp.Core.Interop;

/// <summary>
/// A developer-only file that simulates a staged Store update, so the whole restart flow
/// (countdown, window-closed auto-restart, "Close now and update" button) can be exercised on a
/// VM without publishing a new package to a Store flight — which is otherwise the only way to
/// reach this code at all.
///
/// Drop a file at <see cref="SentinelPath"/> and <c>StoreUpdateManager</c>'s poll slice picks
/// it up within a few seconds, deletes it, and raises the same <c>UpdateStaged</c> event a real
/// download would. Optional file contents are a short duration (<c>5m</c>, <c>90s</c>) that
/// overrides <see cref="UpdateRestartPolicy.DeferralCap"/> for that simulated update, so the
/// forced-restart path is reachable in minutes instead of six hours.
///
/// Kept free of WinRT/OS-projection dependencies (plain <c>System.IO</c>) so it's unit-testable,
/// like the rest of this folder.
/// </summary>
public static class DebugUpdateSentinel
{
    /// <summary>
    /// <c>%PROGRAMDATA%\Virtue\debug-stage-update</c> — the same directory as the app's
    /// <c>ui-startup.log</c>, which is where a developer is already looking.
    /// </summary>
    public static string SentinelPath => Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
        "Virtue",
        "debug-stage-update");

    /// <summary>
    /// Deletes the sentinel file if it exists, reporting whether it did.
    /// </summary>
    /// <param name="deferralOverride">
    /// The duration parsed from the file's contents, or <c>null</c> when the file was empty or
    /// unparseable — an unusable value never suppresses the simulated update, it just falls
    /// back to the normal deferral cap.
    /// </param>
    /// <returns><c>true</c> if the sentinel was present (and has now been consumed).</returns>
    public static bool TryConsume(string path, out TimeSpan? deferralOverride)
    {
        deferralOverride = null;

        string contents;
        try
        {
            if (!File.Exists(path))
            {
                return false;
            }

            contents = File.ReadAllText(path);
            File.Delete(path);
        }
        catch (IOException)
        {
            return false;
        }
        catch (UnauthorizedAccessException)
        {
            return false;
        }

        deferralOverride = ParseDuration(contents);
        return true;
    }

    /// <summary>Parses <c>90s</c> / <c>5m</c> / <c>2h</c>; a bare number is minutes.</summary>
    private static TimeSpan? ParseDuration(string contents)
    {
        var text = contents.Trim().ToLowerInvariant();
        if (text.Length == 0)
        {
            return null;
        }

        var unit = text[^1];
        var numberText = char.IsDigit(unit) ? text : text[..^1];
        if (!double.TryParse(numberText, NumberStyles.Float, CultureInfo.InvariantCulture, out var value) ||
            value <= 0 ||
            double.IsInfinity(value))
        {
            return null;
        }

        return unit switch
        {
            's' => TimeSpan.FromSeconds(value),
            'm' => TimeSpan.FromMinutes(value),
            'h' => TimeSpan.FromHours(value),
            _ when char.IsDigit(unit) => TimeSpan.FromMinutes(value),
            _ => null,
        };
    }
}
