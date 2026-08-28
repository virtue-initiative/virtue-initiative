using Microsoft.UI.Xaml;
using Microsoft.UI.Dispatching;
using Microsoft.Windows.AppLifecycle;
using Windows.ApplicationModel;
using Windows.ApplicationModel.Activation;
using WinRT;
using Virtue.WindowsApp.Core.Tray;
using Virtue.WindowsApp.Core.Interop;
using Virtue.WindowsApp.Core.ViewModels;
using Virtue.WindowsApp.Update;
using WinUiLaunchActivatedEventArgs = Microsoft.UI.Xaml.LaunchActivatedEventArgs;
using StartupTaskActivatedEventArgs = Windows.ApplicationModel.Activation.StartupTaskActivatedEventArgs;
using AppLifecycleInstance = Microsoft.Windows.AppLifecycle.AppInstance;

namespace Virtue.WindowsApp;

public partial class App : Application
{
    private static readonly TimeSpan RefreshInterval = TimeSpan.FromSeconds(5);
    private MainWindow? _mainWindow;
    private SessionViewModel? _viewModel;
    private TrayMenuController? _trayController;
    private CancellationTokenSource? _refreshLoopCancellation;
    private AppLifecycleInstance? _mainInstance;
    private DispatcherQueue? _dispatcherQueue;
    private StoreUpdateManager? _updateManager;
    private CancellationTokenSource? _countdownCancellation;
    private DateTimeOffset? _updateStagedAtUtc;
    private int _updateRestartStarted;
    private static readonly TimeSpan CountdownTickInterval = TimeSpan.FromMinutes(1);
    private static readonly string StartupLogPath = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
        "Virtue",
        "ui-startup.log");

    public App()
    {
        AppDomain.CurrentDomain.UnhandledException += CurrentDomainOnUnhandledException;
        UnhandledException += OnXamlUnhandledException;
        InitializeComponent();
        LogStartup("App constructed.");
    }

    protected override async void OnLaunched(WinUiLaunchActivatedEventArgs args)
    {
        base.OnLaunched(args);

        try
        {
            LogStartup("OnLaunched entered.");
            _dispatcherQueue = DispatcherQueue.GetForCurrentThread();
            if (!await EnsureSingleInstanceAsync())
            {
                LogStartup("Activation redirected to resident instance.");
                return;
            }

            _viewModel = new SessionViewModel(
                new RustInteropClient(),
                ResolveWindowsPackageVersion());
            _viewModel.PropertyChanged += ViewModelOnPropertyChanged;
            LogStartup("SessionViewModel created.");

            _trayController = new TrayMenuController();
            _trayController.OpenRequested += (_, _) => ShowMainWindow();
            _trayController.ExitRequested += async (_, _) => await RequestResidentShutdownAsync();
            _trayController.ReportBugRequested += async (_, _) => await ShowReportBugFromTrayAsync();
            _trayController.ForceCaptureRequested += async (_, _) => await ForceCaptureAsync();
            _trayController.SessionLogoffObserved += (_, _) => HandleSessionLogoff();
            _trayController.SystemShutdownObserved += (_, _) => HandleSystemShutdown();
            _trayController.Initialize();
            _trayController.UpdateToolTip("Virtue: starting");
            LogStartup("Tray controller initialized.");

            await InitializeViewModelAsync(_viewModel);
            StartRefreshLoop();

            RegisterWatchdog();

            // The tray host's hidden window is the process's only HWND in resident mode, and
            // StoreContext needs one before any Store call that can show UI — see
            // StoreUpdateManager's owner-window note. Initialize() ran above, so it exists.
            _updateManager = new StoreUpdateManager(_trayController.WindowHandle);
            _updateManager.UpdateStaged += (_, _) => OnUpdateStaged();
            _updateManager.UpdateCheckFailed += (_, reason) =>
            {
                LogStartup($"Store update check/download failed: {reason}");
                SetUpdateCheckStatus("Update check failed.");
            };
            _updateManager.CheckCompleted += (_, foundUpdate) =>
                SetUpdateCheckStatus(foundUpdate ? null : "Up to date.");
            _updateManager.Log += (_, message) => LogStartup(message);
            _updateManager.Start();

            var activation = AppLifecycleInstance.GetCurrent().GetActivatedEventArgs();
            if (!IsQuietActivation(activation))
            {
                ShowMainWindow();
            }
        }
        catch (Exception ex)
        {
            LogStartup($"OnLaunched failed: {ex}");
            throw;
        }
    }

    private static async Task InitializeViewModelAsync(SessionViewModel viewModel)
    {
        try
        {
            LogStartup("ViewModel initialization started.");
            await viewModel.InitializeAsync();
            LogStartup("ViewModel initialization completed.");
        }
        catch (Exception ex)
        {
            LogStartup($"ViewModel initialization failed: {ex}");
        }
    }

    private static void CurrentDomainOnUnhandledException(object sender, System.UnhandledExceptionEventArgs args)
    {
        LogStartup($"Unhandled exception: {args.ExceptionObject}");
    }

    private static void OnXamlUnhandledException(object sender, Microsoft.UI.Xaml.UnhandledExceptionEventArgs args)
    {
        LogStartup($"XAML unhandled exception: {args.Message}{Environment.NewLine}{args.Exception}");
    }

    private static string ResolveWindowsPackageVersion()
    {
        try
        {
            var version = Package.Current.Id.Version;
            return $"{version.Major}.{version.Minor}.{version.Build}.{version.Revision}";
        }
        catch
        {
            var assemblyVersion = typeof(App).Assembly.GetName().Version;
            return assemblyVersion?.ToString() ?? string.Empty;
        }
    }

    private static void LogStartup(string message)
    {
        try
        {
            var directory = Path.GetDirectoryName(StartupLogPath);
            if (!string.IsNullOrWhiteSpace(directory))
            {
                Directory.CreateDirectory(directory);
            }

            File.AppendAllText(StartupLogPath, $"[{DateTimeOffset.Now:O}] {message}{Environment.NewLine}");
        }
        catch
        {
            // Keep logging failures from taking down startup.
        }
    }

    private async Task<bool> EnsureSingleInstanceAsync()
    {
        _mainInstance = AppLifecycleInstance.FindOrRegisterForKey("VirtueResidentMain");
        if (_mainInstance.IsCurrent)
        {
            _mainInstance.Activated += MainInstanceOnActivated;
            return true;
        }

        await _mainInstance.RedirectActivationToAsync(AppLifecycleInstance.GetCurrent().GetActivatedEventArgs());
        Current.Exit();
        return false;
    }

    private void MainInstanceOnActivated(object? sender, AppActivationArguments args)
    {
        if (IsQuietActivation(args))
        {
            return;
        }

        _ = _dispatcherQueue?.TryEnqueue(ShowMainWindow);
    }

    /// <summary>
    /// Command-line arg the watchdog Scheduled Task relaunches with, so the resident
    /// process comes back into the tray quietly rather than popping a window.
    /// </summary>
    private const string WatchdogRelaunchArg = "--restarted-by-watchdog";

    private static bool IsQuietActivation(AppActivationArguments activation)
    {
        return IsStartupActivation(activation) || IsWatchdogActivation(activation);
    }

    private static bool IsStartupActivation(AppActivationArguments activation)
    {
        if (activation.Kind != ExtendedActivationKind.StartupTask)
        {
            return false;
        }

        return activation.Data is StartupTaskActivatedEventArgs;
    }

    private static bool IsWatchdogActivation(AppActivationArguments activation)
    {
        if (activation.Kind != ExtendedActivationKind.Launch || activation.Data is null)
        {
            return false;
        }

        try
        {
            var launchArgs = activation.Data.As<Windows.ApplicationModel.Activation.ILaunchActivatedEventArgs>();
            return launchArgs.Arguments.Contains(WatchdogRelaunchArg, StringComparison.Ordinal);
        }
        catch (InvalidCastException)
        {
            return false;
        }
    }

    private static void RegisterWatchdog()
    {
        var exePath = Environment.ProcessPath;
        if (!string.IsNullOrEmpty(exePath))
        {
            RestartWatchdog.Register(exePath, WatchdogRelaunchArg);
        }
    }

    private void ShowMainWindow()
    {
        _mainWindow ??= CreateMainWindow();
        _mainWindow.ShowFromTray();
        // Seed the notice card's countdown from the window's now-visible state, rather than
        // leaving the generic "will install soon" text up until the next minute tick.
        EvaluateUpdateRestart();
        _ = _viewModel?.RefreshAsync();
    }

    private async Task ShowReportBugFromTrayAsync()
    {
        ShowMainWindow();
        if (_mainWindow is not null)
        {
            await _mainWindow.ShowReportBugDialogAsync();
        }
    }

    private async Task ForceCaptureAsync()
    {
        try
        {
            await Task.Run(() => new RustInteropClient().ForceScreenshotAndUpload());
            _trayController?.ShowBalloonTip("Virtue", "Screenshot captured and uploading");
        }
        catch (InvalidOperationException ex)
        {
            LogStartup($"Force capture failed: {ex}");
            _trayController?.ShowBalloonTip("Virtue", $"Force screenshot failed: {ex.Message}");
        }
    }

    private MainWindow CreateMainWindow()
    {
        if (_viewModel is null)
        {
            throw new InvalidOperationException("View model must be initialized before showing the window.");
        }

        LogStartup("MainWindow created.");
        var window = new MainWindow(_viewModel);
        window.Hidden += (_, _) => EvaluateUpdateRestart();
        window.CloseNowAndUpdateRequested += (_, _) => _ = HandleManualRestartToUpdateAsync();
        window.CheckForUpdatesRequested += (_, _) =>
        {
            SetUpdateCheckStatus("Checking for updates...");
            _updateManager?.RequestCheckNow();
        };
        window.Activate();
        LogStartup("MainWindow activated.");
        return window;
    }

    public Task RequestResidentShutdownAsync()
    {
        return ExitResidentAsync(requireConfirmation: true);
    }

    private void ViewModelOnPropertyChanged(object? sender, System.ComponentModel.PropertyChangedEventArgs e)
    {
        if (e.PropertyName is nameof(SessionViewModel.TrayTooltip) && _viewModel is not null)
        {
            _trayController?.UpdateToolTip(_viewModel.TrayTooltip);
        }

        if (e.PropertyName is nameof(SessionViewModel.LoggedIn) && _viewModel is not null)
        {
            // SetForceCaptureAvailable rebuilds the tray HMENU, and HMENUs are USER objects
            // owned by the thread that creates them, so keep that work on the UI thread that
            // owns the tray window rather than on whatever thread raised PropertyChanged.
            var loggedIn = _viewModel.LoggedIn;
            _ = _dispatcherQueue?.TryEnqueue(() => _trayController?.SetForceCaptureAvailable(loggedIn));
        }
    }

    private void StartRefreshLoop()
    {
        if (_viewModel is null || _refreshLoopCancellation is not null)
        {
            return;
        }

        _refreshLoopCancellation = new CancellationTokenSource();
        var token = _refreshLoopCancellation.Token;
        _ = Task.Run(async () =>
        {
            using var timer = new PeriodicTimer(RefreshInterval);
            while (await timer.WaitForNextTickAsync(token))
            {
                _ = _dispatcherQueue?.TryEnqueue(async () =>
                {
                    if (_viewModel is null)
                    {
                        return;
                    }

                    await _viewModel.BackgroundRefreshAsync();
                });
            }
        }, token);
    }

    private async Task ExitResidentAsync(bool requireConfirmation)
    {
        if (requireConfirmation && !await ConfirmResidentShutdownAsync())
        {
            return;
        }

        try
        {
            RestartWatchdog.Unregister();
            _refreshLoopCancellation?.Cancel();
            _countdownCancellation?.Cancel();
            _updateManager?.Dispose();
            if (_viewModel is not null)
            {
                if (_viewModel.LoggedIn)
                {
                    await _viewModel.StopMonitoringFromTrayExitAsync();
                }
                else
                {
                    await _viewModel.StopMonitoringAsync();
                }
            }
        }
        catch (Exception ex)
        {
            LogStartup($"Resident shutdown failed: {ex}");
        }
        finally
        {
            _trayController?.Dispose();
            if (_mainWindow is not null)
            {
                _mainWindow.PrepareForExit();
                _mainWindow.Close();
                _mainWindow = null;
            }

            Current.Exit();
        }
    }

    private async Task<bool> ConfirmResidentShutdownAsync()
    {
        if (_viewModel is null)
        {
            return true;
        }

        if (!_viewModel.LoggedIn)
        {
            return true;
        }

        var shouldHideAfterCancel = false;
        if (_mainWindow is null)
        {
            ShowMainWindow();
            shouldHideAfterCancel = true;
        }
        else if (!_mainWindow.IsVisibleToUser)
        {
            _mainWindow.ShowFromTray();
            shouldHideAfterCancel = true;
        }

        if (_mainWindow is null)
        {
            return true;
        }

        var confirmed = await _mainWindow.ShowStopMonitoringConfirmationAsync();
        if (!confirmed && shouldHideAfterCancel)
        {
            _mainWindow.HideToTray();
        }

        return confirmed;
    }

    private void HandleSessionLogoff()
    {
        try
        {
            _refreshLoopCancellation?.Cancel();
            new RustInteropClient().StopMonitoringForOsSessionEnd();
        }
        catch (Exception ex)
        {
            LogStartup($"Session logoff lifecycle handling failed: {ex}");
        }
    }

    private void HandleSystemShutdown()
    {
        try
        {
            _refreshLoopCancellation?.Cancel();
            new RustInteropClient().StopMonitoringForOsSessionEnd();
        }
        catch (Exception ex)
        {
            LogStartup($"System shutdown lifecycle handling failed: {ex}");
        }
    }

    /// <summary>
    /// Fired once <see cref="StoreUpdateManager"/> finishes downloading/staging an update.
    /// Reflects the state in the tray tooltip, evaluates immediately (covers the case where
    /// the window is already hidden, and seeds the countdown text if it's open), then starts a
    /// 1-minute timer that re-evaluates each tick — see <see cref="EvaluateUpdateRestart"/>.
    /// </summary>
    private void OnUpdateStaged()
    {
        _updateStagedAtUtc = DateTimeOffset.UtcNow;
        _ = _dispatcherQueue?.TryEnqueue(() => _viewModel?.NotifyUpdateStaged());
        LogStartup("Store update staged.");

        EvaluateUpdateRestart();

        _countdownCancellation?.Cancel();
        _countdownCancellation = new CancellationTokenSource();
        var token = _countdownCancellation.Token;
        _ = Task.Run(async () =>
        {
            using var timer = new PeriodicTimer(CountdownTickInterval);
            while (await timer.WaitForNextTickAsync(token))
            {
                EvaluateUpdateRestart();
            }
        }, token);
    }

    /// <summary>
    /// The single decision point for the staged-update restart: called immediately when an
    /// update stages, every minute while it's pending, every time the main window is shown, and
    /// every time it transitions to hidden (<see cref="MainWindow.Hidden"/>). If the window is
    /// hidden and the session isn't busy, restarts right away. Otherwise updates the in-window
    /// countdown text, and once the deferral cap is reached, hides the window itself — which
    /// re-raises <see cref="MainWindow.Hidden"/> and re-enters this method, taking the hidden
    /// branch to actually restart.
    ///
    /// The whole body runs on the UI thread: it reads <see cref="MainWindow.IsVisibleToUser"/>,
    /// which is written there, and the countdown timer calls in from a pool thread.
    /// </summary>
    private void EvaluateUpdateRestart()
    {
        _ = _dispatcherQueue?.TryEnqueue(EvaluateUpdateRestartOnUiThread);
    }

    private void EvaluateUpdateRestartOnUiThread()
    {
        if (_updateManager?.IsUpdateStaged != true || _updateStagedAtUtc is not { } stagedAtUtc)
        {
            return;
        }

        var sessionIsBusy = _viewModel?.IsBusy ?? false;
        var mainWindowVisible = _mainWindow?.IsVisibleToUser ?? false;

        if (!mainWindowVisible)
        {
            if (!sessionIsBusy)
            {
                _ = InstallUpdateAndRestartAsync(allowInteractive: false);
            }

            return;
        }

        var deadlineUtc = UpdateRestartPolicy.GetDeadlineUtc(stagedAtUtc, _updateManager?.DeferralOverride);
        var now = DateTimeOffset.UtcNow;
        _viewModel?.SetUpdateCountdownText(UpdateRestartPolicy.FormatCountdown(deadlineUtc - now));

        if (UpdateRestartPolicy.ShouldForceRestart(sessionIsBusy, deadlineUtc, now))
        {
            LogStartup("Update deferral cap reached; hiding window to force the update restart.");
            _mainWindow?.HideToTray();
        }
    }

    /// <summary>
    /// Runs <paramref name="action"/> on the UI thread and completes once it has actually run.
    /// </summary>
    private Task RunOnUiThreadAsync(Action action)
    {
        var completion = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        var enqueued = _dispatcherQueue?.TryEnqueue(() =>
        {
            try
            {
                action();
                completion.TrySetResult();
            }
            catch (Exception ex)
            {
                completion.TrySetException(ex);
            }
        }) ?? false;

        if (!enqueued)
        {
            completion.TrySetResult();
        }

        return completion.Task;
    }

    private void SetUpdateCheckStatus(string? text)
    {
        _ = _dispatcherQueue?.TryEnqueue(() =>
        {
            if (_viewModel is not null)
            {
                _viewModel.UpdateCheckStatusText = text;
            }
        });
    }

    /// <summary>
    /// The in-window "Close now and update" button's handler — an explicit user request always
    /// proceeds immediately, bypassing the busy/deadline check (unlike the automatic path), and
    /// is the one path allowed to let the OS put its consent dialog on screen, since the user is
    /// looking at the window that will own it.
    /// </summary>
    private async Task HandleManualRestartToUpdateAsync()
    {
        var staged = _updateManager?.IsUpdateStaged == true;
        LogStartup($"Manual restart-to-update requested (update staged={staged}).");
        if (!staged)
        {
            return;
        }

        await InstallUpdateAndRestartAsync(allowInteractive: true);
    }

    /// <summary>
    /// Installs the staged Store update and exits. The monitoring daemon is deliberately
    /// <b>never</b> stopped for this: the OS terminates the process itself for the package
    /// swap, which is the same shape as a crash-recovery relaunch that `RestartWatchdog` (left
    /// registered here, unlike tray Exit) and CORE-002's late-wakeup budget already cover, and
    /// `DaemonState` is persisted every tick so there is nothing to truncate. Stopping it early
    /// instead flipped the window and tray to "Monitoring stopped" for however long the install
    /// took — indefinitely, when it was waiting on a dialog nobody could see. macOS's Sparkle
    /// updater made the same call.
    ///
    /// The main window is likewise <b>not</b> hidden first: <c>RequestDownloadAndInstall...</c>
    /// always shows an OS consent dialog, and that dialog needs a real visible owner.
    ///
    /// Can legitimately be reached from several concurrent triggers around the same moment (a
    /// <see cref="MainWindow.Hidden"/> event, a countdown tick, and the manual button), so an
    /// <see cref="Interlocked"/> guard ensures only the first call actually proceeds.
    ///
    /// Exits only if the install actually succeeded (the Store API normally terminates the app
    /// itself in that case). On failure, exiting anyway would spin: the watchdog relaunches
    /// within a minute, the check re-stages, and the app exits again. Instead the staged state
    /// is cleared and <see cref="StoreUpdateManager"/>'s retry backoff picks the update up again
    /// on its next cycle.
    /// </summary>
    /// <param name="allowInteractive">
    /// Whether the OS may show its install-consent dialog — see
    /// <see cref="StoreUpdateManager.TryInstallStagedUpdateAsync"/>. The automatic path passes
    /// <c>false</c> and, if consent turns out to be required, shows the main window and retries
    /// once with <c>true</c> so the dialog has somewhere visible to appear.
    /// </param>
    private async Task InstallUpdateAndRestartAsync(bool allowInteractive)
    {
        if (Interlocked.CompareExchange(ref _updateRestartStarted, 1, 0) != 0)
        {
            return;
        }

        _countdownCancellation?.Cancel();

        try
        {
            var outcome = await _updateManager!.TryInstallStagedUpdateAsync(allowInteractive);
            LogStartup($"Update install call returned (outcome={outcome}).");

            if (outcome == UpdateInstallOutcome.NeedsUserInteraction)
            {
                LogStartup("Update install needs user consent; showing the main window and retrying interactively.");
                // Awaited, not fire-and-forget: the consent dialog is owned by an HWND, so the
                // window has to actually be up before the retry raises it.
                await RunOnUiThreadAsync(ShowMainWindow);
                outcome = await _updateManager!.TryInstallStagedUpdateAsync(allowInteractive: true);
                LogStartup($"Interactive update install call returned (outcome={outcome}).");
            }

            if (outcome != UpdateInstallOutcome.Installed)
            {
                ClearStagedUpdateAfterFailedInstall();
                return;
            }
        }
        catch (Exception ex)
        {
            LogStartup($"Update install failed: {ex}");
            ClearStagedUpdateAfterFailedInstall();
            return;
        }

        // Application.Exit() has UI-thread affinity, and this method also runs from the
        // background countdown thread, so marshal it through the dispatcher.
        _ = _dispatcherQueue?.TryEnqueue(() => Current.Exit());
    }

    /// <summary>
    /// Resets the staged-update state after an install that didn't happen. Nothing was torn
    /// down to restore (monitoring is never stopped for an update), but the countdown timer,
    /// the notice card and the one-shot restart guard all have to be released — otherwise the
    /// card stays up with a frozen countdown over a dead button, and
    /// <see cref="EvaluateUpdateRestart"/> early-returns forever. The check loop re-stages on
    /// its own backoff, which rebuilds all of it through <see cref="OnUpdateStaged"/>.
    /// </summary>
    private void ClearStagedUpdateAfterFailedInstall()
    {
        LogStartup("Update install did not complete; clearing the staged update and leaving the retry to the update loop.");
        try
        {
            _countdownCancellation?.Cancel();
            _updateStagedAtUtc = null;
            _ = _dispatcherQueue?.TryEnqueue(() => _viewModel?.NotifyUpdateUnstaged());
        }
        finally
        {
            Interlocked.Exchange(ref _updateRestartStarted, 0);
        }
    }

}
