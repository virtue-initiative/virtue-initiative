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
/// <b>Silent first.</b> Both <c>RequestDownloadStorePackageUpdatesAsync</c> and
/// <c>RequestDownloadAndInstallStorePackageUpdatesAsync</c> show an OS consent dialog — the
/// install one <i>always</i> does. That dialog is what made auto-update look dead: it is owned
/// by whichever HWND the context was initialized with, which in resident mode is the tray
/// host's hidden window, so the user saw nothing happen. The <c>TrySilent*</c> pair does the
/// same work with no UI whenever <see cref="StoreContext.CanSilentlyDownloadStorePackageUpdates"/>
/// is set (it is, unless the user turned off the Store's "Update apps automatically"), so those
/// are preferred and the <c>Request*</c> ones are only a fallback. The fallback install is
/// gated behind an explicit <c>allowInteractive</c> flag so <c>App</c> can put a real window on
/// screen first — see <see cref="TryInstallStagedUpdateAsync"/>.
/// </para>
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
/// The monitoring daemon is deliberately <b>never</b> stopped for an update — the OS terminates
/// the process for the package swap, which is the same shape as a crash-recovery relaunch that
/// <c>RestartWatchdog</c> and CORE-002's late-wakeup budget already cover, and `DaemonState` is
/// persisted every tick so there is nothing to truncate. macOS's Sparkle updater made the same
/// call (`client/mac/app/Sources/UpdateController.swift`). See `client/windows/README.md`.
/// </summary>
public sealed class StoreUpdateManager : IDisposable
{
    /// <summary>
    /// How long the loop sleeps between slices. Short so the debug sentinel is picked up
    /// promptly and <see cref="RequestCheckNow"/> feels instant; the real Store check still
    /// only runs on <see cref="StoreUpdateRetryPolicy"/>'s cadence.
    /// </summary>
    private static readonly TimeSpan PollInterval = TimeSpan.FromSeconds(5);

    private readonly StoreContext _storeContext;
    private readonly bool _ownerInitialized;
    /// Owner-window failure recorded at construction time, when no <see cref="Log"/> handler is
    /// subscribed yet; flushed on the first check instead of being dropped.
    private string? _pendingOwnerWindowError;
    private CancellationTokenSource? _loopCancellation;
    /// Released by <see cref="RequestCheckNow"/> to cut a poll slice short.
    private readonly SemaphoreSlim _checkNowSignal = new(0);
    /// Set once the debug sentinel has staged a fake update; makes the install a no-op exit.
    private bool _simulatedUpdate;

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
    /// Raised after every completed check, with whether it found an update. Drives the manual
    /// "Check for updates" button's status line; a found update reports itself through the
    /// Update Ready card instead.
    /// </summary>
    public event EventHandler<bool>? CheckCompleted;

    /// <summary>
    /// Raised (log-only) for ordinary check-lifecycle progress. `ui-startup.log` is the only
    /// observability channel on a Store install, so the whole lifecycle is logged — including
    /// the "no updates available" case, whose previous silence made it impossible to tell a
    /// check that found nothing from a check that never ran.
    /// </summary>
    public event EventHandler<string>? Log;

    public bool IsUpdateStaged { get; private set; }

