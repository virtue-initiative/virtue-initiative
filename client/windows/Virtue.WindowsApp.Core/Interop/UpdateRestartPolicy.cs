namespace Virtue.WindowsApp.Core.Interop;

/// <summary>
/// Pure decision logic for when a staged Store update is safe to install. Kept free of
/// WinRT/OS dependencies so it's unit-testable, unlike <see cref="StoreUpdateManager"/> and
/// <see cref="RestartWatchdog"/> which are thin OS glue.
///
/// The daemon itself has no data-corruption window to protect against here — see
/// <see cref="StoreUpdateManager"/>'s doc comment for why. What this policy actually guards
/// is UX: don't restart out from under an actively-interacting user or mid-login. A deferral
/// cap forces the restart through even if the window happens to stay open indefinitely.
///
/// The "window hidden -> restart immediately" half of the old decision now lives directly in
/// <c>App.EvaluateUpdateRestart</c> (it's a state/event check, not really "policy"). What
/// remains here is: computing the forced-restart deadline, deciding whether that deadline has
/// been reached, and formatting the countdown shown in the in-window notice.
/// </summary>
public static class UpdateRestartPolicy
{
    public static readonly TimeSpan DeferralCap = TimeSpan.FromHours(6);

    /// <param name="updateStagedAtUtc">When the update finished downloading/staging.</param>
    /// <param name="cap">
    /// Overrides <see cref="DeferralCap"/> — only ever supplied by the debug sentinel
    /// (<see cref="DebugUpdateSentinel"/>), which needs a deadline reachable in minutes.
    /// </param>
    public static DateTimeOffset GetDeadlineUtc(DateTimeOffset updateStagedAtUtc, TimeSpan? cap = null) =>
        updateStagedAtUtc + (cap ?? DeferralCap);

    /// <param name="sessionIsBusy">Whether <c>SessionViewModel.IsBusy</c> is set (e.g. a login/logout in progress).</param>
    /// <param name="deadlineUtc">The forced-restart deadline, from <see cref="GetDeadlineUtc"/>.</param>
    /// <param name="nowUtc">Current time, passed in for testability.</param>
    public static bool ShouldForceRestart(bool sessionIsBusy, DateTimeOffset deadlineUtc, DateTimeOffset nowUtc)
    {
        if (sessionIsBusy)
        {
            return false;
        }

        return nowUtc >= deadlineUtc;
    }

    /// <summary>
    /// Formats a duration remaining before the forced restart, for the in-window notice.
    /// </summary>
    public static string FormatCountdown(TimeSpan remaining)
    {
        if (remaining <= TimeSpan.Zero)
        {
            return "any moment";
        }

        if (remaining >= TimeSpan.FromHours(1))
        {
            return $"{(int)remaining.TotalHours}h {remaining.Minutes}m";
        }

        var minutes = Math.Max(1, remaining.Minutes);
        return $"{minutes}m";
    }
}
