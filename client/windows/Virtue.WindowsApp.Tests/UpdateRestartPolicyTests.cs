using Virtue.WindowsApp.Core.Interop;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class UpdateRestartPolicyTests
{
    private static readonly DateTimeOffset StagedAt = new(2026, 1, 1, 0, 0, 0, TimeSpan.Zero);

    [Fact]
    public void GetDeadlineUtc_IsStagedAtPlusDeferralCap()
    {
        Assert.Equal(StagedAt + UpdateRestartPolicy.DeferralCap, UpdateRestartPolicy.GetDeadlineUtc(StagedAt));
    }

    [Fact]
    public void ShouldForceRestart_Busy_NeverRestarts_RegardlessOfDeadline()
    {
        var deadline = UpdateRestartPolicy.GetDeadlineUtc(StagedAt);
        Assert.False(UpdateRestartPolicy.ShouldForceRestart(
            sessionIsBusy: true,
            deadlineUtc: deadline,
            nowUtc: deadline + TimeSpan.FromDays(1)));
    }

    [Fact]
    public void ShouldForceRestart_BeforeDeadline_DoesNotRestart()
    {
        var deadline = UpdateRestartPolicy.GetDeadlineUtc(StagedAt);
        Assert.False(UpdateRestartPolicy.ShouldForceRestart(
            sessionIsBusy: false,
            deadlineUtc: deadline,
            nowUtc: deadline - TimeSpan.FromMinutes(1)));
    }

    [Fact]
    public void ShouldForceRestart_AtDeadline_AndNotBusy_ForcesRestart()
    {
        var deadline = UpdateRestartPolicy.GetDeadlineUtc(StagedAt);
        Assert.True(UpdateRestartPolicy.ShouldForceRestart(
            sessionIsBusy: false,
            deadlineUtc: deadline,
            nowUtc: deadline));
    }

    [Fact]
    public void ShouldForceRestart_AfterDeadline_AndNotBusy_ForcesRestart()
    {
        var deadline = UpdateRestartPolicy.GetDeadlineUtc(StagedAt);
        Assert.True(UpdateRestartPolicy.ShouldForceRestart(
            sessionIsBusy: false,
            deadlineUtc: deadline,
            nowUtc: deadline + TimeSpan.FromMinutes(1)));
    }

    [Fact]
    public void FormatCountdown_OneHourOrMore_ShowsHoursAndMinutes()
    {
        Assert.Equal("3h 12m", UpdateRestartPolicy.FormatCountdown(TimeSpan.FromMinutes(192)));
    }

    [Fact]
    public void FormatCountdown_ExactlyOneHour_ShowsHoursAndMinutes()
    {
        Assert.Equal("1h 0m", UpdateRestartPolicy.FormatCountdown(TimeSpan.FromHours(1)));
    }

    [Fact]
    public void FormatCountdown_UnderOneHour_ShowsMinutesOnly()
    {
        Assert.Equal("42m", UpdateRestartPolicy.FormatCountdown(TimeSpan.FromMinutes(42)));
    }

    [Fact]
    public void FormatCountdown_UnderOneMinute_ShowsAtLeastOneMinute()
    {
        Assert.Equal("1m", UpdateRestartPolicy.FormatCountdown(TimeSpan.FromSeconds(30)));
    }

    [Fact]
    public void FormatCountdown_ZeroOrNegative_ShowsAnyMoment()
    {
        Assert.Equal("any moment", UpdateRestartPolicy.FormatCountdown(TimeSpan.Zero));
        Assert.Equal("any moment", UpdateRestartPolicy.FormatCountdown(TimeSpan.FromMinutes(-5)));
    }
}
