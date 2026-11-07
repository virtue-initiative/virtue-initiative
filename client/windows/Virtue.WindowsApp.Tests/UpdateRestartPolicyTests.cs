using Virtue.WindowsApp.Core.Interop;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class UpdateRestartPolicyTests
{
    private static readonly DateTimeOffset StagedAt = new(2026, 1, 1, 0, 0, 0, TimeSpan.Zero);

    // CLIENT-029: the forced install deadline is the staging time plus the deferral cap.
    [Fact]
    public void GetDeadlineUtc_IsStagedAtPlusDeferralCap()
    {
        Assert.Equal(StagedAt + UpdateRestartPolicy.DeferralCap, UpdateRestartPolicy.GetDeadlineUtc(StagedAt));
    }

    // CLIENT-029: a developer-set cap replaces the normal deferral cap.
    [Fact]
    public void GetDeadlineUtc_WithExplicitCap_UsesItInsteadOfTheDeferralCap()
    {
        Assert.Equal(
            StagedAt + TimeSpan.FromMinutes(5),
            UpdateRestartPolicy.GetDeadlineUtc(StagedAt, TimeSpan.FromMinutes(5)));
    }

    // CLIENT-029: no cap override means the normal deferral cap.
    [Fact]
    public void GetDeadlineUtc_WithNullCap_FallsBackToTheDeferralCap()
    {
        Assert.Equal(
            StagedAt + UpdateRestartPolicy.DeferralCap,
            UpdateRestartPolicy.GetDeadlineUtc(StagedAt, cap: null));
    }

    // CLIENT-029: the window is never hidden to force an install while an action is running.
    [Fact]
    public void ShouldForceRestart_Busy_NeverRestarts_RegardlessOfDeadline()
    {
        var deadline = UpdateRestartPolicy.GetDeadlineUtc(StagedAt);
        Assert.False(UpdateRestartPolicy.ShouldForceRestart(
            sessionIsBusy: true,
            deadlineUtc: deadline,
            nowUtc: deadline + TimeSpan.FromDays(1)));
    }

    // CLIENT-029: no forced install before the deadline.
    [Fact]
    public void ShouldForceRestart_BeforeDeadline_DoesNotRestart()
    {
        var deadline = UpdateRestartPolicy.GetDeadlineUtc(StagedAt);
        Assert.False(UpdateRestartPolicy.ShouldForceRestart(
            sessionIsBusy: false,
            deadlineUtc: deadline,
            nowUtc: deadline - TimeSpan.FromMinutes(1)));
    }

    // CLIENT-029: the install is forced at the deadline.
    [Fact]
    public void ShouldForceRestart_AtDeadline_AndNotBusy_ForcesRestart()
    {
        var deadline = UpdateRestartPolicy.GetDeadlineUtc(StagedAt);
        Assert.True(UpdateRestartPolicy.ShouldForceRestart(
            sessionIsBusy: false,
            deadlineUtc: deadline,
            nowUtc: deadline));
    }

    // CLIENT-029: the install is forced after the deadline.
    [Fact]
    public void ShouldForceRestart_AfterDeadline_AndNotBusy_ForcesRestart()
    {
        var deadline = UpdateRestartPolicy.GetDeadlineUtc(StagedAt);
        Assert.True(UpdateRestartPolicy.ShouldForceRestart(
            sessionIsBusy: false,
            deadlineUtc: deadline,
            nowUtc: deadline + TimeSpan.FromMinutes(1)));
    }

    // CLIENT-029: the countdown reads "<h>h <m>m" from one hour up.
    [Fact]
    public void FormatCountdown_OneHourOrMore_ShowsHoursAndMinutes()
    {
        Assert.Equal("3h 12m", UpdateRestartPolicy.FormatCountdown(TimeSpan.FromMinutes(192)));
    }

    // CLIENT-029: the countdown reads "1h 0m" at exactly one hour.
    [Fact]
    public void FormatCountdown_ExactlyOneHour_ShowsHoursAndMinutes()
    {
        Assert.Equal("1h 0m", UpdateRestartPolicy.FormatCountdown(TimeSpan.FromHours(1)));
    }

    // CLIENT-029: the countdown reads "<m>m" below one hour.
    [Fact]
    public void FormatCountdown_UnderOneHour_ShowsMinutesOnly()
    {
        Assert.Equal("42m", UpdateRestartPolicy.FormatCountdown(TimeSpan.FromMinutes(42)));
    }

    // CLIENT-029: the countdown never reads below 1m.
    [Fact]
    public void FormatCountdown_UnderOneMinute_ShowsAtLeastOneMinute()
    {
        Assert.Equal("1m", UpdateRestartPolicy.FormatCountdown(TimeSpan.FromSeconds(30)));
    }

    // CLIENT-029: the countdown reads "any moment" at or past the deadline.
    [Fact]
    public void FormatCountdown_ZeroOrNegative_ShowsAnyMoment()
    {
        Assert.Equal("any moment", UpdateRestartPolicy.FormatCountdown(TimeSpan.Zero));
        Assert.Equal("any moment", UpdateRestartPolicy.FormatCountdown(TimeSpan.FromMinutes(-5)));
    }
}