    /// <summary>
    /// Overrides <see cref="UpdateRestartPolicy.DeferralCap"/> for the currently staged update.
    /// Only ever set by the debug sentinel; <c>null</c> for a real Store update.
    /// </summary>
    public TimeSpan? DeferralOverride { get; private set; }

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
            var nextCheckAtUtc = DateTimeOffset.UtcNow;
            try
            {
                while (!token.IsCancellationRequested)
                {
                    // Nothing to check for while an update is already staged and waiting for a
                    // safe restart. If the install later fails, App clears the flag and this
                    // loop picks the check back up on its next slice.
                    if (!IsUpdateStaged && DateTimeOffset.UtcNow >= nextCheckAtUtc)
                    {
                        var succeeded = await CheckOnceAsync().ConfigureAwait(false);
                        consecutiveFailures = succeeded ? 0 : consecutiveFailures + 1;
                        nextCheckAtUtc = DateTimeOffset.UtcNow + StoreUpdateRetryPolicy.GetNextDelay(consecutiveFailures);
                    }

                    if (!IsUpdateStaged)
                    {
                        StageSimulatedUpdateIfRequested();
                    }

                    // Waking on a short slice rather than one long delay keeps every path that
                    // stages an update (real check, debug sentinel, manual re-check) on this
                    // one thread, and lets RequestCheckNow short-circuit the cadence.
                    if (await _checkNowSignal.WaitAsync(PollInterval, token).ConfigureAwait(false))
                    {
                        nextCheckAtUtc = DateTimeOffset.UtcNow;
                    }
                }
            }
            catch (OperationCanceledException)
            {
                // Disposed — normal shutdown.
            }
        }, token);
    }

    /// <summary>
    /// Runs the next Store check immediately instead of on <see cref="StoreUpdateRetryPolicy"/>'s
    /// cadence. That is its only effect — the check itself, and everything downstream of it, is
    /// unchanged. Backs the main window's "Check for updates" button.
    /// </summary>
    public void RequestCheckNow()
    {
        if (_checkNowSignal.CurrentCount == 0)
        {
            _checkNowSignal.Release();
        }
    }

    /// <summary>
    /// Developer-only path: a sentinel file stages a fake update so the countdown, the
    /// window-closed auto-restart and the "Close now and update" button can all be exercised
    /// without a Store flight. See <see cref="DebugUpdateSentinel"/>.
    /// </summary>
    private void StageSimulatedUpdateIfRequested()
    {
        if (!DebugUpdateSentinel.TryConsume(DebugUpdateSentinel.SentinelPath, out var deferralOverride))
        {
            return;
        }

        _simulatedUpdate = true;
        DeferralOverride = deferralOverride;
        IsUpdateStaged = true;
        Log?.Invoke(this, $"Debug sentinel staged a simulated update (deferral override={deferralOverride?.ToString() ?? "none"}).");
        UpdateStaged?.Invoke(this, EventArgs.Empty);
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
            CheckCompleted?.Invoke(this, updates.Count > 0);
            if (updates.Count == 0)
            {
                return true;
            }

            // Silent when the Store allows it (the normal case), so no consent dialog appears
            // for a background download — see the class note.
            StorePackageUpdateResult downloadResult;
            if (_storeContext.CanSilentlyDownloadStorePackageUpdates)
            {
                Log?.Invoke(this, "Store update silent download started.");
                downloadResult = await _storeContext.TrySilentDownloadStorePackageUpdatesAsync(updates);
            }
            else
            {
                Log?.Invoke(this, "Store update download started (silent download unavailable).");
                downloadResult = await _storeContext.RequestDownloadStorePackageUpdatesAsync(updates);
            }

            if (downloadResult.OverallState == StorePackageUpdateState.Completed)
            {
                Log?.Invoke(this, $"Store update download finished (OverallState={downloadResult.OverallState}).");
                IsUpdateStaged = true;
                DeferralOverride = null;
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
    /// Installs the update staged by a prior <see cref="CheckOnceAsync"/> call. Never throws —
    /// failures are reported via <see cref="UpdateCheckFailed"/> and returned as
    /// <see cref="UpdateInstallOutcome.Failed"/>.
    /// </summary>
    /// <param name="allowInteractive">
    /// Whether the caller is willing for the OS to show its consent dialog. Only pass
    /// <c>true</c> with the main window on screen: the dialog is owned by the context's owner
    /// HWND (the hidden tray window in resident mode), so an unowned one is invisible and the
    /// install silently stalls waiting on it. When silent install is unavailable and this is
    /// <c>false</c>, nothing is attempted and
    /// <see cref="UpdateInstallOutcome.NeedsUserInteraction"/> is returned with the update left
    /// staged.
    /// </param>
    public async Task<UpdateInstallOutcome> TryInstallStagedUpdateAsync(bool allowInteractive)
    {
        if (!IsUpdateStaged)
        {
            return UpdateInstallOutcome.Failed;
        }

        if (_simulatedUpdate)
        {
            Log?.Invoke(this, "Simulated update install (debug sentinel); exiting without touching the Store.");
            return UpdateInstallOutcome.Installed;
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
                return UpdateInstallOutcome.Failed;
            }

            StorePackageUpdateResult installResult;
            if (_storeContext.CanSilentlyDownloadStorePackageUpdates)
            {
                Log?.Invoke(this, "Store update silent install started.");
                installResult = await _storeContext.TrySilentDownloadAndInstallStorePackageUpdatesAsync(updates);
            }
            else if (allowInteractive)
            {
                Log?.Invoke(this, "Store update interactive install started (silent install unavailable).");
                installResult = await _storeContext.RequestDownloadAndInstallStorePackageUpdatesAsync(updates);
            }
            else
            {
                Log?.Invoke(this, "Store update install needs user consent; leaving it staged for an interactive retry.");
                return UpdateInstallOutcome.NeedsUserInteraction;
            }

            Log?.Invoke(this, $"Store update install finished (OverallState={installResult.OverallState}).");
            IsUpdateStaged = false;
            return installResult.OverallState == StorePackageUpdateState.Completed
                ? UpdateInstallOutcome.Installed
                : UpdateInstallOutcome.Failed;
        }
        catch (Exception ex)
        {
            UpdateCheckFailed?.Invoke(this, Describe(ex));
            IsUpdateStaged = false;
            return UpdateInstallOutcome.Failed;
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
        // _checkNowSignal is deliberately not disposed: the loop may be sitting in WaitAsync on
        // it right now, and cancellation unblocks that without needing the handle torn down.
    }
}
