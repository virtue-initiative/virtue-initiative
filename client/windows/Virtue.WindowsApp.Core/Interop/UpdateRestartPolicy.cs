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
/// </summary>
public static class UpdateRestartPolicy
{
    public static readonly TimeSpan DeferralCap = TimeSpan.FromHours(6);

    /// <param name="mainWindowVisible">Whether the settings/login window is currently shown to the user.</param>
    /// <param name="sessionIsBusy">Whether <c>SessionViewModel.IsBusy</c> is set (e.g. a login/logout in progress).</param>
    /// <param name="updateStagedAtUtc">When the update finished downloading/staging.</param>
    /// <param name="nowUtc">Current time, passed in for testability.</param>
    public static bool ShouldRestartNow(
        bool mainWindowVisible,
        bool sessionIsBusy,
        DateTimeOffset updateStagedAtUtc,
        DateTimeOffset nowUtc)
    {
        if (sessionIsBusy)
        {
            return false;
        }

        if (!mainWindowVisible)
        {
            return true;
        }

        return nowUtc - updateStagedAtUtc >= DeferralCap;
    }
}
