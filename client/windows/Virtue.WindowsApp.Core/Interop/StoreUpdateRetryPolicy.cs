namespace Virtue.WindowsApp.Core.Interop;

/// <summary>
/// How long <c>StoreUpdateManager</c> waits before its next Store check. Kept free of
/// WinRT/OS dependencies so it's unit-testable, for the same reason as
/// <see cref="UpdateRestartPolicy"/>.
///
/// A plain fixed interval meant a single failed check (for example the
/// <c>ERROR_INVALID_WINDOW_HANDLE</c> the missing <c>StoreContext</c> owner window used to
/// produce) left the app un-updated for a full <see cref="SuccessInterval"/>. Failures instead
/// back off from <see cref="FirstRetry"/>, doubling up to <see cref="MaxRetry"/>.
/// </summary>
public static class StoreUpdateRetryPolicy
{
    public static readonly TimeSpan SuccessInterval = TimeSpan.FromHours(4);
    public static readonly TimeSpan FirstRetry = TimeSpan.FromMinutes(5);
    public static readonly TimeSpan MaxRetry = TimeSpan.FromHours(1);

    /// <summary>
    /// 0 -&gt; <see cref="SuccessInterval"/>; 1 -&gt; <see cref="FirstRetry"/>; each further
    /// consecutive failure doubles the previous retry, capped at <see cref="MaxRetry"/>.
    /// Non-positive inputs are treated as "no failures".
    /// </summary>
    public static TimeSpan GetNextDelay(int consecutiveFailures)
    {
        if (consecutiveFailures <= 0)
        {
            return SuccessInterval;
        }

        // Compute the doubling in ticks, but stop early so a large failure count can't
        // overflow — the result is clamped to MaxRetry anyway.
        var delay = FirstRetry;
        for (var i = 1; i < consecutiveFailures && delay < MaxRetry; i++)
        {
            delay += delay;
        }

        return delay > MaxRetry ? MaxRetry : delay;
    }
}
