using Microsoft.UI.Xaml;
using Microsoft.UI.Dispatching;
using Microsoft.Windows.AppLifecycle;
using Windows.ApplicationModel;
using Windows.ApplicationModel.Activation;
using Virtue.WindowsApp.Core.Tray;
using Virtue.WindowsApp.Core.Interop;
using Virtue.WindowsApp.Core.ViewModels;
using WinUiLaunchActivatedEventArgs = Microsoft.UI.Xaml.LaunchActivatedEventArgs;
using StartupTaskActivatedEventArgs = Windows.ApplicationModel.Activation.StartupTaskActivatedEventArgs;
using AppLifecycleInstance = Microsoft.Windows.AppLifecycle.AppInstance;
using System.Runtime.InteropServices;

namespace Virtue.WindowsApp;

public partial class App : Application
{
    private const string StopMonitoringDialogTitle = "Virtue stop monitoring";
    private const string StopMonitoringDialogMessage =
        "Stopping the background service will alert people monitoring you. Continue?";
    private const uint MbIconWarning = 0x00000030;
    private const uint MbOkCancel = 0x00000001;
    private const int IdOk = 1;
    private MainWindow? _mainWindow;
    private SessionViewModel? _viewModel;
    private TrayMenuController? _trayController;
    private CancellationTokenSource? _refreshLoopCancellation;
    private AppLifecycleInstance? _mainInstance;
    private DispatcherQueue? _dispatcherQueue;
    private static readonly string StartupLogPath = Path.Combine(
        Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData),
        "Virtue",
        "ui-startup.log");

    public App()
    {
        AppDomain.CurrentDomain.UnhandledException += CurrentDomainOnUnhandledException;
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
            _trayController.ExitRequested += async (_, _) => await ExitResidentAsync();
            _trayController.SessionLogonObserved += (_, _) => RecordSessionLogon();
            _trayController.SessionLogoffObserved += (_, _) => HandleSessionLogoff();
            _trayController.SystemShutdownObserved += (_, _) => HandleSystemShutdown();
            _trayController.SuspendObserved += (_, _) => RecordSuspend();
            _trayController.ResumeObserved += (_, _) => RecordResume();
            _trayController.Initialize();
            _trayController.UpdateToolTip("Virtue: starting");
            LogStartup("Tray controller initialized.");

            await InitializeViewModelAsync(_viewModel);
            StartRefreshLoop();

            var activation = AppLifecycleInstance.GetCurrent().GetActivatedEventArgs();
            if (!IsStartupActivation(activation))
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
        if (IsStartupActivation(args))
        {
            return;
        }

        _ = _dispatcherQueue?.TryEnqueue(ShowMainWindow);
    }

    private bool IsStartupActivation(AppActivationArguments activation)
    {
        if (activation.Kind != ExtendedActivationKind.StartupTask)
        {
            return false;
        }

        return activation.Data is StartupTaskActivatedEventArgs;
    }

    private void ShowMainWindow()
    {
        _mainWindow ??= CreateMainWindow();
        _mainWindow.ShowFromTray();
    }

    private MainWindow CreateMainWindow()
    {
        if (_viewModel is null)
        {
            throw new InvalidOperationException("View model must be initialized before showing the window.");
        }

        LogStartup("MainWindow created.");
        var window = new MainWindow(_viewModel);
        window.Activate();
        LogStartup("MainWindow activated.");
        return window;
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
            using var timer = new PeriodicTimer(TimeSpan.FromSeconds(15));
            while (await timer.WaitForNextTickAsync(token))
            {
                _ = _dispatcherQueue?.TryEnqueue(async () =>
                {
                    if (_viewModel is null)
                    {
                        return;
                    }

                    await _viewModel.RefreshAsync();
                });
            }
        }, token);
    }

    private async Task ExitResidentAsync()
    {
        if (!ConfirmTrayExit())
        {
            return;
        }

        try
        {
            _refreshLoopCancellation?.Cancel();
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

    private bool ConfirmTrayExit()
    {
        if (_viewModel is null)
        {
            return true;
        }

        if (!_viewModel.LoggedIn)
        {
            return true;
        }

        return MessageBox(IntPtr.Zero, StopMonitoringDialogMessage, StopMonitoringDialogTitle, MbOkCancel | MbIconWarning) == IdOk;
    }

    private static void RecordSessionLogon()
    {
        try
        {
            new RustInteropClient().NotifySessionLogon();
        }
        catch
        {
        }
    }

    private void HandleSessionLogoff()
    {
        try
        {
            _refreshLoopCancellation?.Cancel();
            new RustInteropClient().StopMonitoringForSessionLogoff();
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
            new RustInteropClient().StopMonitoringForSystemShutdown();
        }
        catch (Exception ex)
        {
            LogStartup($"System shutdown lifecycle handling failed: {ex}");
        }
    }

    private static void RecordSuspend()
    {
        try
        {
            new RustInteropClient().NotifySuspend();
        }
        catch
        {
        }
    }

    private static void RecordResume()
    {
        try
        {
            new RustInteropClient().NotifyResume();
        }
        catch
        {
        }
    }

    [DllImport("user32.dll", EntryPoint = "MessageBoxW", CharSet = CharSet.Unicode)]
    private static extern int MessageBox(IntPtr hWnd, string text, string caption, uint type);
}
