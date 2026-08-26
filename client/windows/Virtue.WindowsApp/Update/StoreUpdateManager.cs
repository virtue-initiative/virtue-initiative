using Virtue.WindowsApp.Core.Interop;
using Windows.Services.Store;

namespace Virtue.WindowsApp.Update;

/// <summary>
/// Detects, downloads, and installs Microsoft Store package updates for this MSIX app.
///
/// This lives in <c>Virtue.WindowsApp</c> (not <c>Virtue.WindowsApp.Core</c>, which targets
/// plain <c>net8.0</c> with no Windows SDK/WinRT projection so it stays unit-testable and
/// platform-neutral — see how <c>RestartWatchdog</c> avoids WinRT entirely for the same
/// reason) because <see cref="StoreContext"/> is a WinRT API only usable from the packaged
/// WinUI process.
///
/// <para>
/// <b>Owner window.</b> In a packaged <i>desktop</i> app (as opposed to a UWP one) the
/// <see cref="StoreContext"/> returned by <c>GetDefault()</c> must be associated with an owner
/// HWND via the <c>IInitializeWithWindow</c> interop before any Store call that <i>can</i> show
/// UI. Without it, <c>RequestDownloadStorePackageUpdatesAsync</c> and
/// <c>RequestDownloadAndInstallStorePackageUpdatesAsync</c> fail with
/// <c>ERROR_INVALID_WINDOW_HANDLE</c> (0x80070578) — which is exactly what shipped builds did,
/// so auto-update never worked. The app normally runs resident with no <c>MainWindow</c> at
/// all, so the owner passed in is the tray host's hidden top-level window
/// (<c>ITrayIconHost.WindowHandle</c>): the one HWND guaranteed to exist for the whole process
/// lifetime.
/// </para>
///
/// Detect and download are deliberately kept separate from install:
///  - <see cref="CheckOnceAsync"/> only calls <c>GetAppAndOptionalStorePackageUpdatesAsync</c>
///    and <c>RequestDownloadStorePackageUpdatesAsync</c>, both of which run in the background
///    without requiring the app to stop — see the task-level design notes captured in
///    the implementation plan for this feature.
///  - <see cref="TryInstallStagedUpdateAsync"/> (which calls
///    <c>RequestDownloadAndInstallStorePackageUpdatesAsync</c> — a fast install-only pass
///    since the update was already staged by the download step) is only ever invoked by
///    <c>App.xaml.cs</c> once it has stopped the resident monitoring daemon at a safe point.
///
/// No changes were made to `client/core`'s tamper-detection budget (CORE-002) for this: an
/// update-triggered exit is deliberately treated the same as a crash by the daemon (it is
/// NOT excused via `note_user_stop`, since only an actual user-initiated stop may use that
/// path), and relies on `RestartWatchdog`'s existing per-minute relaunch to bring the app
/// back — the same worst-case gap crash recovery already produces, which is what the
/// CORE-002 single-late threshold was already sized for. See `client/windows/README.md`.
/// </summary>
public sealed class StoreUpdateManager : IDisposable
{
    private readonly StoreContext _storeContext;
    private readonly bool _ownerInitialized;
    /// Owner-window failure recorded at construction time, when no <see cref="Log"/> handler is
    /// subscribed yet; flushed on the first check instead of being dropped.
    private string? _pendingOwnerWindowError;
    private CancellationTokenSource? _loopCancellation;

    /// <param name="ownerWindow">
    /// HWND to own any Store-shown UI — see the owner-window note on the class. Pass
    /// <see cref="IntPtr.Zero"/> only when no window exists; the download/install calls are
    /// then expected to fail with 0x80070578.
    /// </param>
    public StoreUpdateManager(IntPtr ownerWindow)
    {
        _storeContext = StoreContext.GetDefault();

        // Never throw out of here: this runs during OnLaunched, where a throw takes down
        // startup entirely.
        try
        {
            if (ownerWindow != IntPtr.Zero)
            {
                WinRT.Interop.InitializeWithWindow.Initialize(_storeContext, ownerWindow);
                _ownerInitialized = true;
            }
        }
        catch (Exception ex)
        {
            _ownerInitialized = false;
            _pendingOwnerWindowError = $"Store context owner-window initialization failed: {Describe(ex)}";
        }
    }

    /// <summary>Raised once an update has finished downloading/staging and is ready to install.</summary>
    public event EventHandler? UpdateStaged;

    /// <summary>Raised (log-only) when a check or download attempt fails; the loop retries on its own schedule.</summary>
    public event EventHandler<string>? UpdateCheckFailed;

    /// <summary>
    /// Raised (log-only) for ordinary check-lifecycle progress. `ui-startup.log` is the only
    /// observability channel on a Store install, so the whole lifecycle is logged — including
    /// the "no updates available" case, whose previous silence made it impossible to tell a
    /// check that found nothing from a check that never ran.
    /// </summary>
    public event EventHandler<string>? Log;

