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
    private static readonly TimeSpan CheckInterval = TimeSpan.FromHours(4);

    private readonly StoreContext _storeContext = StoreContext.GetDefault();
    private CancellationTokenSource? _loopCancellation;

    /// <summary>Raised once an update has finished downloading/staging and is ready to install.</summary>
    public event EventHandler? UpdateStaged;

    /// <summary>Raised (log-only) when a check or download attempt fails; the loop retries on its own schedule.</summary>
    public event EventHandler<string>? UpdateCheckFailed;

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
            await CheckOnceAsync().ConfigureAwait(false);

            using var timer = new PeriodicTimer(CheckInterval);
            while (await timer.WaitForNextTickAsync(token).ConfigureAwait(false))
            {
                if (!IsUpdateStaged)
                {
                    await CheckOnceAsync().ConfigureAwait(false);
                }
            }
        }, token);
    }

    private async Task CheckOnceAsync()
    {
        try
        {
            var updates = await _storeContext.GetAppAndOptionalStorePackageUpdatesAsync();
            if (updates.Count == 0)
            {
                return;
            }

            var downloadOperation = _storeContext.RequestDownloadStorePackageUpdatesAsync(updates);
            var downloadResult = await downloadOperation;
            if (downloadResult.OverallState == StorePackageUpdateState.Completed)
            {
                IsUpdateStaged = true;
                UpdateStaged?.Invoke(this, EventArgs.Empty);
            }
            else
            {
                UpdateCheckFailed?.Invoke(this, $"update download ended in state {downloadResult.OverallState}");
            }
        }
        catch (Exception ex)
        {
            UpdateCheckFailed?.Invoke(this, ex.Message);
        }
    }

    /// <summary>
    /// Installs the update staged by a prior <see cref="CheckOnceAsync"/> call. Only ever
    /// called once the caller has confirmed this is a safe point (daemon stopped). Never
    /// throws — failures are reported via <see cref="UpdateCheckFailed"/> and the caller
    /// should proceed with its planned exit/relaunch regardless of the result, since the
    /// daemon has already been stopped by the time this runs.
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
            if (updates.Count == 0)
            {
                IsUpdateStaged = false;
                return false;
            }

            var installOperation = _storeContext.RequestDownloadAndInstallStorePackageUpdatesAsync(updates);
            var installResult = await installOperation;
            IsUpdateStaged = false;
            return installResult.OverallState == StorePackageUpdateState.Completed;
        }
        catch (Exception ex)
        {
            UpdateCheckFailed?.Invoke(this, ex.Message);
            IsUpdateStaged = false;
            return false;
        }
    }

    public void Dispose()
    {
        _loopCancellation?.Cancel();
        _loopCancellation?.Dispose();
        _loopCancellation = null;
    }
}
