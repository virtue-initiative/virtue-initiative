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
    private string _monitorState = "stopped";
    private string? _monitorError;
    private string? _deviceId;
    private int _pendingRequestCount;
    private long? _lastScreenshotAtMs;
    private string _apiBaseUrl = string.Empty;
    private string _captureIntervalSeconds = string.Empty;
    private string _batchWindowSeconds = string.Empty;
    private string _configPath = "Loading config path...";
    private bool _isBusy;
    private bool _isHydratingEmailInput;
    private bool _hasUserEditedEmailInput;

    public SessionViewModel(IRustInteropClient interopClient, string? windowsPackageVersion = null)
    {
        _interopClient = interopClient;
        _windowsPackageVersion = windowsPackageVersion?.Trim() ?? string.Empty;
        RefreshCommand = new DelegateCommand(RefreshAsync, () => !IsBusy);
        SaveSettingsCommand = new DelegateCommand(SaveSettingsAsync, () => !IsBusy);
        LogoutCommand = new DelegateCommand(LogoutAsync, () => !IsBusy);
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    public DelegateCommand RefreshCommand { get; }

    public DelegateCommand SaveSettingsCommand { get; }

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

    public string LoggedInText => LoggedIn ? "Yes" : "No";

    public string AccountSummary => string.IsNullOrWhiteSpace(AccountEmail) ? "Not signed in" : AccountEmail;

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
        MonitorState switch
        {
            "running" => "Virtue: monitoring active",
            "starting" => "Virtue: starting monitoring",
            "error" => string.IsNullOrWhiteSpace(MonitorError)
                ? "Virtue: monitoring error"
                : $"Virtue: {MonitorError}",
            "signed_out" => "Virtue: sign in required",
            _ => "Virtue: monitoring stopped",
        };

    public string ApiBaseUrl
    {
        get => _apiBaseUrl;
        set => SetProperty(ref _apiBaseUrl, value);
    }

    public string CaptureIntervalSeconds
    {
        get => _captureIntervalSeconds;
        set => SetProperty(ref _captureIntervalSeconds, value);
    }

    public string BatchWindowSeconds
    {
        get => _batchWindowSeconds;
        set => SetProperty(ref _batchWindowSeconds, value);
    }

    public string ConfigPath
    {
        get => _configPath;
        private set
        {
            if (SetProperty(ref _configPath, value))
            {
                PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(ConfigPathDisplay)));
            }
        }
    }

    public string ConfigPathDisplay =>
        string.IsNullOrWhiteSpace(ConfigPath) ? "Config path unavailable." : ConfigPath;

    public bool IsBusy
    {
        get => _isBusy;
        private set
        {
            if (SetProperty(ref _isBusy, value))
            {
                RefreshCommand.RaiseCanExecuteChanged();
                SaveSettingsCommand.RaiseCanExecuteChanged();
                LogoutCommand.RaiseCanExecuteChanged();
            }
        }
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
            StatusText = "Email is required.";
            return;
        }

        if (string.IsNullOrEmpty(PasswordInput))
        {
            StatusText = "Password is required.";
            return;
        }

        var deviceName = string.IsNullOrWhiteSpace(DeviceNameInput)
            ? Environment.MachineName
            : DeviceNameInput.Trim();

        await RunBusyAsync(async () =>
        {
            StatusText = "Signing in...";
            var email = EmailInput.Trim();
            var password = PasswordInput;
            await Task.Run(() => _interopClient.Login(email, password, deviceName));
            PasswordInput = string.Empty;
            await RefreshInternalAsync();
            StatusText = BuildStatusText();
        });
    }

    public async Task LogoutAsync()
    {
        await RunBusyAsync(async () =>
        {
            StatusText = "Signing out...";
            await Task.Run(() => _interopClient.Logout());
            await RefreshInternalAsync();
            StatusText = BuildStatusText();
        });
    }

    public async Task RefreshAsync()
    {
        await RunBusyAsync(async () =>
        {
            await RefreshInternalAsync();
            StatusText = BuildStatusText();
        });
    }

    public async Task BackgroundRefreshAsync()
    {
        await RunBusyAsync(RefreshInternalAsync);
    }

    public async Task SaveSettingsAsync()
    {
        if (!TryParseInteger(CaptureIntervalSeconds, out var captureIntervalSeconds, out var captureError))
        {
            StatusText = captureError;
            return;
        }

        if (!TryParseInteger(BatchWindowSeconds, out var batchWindowSeconds, out var batchError))
        {
            StatusText = batchError;
            return;
        }

        await RunBusyAsync(async () =>
        {
            StatusText = "Saving runtime settings...";
            _interopClient.SetRuntimeConfig(new RuntimeConfigUpdate(ApiBaseUrl, captureIntervalSeconds, batchWindowSeconds));
            await RefreshInternalAsync();
            StatusText = "Runtime settings saved.";
        });
    }

    private Task RefreshInternalAsync()
    {
        var status = _interopClient.GetSessionStatus();
        var monitorStatus = _interopClient.GetMonitorStatus();
        var runtimeConfig = _interopClient.GetRuntimeConfig();
        var resolvedMonitorState = ResolveMonitorState(status.LoggedIn, monitorStatus.State);
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
        ApiBaseUrl = runtimeConfig.ApiBaseUrl;
        CaptureIntervalSeconds = runtimeConfig.CaptureIntervalSeconds.ToString();
        BatchWindowSeconds = runtimeConfig.BatchWindowSeconds.ToString();
        ConfigPath = runtimeConfig.ConfigPath;

        return Task.CompletedTask;
    }

    private static string ResolveMonitorState(bool loggedIn, string monitorState)
    {
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

    private async Task RunBusyAsync(Func<Task> action)
    {
        if (IsBusy)
        {
            return;
        }

        try
        {
            IsBusy = true;
            await action();
        }
        catch (Exception ex)
        {
            StatusText = ex.Message;
        }
        finally
        {
            IsBusy = false;
        }
    }

    private static bool TryParseInteger(string rawValue, out int? value, out string error)
    {
        if (string.IsNullOrWhiteSpace(rawValue))
        {
            value = null;
            error = string.Empty;
            return true;
        }

        if (int.TryParse(rawValue, out var parsed) && parsed >= 0)
        {
            value = parsed;
            error = string.Empty;
            return true;
        }

        value = null;
        error = $"Expected a positive integer value, got '{rawValue}'.";
        return false;
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
