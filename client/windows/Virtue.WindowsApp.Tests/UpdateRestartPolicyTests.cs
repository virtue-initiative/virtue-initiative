using Virtue.WindowsApp.Core.Interop;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class UpdateRestartPolicyTests
{
    private static readonly DateTimeOffset StagedAt = new(2026, 1, 1, 0, 0, 0, TimeSpan.Zero);

    [Fact]
    public void Busy_NeverRestarts_RegardlessOfWindowOrTime()
    {
        Assert.False(UpdateRestartPolicy.ShouldRestartNow(
            mainWindowVisible: false,
            sessionIsBusy: true,
            updateStagedAtUtc: StagedAt,
            nowUtc: StagedAt + UpdateRestartPolicy.DeferralCap + TimeSpan.FromDays(1)));
    }

    [Fact]
    public void WindowHidden_AndNotBusy_RestartsImmediately()
    {
        Assert.True(UpdateRestartPolicy.ShouldRestartNow(
            mainWindowVisible: false,
            sessionIsBusy: false,
            updateStagedAtUtc: StagedAt,
            nowUtc: StagedAt));
    }

    [Fact]
    public void WindowVisible_UnderDeferralCap_DoesNotRestart()
    {
        Assert.False(UpdateRestartPolicy.ShouldRestartNow(
            mainWindowVisible: true,
            sessionIsBusy: false,
            updateStagedAtUtc: StagedAt,
            nowUtc: StagedAt + UpdateRestartPolicy.DeferralCap - TimeSpan.FromMinutes(1)));
    }

    [Fact]
    public void WindowVisible_AtOrPastDeferralCap_AndNotBusy_ForcesRestart()
    {
        Assert.True(UpdateRestartPolicy.ShouldRestartNow(
            mainWindowVisible: true,
            sessionIsBusy: false,
            updateStagedAtUtc: StagedAt,
            nowUtc: StagedAt + UpdateRestartPolicy.DeferralCap));
    }
}
