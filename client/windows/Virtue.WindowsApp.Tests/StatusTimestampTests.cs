using System.Globalization;
using Virtue.WindowsApp.Core.ViewModels;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class StatusTimestampTests
{
    private const long Second = 1000;
    private const long Minute = 60 * Second;
    private const long Hour = 60 * Minute;
    private const long Day = 24 * Hour;
    private static readonly DateTimeOffset Now = DateTimeOffset.FromUnixTimeSeconds(1_800_000_000);

    // CLIENT-007: ages read Ns, Nm, Nh Mm or Nd Nh ago, rounded down, without a
    // second unit of zero.
    [Theory]
    [InlineData(-1, "in the future")]
    [InlineData(0, "0s ago")]
    [InlineData(59 * Second + 999, "59s ago")]
    [InlineData(Minute, "1m ago")]
    [InlineData(59 * Minute + 59 * Second, "59m ago")]
    [InlineData(Hour, "1h ago")]
    [InlineData(4 * Hour + 54 * Minute, "4h 54m ago")]
    [InlineData(23 * Hour + 59 * Minute + 59 * Second, "23h 59m ago")]
    [InlineData(Day, "1d ago")]
    [InlineData(2 * Day + 3 * Hour + 30 * Minute, "2d 3h ago")]
    [InlineData(40 * Day + 10 * Hour, "40d 10h ago")]
    public void Age_ReadsTheSameOnEveryPlatform(long deltaMs, string expected)
    {
        var nowMs = Now.ToUnixTimeMilliseconds();
        Assert.Equal(expected, StatusTimestamp.Age(nowMs - deltaMs, nowMs));
    }

    // CLIENT-007: an Advanced timestamp is local time followed by the age.
    [Fact]
    public void Full_IsLocalTimeFollowedByTheAge()
    {
        var then = Now - TimeSpan.FromMinutes(5);
        var local = then.ToLocalTime().ToString("yyyy-MM-dd HH:mm:ss", CultureInfo.InvariantCulture);
        Assert.Equal($"{local} (5m ago)", StatusTimestamp.Full(then.ToUnixTimeMilliseconds(), Now));
    }

    // CLIENT-007: the summary's Last screenshot has a relative age too.
    [Fact]
    public void Short_EndsWithTheAge()
    {
        var then = Now - TimeSpan.FromMinutes(4 * 60 + 54);
        Assert.EndsWith(" (4h 54m ago)", StatusTimestamp.Short(then.ToUnixTimeMilliseconds(), Now));
    }

    // CLIENT-007: an absent timestamp shows <none>.
    [Fact]
    public void Full_ShowsNoneForAnAbsentValue()
    {
        Assert.Equal("<none>", StatusTimestamp.Full(null));
    }
}
