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
            _trayController.RestartToUpdateRequested += (_, _) => _ = HandleManualRestartToUpdateAsync();
            _trayController.ForceCaptureRequested += async (_, _) => await ForceCaptureAsync();
            _trayController.SessionLogoffObserved += (_, _) => HandleSessionLogoff();
            _trayController.SystemShutdownObserved += (_, _) => HandleSystemShutdown();
            _trayController.Initialize();
            _trayController.UpdateToolTip("Virtue: starting");
            LogStartup("Tray controller initialized.");

            await InitializeViewModelAsync(_viewModel);
            StartRefreshLoop();

            RegisterWatchdog();

            _updateManager = new StoreUpdateManager();
            _updateManager.UpdateStaged += (_, _) => OnUpdateStaged();
            _updateManager.UpdateCheckFailed += (_, reason) => LogStartup($"Store update check/download failed: {reason}");
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
    /// update stages, every minute while it's pending, and every time the main window
    /// transitions to hidden (<see cref="MainWindow.Hidden"/>). If the window is hidden and the
    /// session isn't busy, restarts right away. Otherwise updates the in-window countdown text,
    /// and once the 6-hour deferral cap is reached, hides the window itself — which re-raises
    /// <see cref="MainWindow.Hidden"/> and re-enters this method, taking the hidden branch to
    /// actually restart.
    /// </summary>
    private void EvaluateUpdateRestart()
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
                _ = InstallUpdateAndRestartAsync();
            }

            return;
        }

        var deadlineUtc = UpdateRestartPolicy.GetDeadlineUtc(stagedAtUtc);
        var now = DateTimeOffset.UtcNow;
        var countdownText = UpdateRestartPolicy.FormatCountdown(deadlineUtc - now);
        _ = _dispatcherQueue?.TryEnqueue(() => _viewModel?.SetUpdateCountdownText(countdownText));

        if (UpdateRestartPolicy.ShouldForceRestart(sessionIsBusy, deadlineUtc, now))
        {
            _ = _dispatcherQueue?.TryEnqueue(() => _mainWindow?.HideToTray());
        }
    }

    /// <summary>
    /// The tray "Restart to Update" menu item's and the in-window "Close now and update"
    /// button's shared handler — an explicit user request always proceeds immediately,
    /// bypassing the busy/deadline check (unlike the automatic path).
    /// </summary>
    private async Task HandleManualRestartToUpdateAsync()
    {
        if (_updateManager?.IsUpdateStaged != true)
        {
            return;
        }

        await InstallUpdateAndRestartAsync();
    }

    /// <summary>
    /// Stops resident monitoring the same way an OS session logoff/shutdown does (NOT the
    /// tray-exit path — this must not be treated as a user-initiated stop for CORE-002
    /// purposes, and it must not log the device out), installs the staged Store update, and
    /// exits. `RestartWatchdog` is deliberately left registered (unlike tray Exit) so its
    /// existing per-minute poll relaunches the updated build.
    ///
    /// Can legitimately be reached from several concurrent triggers around the same
    /// moment (a <see cref="MainWindow.Hidden"/> event, a countdown tick, and the manual
    /// button/tray item), so an <see cref="Interlocked"/> guard ensures only the first call
    /// actually proceeds.
    /// </summary>
    private async Task InstallUpdateAndRestartAsync()
    {
        if (Interlocked.CompareExchange(ref _updateRestartStarted, 1, 0) != 0)
        {
            return;
        }

        _countdownCancellation?.Cancel();
        // MainWindow is a WinRT object with UI-thread affinity; this method may run on a
        // background poll thread (the automatic countdown path) as well as the UI thread
        // (the manual tray-menu/button path), so always marshal the hide through the dispatcher.
        _ = _dispatcherQueue?.TryEnqueue(() => _mainWindow?.HideToTray());

        try
        {
            _refreshLoopCancellation?.Cancel();
            new RustInteropClient().StopMonitoringForOsSessionEnd();
            LogStartup("Stopped resident monitoring for Store update install.");

            var installed = await _updateManager!.TryInstallStagedUpdateAsync();
            LogStartup($"Update install call returned (installed={installed}).");
        }
        catch (Exception ex)
        {
            LogStartup($"Update install failed: {ex}");
        }
        finally
        {
            // Application.Exit() also has UI-thread affinity — same reasoning as the
            // HideToTray() marshal above.
            _ = _dispatcherQueue?.TryEnqueue(() => Current.Exit());
        }
    }

}
