using System.Globalization;

namespace Virtue.WindowsApp.Core.ViewModels;

/// <summary>
/// How Status Details writes a timestamp (CLIENT-007): local time, then a
/// relative age in parentheses. The age wording is the same on every platform.
/// </summary>
public static class StatusTimestamp
{
    /// <summary>
    /// <c>Ns ago</c>, <c>Nm ago</c>, <c>Nh Mm ago</c> or <c>Nd Nh ago</c>, rounded
    /// down, without a second unit of zero, or <c>in the future</c>.
    /// </summary>
    public static string Age(long unixMs, long nowUnixMs)
    {
        var deltaMs = nowUnixMs - unixMs;
        if (deltaMs < 0)
        {
            return "in the future";
        }

        var secs = deltaMs / 1000;
        if (secs < 60)
        {
            return $"{secs}s ago";
        }

        if (secs < 3600)
        {
            return $"{secs / 60}m ago";
        }

        var (major, minor, minorUnit) = secs < 86_400
            ? ($"{secs / 3600}h", secs % 3600 / 60, "m")
            : ($"{secs / 86_400}d", secs % 86_400 / 3600, "h");
        return minor == 0 ? $"{major} ago" : $"{major} {minor}{minorUnit} ago";
    }

    /// <summary><c>2026-11-11 19:32:05 (5m ago)</c>, or <c>&lt;none&gt;</c> for an absent value.</summary>
    public static string Full(long? unixMs, DateTimeOffset? now = null)
    {
        if (unixMs is not long value)
        {
            return "<none>";
        }

        var local = DateTimeOffset.FromUnixTimeMilliseconds(value).ToLocalTime();
        var stamp = local.ToString("yyyy-MM-dd HH:mm:ss", CultureInfo.InvariantCulture);
        return $"{stamp} ({Age(value, NowMs(now))})";
    }

    /// <summary>
    /// For a summary row: <c>7:32 PM (5m ago)</c>, with the date in front when it
    /// isn't today.
    /// </summary>
    public static string Short(long unixMs, DateTimeOffset? now = null)
    {
        var current = (now ?? DateTimeOffset.Now).ToLocalTime();
        var local = DateTimeOffset.FromUnixTimeMilliseconds(unixMs).ToLocalTime();
        var time = local.ToString("t", CultureInfo.CurrentCulture);
        var stamp = local.Date == current.Date
            ? time
            : $"{local.ToString("M", CultureInfo.CurrentCulture)}, {time}";
        return $"{stamp} ({Age(unixMs, NowMs(now))})";
    }

    private static long NowMs(DateTimeOffset? now) =>
        (now ?? DateTimeOffset.UtcNow).ToUnixTimeMilliseconds();
}