    public bool IsUpdateStaged { get; private set; }

    /// <summary>
    /// Starts the periodic background check. Safe to call once per process lifetime.
    /// </summary>
    public void Start()
    {
        if (_loopCancellation is not null)
        {
            return;
        }

        _loopCancellation = new CancellationTokenSource();
        var token = _loopCancellation.Token;
        _ = Task.Run(async () =>
        {
            var consecutiveFailures = 0;
            try
            {
                while (!token.IsCancellationRequested)
                {
                    // Nothing to check for while an update is already staged and waiting for a
                    // safe restart. If the install later fails, App clears the flag and this
                    // loop picks the check back up on its next tick.
                    if (!IsUpdateStaged)
                    {
                        var succeeded = await CheckOnceAsync().ConfigureAwait(false);
                        consecutiveFailures = succeeded ? 0 : consecutiveFailures + 1;
                    }

                    var delay = StoreUpdateRetryPolicy.GetNextDelay(consecutiveFailures);
                    await Task.Delay(delay, token).ConfigureAwait(false);
                }
            }
            catch (OperationCanceledException)
            {
                // Disposed — normal shutdown.
            }
        }, token);
    }

    /// <returns><c>true</c> if the check completed without error (whether or not it found an update).</returns>
    private async Task<bool> CheckOnceAsync()
    {
        if (_pendingOwnerWindowError is { } ownerWindowError)
        {
            _pendingOwnerWindowError = null;
            UpdateCheckFailed?.Invoke(this, ownerWindowError);
        }

        try
        {
            Log?.Invoke(this, $"Store update check starting (owner window initialized={_ownerInitialized}).");
            var updates = await _storeContext.GetAppAndOptionalStorePackageUpdatesAsync();
            Log?.Invoke(this, $"Store update check found {updates.Count} update(s).");
            if (updates.Count == 0)
            {
                return true;
            }

            Log?.Invoke(this, "Store update download started.");
            var downloadOperation = _storeContext.RequestDownloadStorePackageUpdatesAsync(updates);
            var downloadResult = await downloadOperation;
            if (downloadResult.OverallState == StorePackageUpdateState.Completed)
            {
                Log?.Invoke(this, $"Store update download finished (OverallState={downloadResult.OverallState}).");
                IsUpdateStaged = true;
                UpdateStaged?.Invoke(this, EventArgs.Empty);
                return true;
            }

            UpdateCheckFailed?.Invoke(this, $"update download ended in state {downloadResult.OverallState}");
            return false;
        }
        catch (Exception ex)
        {
            UpdateCheckFailed?.Invoke(this, Describe(ex));
            return false;
        }
    }

    /// <summary>
    /// Installs the update staged by a prior <see cref="CheckOnceAsync"/> call. Only ever
    /// called once the caller has confirmed this is a safe point (daemon stopped). Never
    /// throws — failures are reported via <see cref="UpdateCheckFailed"/> and returned as
    /// <c>false</c>, which the caller uses to decide whether to exit for the install or
    /// resume monitoring and let the retry loop try again.
    /// </summary>
    public async Task<bool> TryInstallStagedUpdateAsync()
    {
        if (!IsUpdateStaged)
        {
            return false;
        }

        try
        {
            // Re-fetch rather than trusting the cached flag, in case the Store retracted the
            // update between staging and now.
            var updates = await _storeContext.GetAppAndOptionalStorePackageUpdatesAsync();
            Log?.Invoke(this, $"Store update install re-fetch found {updates.Count} update(s).");
            if (updates.Count == 0)
            {
                IsUpdateStaged = false;
                return false;
            }

            var installOperation = _storeContext.RequestDownloadAndInstallStorePackageUpdatesAsync(updates);
            var installResult = await installOperation;
            Log?.Invoke(this, $"Store update install finished (OverallState={installResult.OverallState}).");
            IsUpdateStaged = false;
            return installResult.OverallState == StorePackageUpdateState.Completed;
        }
        catch (Exception ex)
        {
            UpdateCheckFailed?.Invoke(this, Describe(ex));
            IsUpdateStaged = false;
            return false;
        }
    }

    /// <summary>
    /// Includes the HRESULT, which is the part that actually identifies a Store failure
    /// (0x80070578 = <c>ERROR_INVALID_WINDOW_HANDLE</c>); the message text alone is ambiguous.
    /// </summary>
    private static string Describe(Exception ex) =>
        $"{ex.GetType().Name}: {ex.Message} (0x{ex.HResult:X8})";

    public void Dispose()
    {
        _loopCancellation?.Cancel();
        _loopCancellation?.Dispose();
        _loopCancellation = null;
    }
}
