using System.ComponentModel;
using System.Runtime.CompilerServices;
using Virtue.WindowsApp.Core.Infrastructure;
using Virtue.WindowsApp.Core.Interop;

namespace Virtue.WindowsApp.Core.ViewModels;

public sealed class SessionViewModel : INotifyPropertyChanged
{
    private readonly IRustInteropClient _interopClient;
    private readonly string _windowsPackageVersion;
    private string _buildLabel = "unknown";
    private bool _loggedIn;
    private string _accountEmail = string.Empty;
    private string _emailInput = string.Empty;
    private string _passwordInput = string.Empty;
    private string _deviceNameInput = Environment.MachineName;
    private string _statusText = "Starting Virtue...";
    private string _monitorState = "loading";
    private string? _monitorError;
    private string? _deviceId;
    private int _pendingRequestCount;
    private long? _lastScreenshotAtMs;
    private bool _isBusy;
    private bool _hasLoadedStatus;
    private bool _isHydratingEmailInput;
    private bool _hasUserEditedEmailInput;
    private string? _transitionMessage;
    private string? _errorText;
    private bool _updateReady;
    private string? _updateCountdownText;

    public SessionViewModel(IRustInteropClient interopClient, string? windowsPackageVersion = null)
    {
        _interopClient = interopClient;
        _windowsPackageVersion = windowsPackageVersion?.Trim() ?? string.Empty;
        RefreshCommand = new DelegateCommand(RefreshAsync, () => !IsBusy);
        LogoutCommand = new DelegateCommand(LogoutAsync, () => !IsBusy);
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    public DelegateCommand RefreshCommand { get; }

    public DelegateCommand LogoutCommand { get; }

    public string BuildLabel
    {
        get => _buildLabel;
        private set
        {
            if (SetProperty(ref _buildLabel, value))
            {
                PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(BuildLabelText)));
            }
        }
    }

    public bool LoggedIn
    {
        get => _loggedIn;
        private set
        {
            if (SetProperty(ref _loggedIn, value))
            {
                PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(LoggedInText)));
            }
        }
    }

    public bool HasLoadedStatus
    {
        get => _hasLoadedStatus;
        private set
        {
            if (SetProperty(ref _hasLoadedStatus, value))
            {
                PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(LoggedInText)));
                PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(AccountSummary)));
                PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(TrayTooltip)));
            }
        }
    }

    public string AccountEmail
    {
        get => _accountEmail;
        private set
        {
            if (SetProperty(ref _accountEmail, value))
            {
                PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(AccountSummary)));
            }
        }
    }

    public string BuildLabelText =>
        string.IsNullOrWhiteSpace(_windowsPackageVersion)
            ? $"Build {BuildLabel}"
            : $"Build {BuildLabel} | Windows package {_windowsPackageVersion}";

    public string WindowsPackageVersion => _windowsPackageVersion;

    public string LoggedInText => !HasLoadedStatus ? "Loading..." : (LoggedIn ? "Yes" : "No");

    public string AccountSummary => !HasLoadedStatus
        ? "Loading..."
        : (string.IsNullOrWhiteSpace(AccountEmail) ? "Not signed in" : AccountEmail);

    public string EmailInput
    {
        get => _emailInput;
        set
        {
            if (SetProperty(ref _emailInput, value) && !_isHydratingEmailInput)
            {
                _hasUserEditedEmailInput = true;
            }
        }
    }

    public string PasswordInput
    {
        get => _passwordInput;
        set => SetProperty(ref _passwordInput, value);
    }

    public string DeviceNameInput
    {
        get => _deviceNameInput;
        set => SetProperty(ref _deviceNameInput, value);
    }

    public string StatusText
    {
        get => _statusText;
        private set => SetProperty(ref _statusText, value);
    }

    public string MonitorState
    {
        get => _monitorState;
        private set
        {
            if (SetProperty(ref _monitorState, value))
            {
                PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(MonitorStateDisplay)));
                PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(TrayTooltip)));
            }
        }
    }

    public string? MonitorError
    {
        get => _monitorError;
        private set
        {
            if (SetProperty(ref _monitorError, value))
            {
                PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(TrayTooltip)));
            }
        }
    }

    public string? DeviceId
    {
        get => _deviceId;
        private set => SetProperty(ref _deviceId, value);
    }

    public int PendingRequestCount
    {
        get => _pendingRequestCount;
        private set => SetProperty(ref _pendingRequestCount, value);
    }

    public long? LastScreenshotAtMs
    {
        get => _lastScreenshotAtMs;
        private set => SetProperty(ref _lastScreenshotAtMs, value);
    }

    public string MonitorStateDisplay => MonitorState.Replace('_', ' ');

    public string TrayTooltip =>
        BaseTrayTooltip + (_updateReady ? " (update ready)" : string.Empty);

    private string BaseTrayTooltip =>
        MonitorState switch
        {
            "loading" => "Virtue: loading status",
            "running" => "Virtue: monitoring active",
            "starting" => "Virtue: starting monitoring",
            "error" => string.IsNullOrWhiteSpace(MonitorError)
                ? "Virtue: monitoring error"
                : $"Virtue: {MonitorError}",
            "signed_out" => "Virtue: sign in required",
            _ => "Virtue: monitoring stopped",
        };

    /// <summary>
    /// Called once a Store update has finished downloading/staging, so the tray tooltip
    /// reflects it. See <c>Virtue.WindowsApp.Update.StoreUpdateManager</c>.
    /// </summary>
    public void NotifyUpdateStaged()
    {
        if (SetProperty(ref _updateReady, true, nameof(UpdateReady)))
        {
            PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(TrayTooltip)));
        }
    }

    public bool UpdateReady => _updateReady;

    public string? UpdateCountdownText
    {
        get => _updateCountdownText;
        private set => SetProperty(ref _updateCountdownText, value);
    }

    /// <summary>
    /// Called each time the countdown to the forced Store-update restart deadline is
    /// recomputed, so the in-window notice can show it. See <c>App.EvaluateUpdateRestart</c>.
    /// </summary>
    public void SetUpdateCountdownText(string? text)
    {
        UpdateCountdownText = text;
    }

    public bool IsBusy
    {
        get => _isBusy;
        private set
        {
            if (SetProperty(ref _isBusy, value))
            {
                RefreshCommand.RaiseCanExecuteChanged();
                LogoutCommand.RaiseCanExecuteChanged();
            }
        }
    }

    public string? TransitionMessage
    {
        get => _transitionMessage;
        private set
        {
            if (SetProperty(ref _transitionMessage, value))
            {
                PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(IsTransitioning)));
            }
        }
    }

    public bool IsTransitioning => !string.IsNullOrEmpty(TransitionMessage);

    public string? ErrorText
    {
        get => _errorText;
        private set => SetProperty(ref _errorText, value);
    }

    public async Task InitializeAsync()
    {
        await RunBusyAsync(async () =>
        {
            _interopClient.Initialize();
            _interopClient.StartMonitoring();
            await RefreshInternalAsync();
            StatusText = BuildStatusText();
        });
    }

    public async Task LoginAsync()
    {
        if (string.IsNullOrWhiteSpace(EmailInput))
        {
            ErrorText = "Email is required.";
            return;
        }

        if (string.IsNullOrEmpty(PasswordInput))
        {
            ErrorText = "Password is required.";
            return;
        }

        var deviceName = string.IsNullOrWhiteSpace(DeviceNameInput)
            ? Environment.MachineName
            : DeviceNameInput.Trim();

        await RunBusyAsync(async () =>
        {
            var email = EmailInput.Trim();
            var password = PasswordInput;
            await Task.Run(() => _interopClient.Login(email, password, deviceName));
            PasswordInput = string.Empty;
            await RefreshInternalAsync();
            StatusText = BuildStatusText();
        }, "Signing in...");
    }

    public async Task LogoutAsync()
    {
        await RunBusyAsync(async () =>
        {
            await Task.Run(() => _interopClient.Logout());
            await RefreshInternalAsync();
            StatusText = BuildStatusText();
        }, "Signing out...");
    }

    public async Task<bool> SubmitBugReportAsync(string message, string? contactEmail, bool includeLogs)
    {
        var succeeded = false;
        await RunBusyAsync(async () =>
        {
            await Task.Run(() => _interopClient.ReportIssue(message, contactEmail, includeLogs));
            succeeded = true;
        }, "Sending report...");
        return succeeded;
    }

    public async Task RefreshAsync()
    {
        await RunBusyAsync(async () =>
        {
            await RefreshInternalAsync();
            StatusText = BuildStatusText();
        });
    }

    public Task BackgroundRefreshAsync()
    {
        return BackgroundRefreshInternalAsync();
    }

    private Task BackgroundRefreshInternalAsync()
    {
        var monitorStatus = _interopClient.GetMonitorStatus();
        var resolvedMonitorState = ResolveMonitorState(_hasLoadedStatus, _loggedIn, monitorStatus.State);

        MonitorState = resolvedMonitorState;
        MonitorError = _loggedIn ? monitorStatus.LastError : null;
        PendingRequestCount = _loggedIn ? monitorStatus.PendingRequestCount : 0;
        LastScreenshotAtMs = _loggedIn ? monitorStatus.LastScreenshotAtMs : null;

        return Task.CompletedTask;
    }

    private Task RefreshInternalAsync()
    {
        var status = _interopClient.GetSessionStatus();
        HasLoadedStatus = true;
        var monitorStatus = _interopClient.GetMonitorStatus();
        var resolvedMonitorState = ResolveMonitorState(_hasLoadedStatus, status.LoggedIn, monitorStatus.State);
        var isSignedIn = status.LoggedIn;

        BuildLabel = status.BuildLabel;
        LoggedIn = isSignedIn;
        DeviceId = isSignedIn ? status.DeviceId : null;
        AccountEmail = status.Email ?? string.Empty;
        MonitorState = resolvedMonitorState;
        MonitorError = isSignedIn ? monitorStatus.LastError : null;
        PendingRequestCount = isSignedIn ? monitorStatus.PendingRequestCount : 0;
        LastScreenshotAtMs = isSignedIn ? monitorStatus.LastScreenshotAtMs : null;
        if (isSignedIn)
        {
            SetEmailInput(status.Email ?? string.Empty);
        }
        else if (!_hasUserEditedEmailInput)
        {
            SetEmailInput(status.Email ?? string.Empty);
        }

        return Task.CompletedTask;
    }

    private static string ResolveMonitorState(bool hasStatus, bool loggedIn, string monitorState)
    {
        if (!hasStatus)
        {
            return "loading";
        }

        if (!loggedIn)
        {
            return "signed_out";
        }

        return monitorState switch
        {
            "signed_out" => "starting",
            _ => monitorState,
        };
    }

    private void SetEmailInput(string value)
    {
        _isHydratingEmailInput = true;
        try
        {
            EmailInput = value;
            _hasUserEditedEmailInput = false;
        }
        finally
        {
            _isHydratingEmailInput = false;
        }
    }

    public async Task StopMonitoringAsync()
    {
        await RunBusyAsync(async () =>
        {
            await Task.Run(() => _interopClient.StopMonitoring());
            await RefreshInternalAsync();
            StatusText = BuildStatusText();
        });
    }

    public async Task StopMonitoringFromTrayExitAsync()
    {
        await RunBusyAsync(async () =>
        {
            await Task.Run(() => _interopClient.StopMonitoringFromTrayExit());
            await RefreshInternalAsync();
            StatusText = BuildStatusText();
        });
    }

    private async Task RunBusyAsync(Func<Task> action, string? transitionMessage = null)
    {
        if (IsBusy)
        {
            return;
        }

        try
        {
            IsBusy = true;
            ErrorText = null;
            if (transitionMessage != null)
            {
                TransitionMessage = transitionMessage;
            }
            await action();
        }
        catch (Exception ex)
        {
            ErrorText = ex.Message;
            StatusText = ex.Message;
        }
        finally
        {
            TransitionMessage = null;
            IsBusy = false;
        }
    }

    private bool SetProperty<T>(ref T field, T value, [CallerMemberName] string? propertyName = null)
    {
        if (EqualityComparer<T>.Default.Equals(field, value))
        {
            return false;
        }

        field = value;
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(propertyName));
        return true;
    }

    private string BuildStatusText()
    {
        if (!LoggedIn)
        {
            return "Sign in to start monitoring.";
        }

        return MonitorState switch
        {
            "running" => "Monitoring is active on this device.",
            "starting" => "Monitoring is starting for this device.",
            "error" when !string.IsNullOrWhiteSpace(MonitorError) => $"Monitoring needs attention: {MonitorError}",
            "error" => "Monitoring needs attention.",
            "stopped" => "Monitoring is stopped on this device.",
            _ => "Monitoring state is updating.",
        };
    }
}
