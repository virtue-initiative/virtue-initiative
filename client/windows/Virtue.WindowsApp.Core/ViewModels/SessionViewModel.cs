using System.ComponentModel;
using System.Globalization;
using System.Runtime.CompilerServices;
using Virtue.WindowsApp.Core.Infrastructure;
using Virtue.WindowsApp.Core.Interop;

namespace Virtue.WindowsApp.Core.ViewModels;

/// <summary>What the window and tray show, first match wins (CLIENT-027).</summary>
public enum MonitorPhase
{
    Loading,
    SignedOut,
    Error,
    Stopped,
    Starting,
    Active,
}

public sealed class SessionViewModel : INotifyPropertyChanged
{
    private readonly IRustInteropClient _interopClient;
    private readonly string _windowsPackageVersion;
    private string _buildLabel = "unknown";
    private bool _loggedIn;
    private string _accountEmail = string.Empty;
    private string _emailInput = string.Empty;
    private string _passwordInput = string.Empty;
    private string _deviceNameInput = string.Empty;
    private string _monitorState = "loading";
    private string? _monitorError;
    private string? _deviceId;
    private int _pendingRequestCount;
    private long? _lastScreenshotAtMs;
    private MonitorStatusPayload? _monitorStatus;
    private bool _isBusy;
    private bool _hasLoadedStatus;
    private bool _isHydratingEmailInput;
    private bool _hasUserEditedEmailInput;
    private string? _transitionMessage;
    private string? _errorText;
    private bool _updateReady;
    private string? _updateCountdownText;
    private string? _updateCheckStatusText;
    private bool _updateInstalling;
    private bool _isCheckingForUpdates;
    private Task _currentAction = Task.CompletedTask;

    public SessionViewModel(IRustInteropClient interopClient, string? windowsPackageVersion = null)
    {
        _interopClient = interopClient;
        _windowsPackageVersion = windowsPackageVersion?.Trim() ?? string.Empty;
        RefreshCommand = new DelegateCommand(RefreshAsync, () => !IsBusy);
        LogoutCommand = new DelegateCommand(LogoutAsync, () => !IsBusy);
    }

    public event PropertyChangedEventHandler? PropertyChanged;

    /// <summary>
    /// Raised every time the update check line is set, even to the same text,
    /// so the window can show each result of a check it started.
    /// </summary>
    public event EventHandler? UpdateCheckReported;

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

    /// <summary>
    /// The one state the headline, ring, tray and Status Details all follow
    /// (CLIENT-027). Active needs a screenshot, so a fresh sign-in reads as
    /// starting until the first one.
    /// </summary>
    public MonitorPhase Phase =>
        MonitorState switch
        {
            "loading" => MonitorPhase.Loading,
            "signed_out" => MonitorPhase.SignedOut,
            "error" => MonitorPhase.Error,
            "stopped" => MonitorPhase.Stopped,
            _ => LastScreenshotAtMs is null ? MonitorPhase.Starting : MonitorPhase.Active,
        };

    /// <summary>The window's headline (CLIENT-027).</summary>
    public string Headline =>
        Phase switch
        {
            MonitorPhase.Loading => "Loading...",
            MonitorPhase.SignedOut => "Signed out",
            MonitorPhase.Error => "Monitoring needs attention",
            MonitorPhase.Stopped => "Monitoring stopped",
            MonitorPhase.Active => "Monitoring active",
            _ => "Starting...",
        };

    /// <summary>The one sentence under <see cref="Headline"/> (CLIENT-027).</summary>
    public string StatusSentence =>
        Phase switch
        {
            MonitorPhase.Loading => "Checking your sign-in status...",
            MonitorPhase.SignedOut => "Sign in to start monitoring on this device.",
            MonitorPhase.Error => string.IsNullOrWhiteSpace(MonitorError)
                ? "Virtue could not start monitoring."
                : WithTerminalPunctuation(MonitorError!.Trim()),
            MonitorPhase.Stopped => "Monitoring is stopped on this device.",
            MonitorPhase.Active => $"Virtue is capturing every screen. Last screenshot at {FormatShortTime(LastScreenshotAtMs!.Value)}.",
            _ => "Virtue is getting ready. The first screenshot is taken within a few minutes.",
        };

    /// <summary>The local time, with the date in front when it isn't today.</summary>
    public static string FormatShortTime(long unixMs)
    {
        var local = DateTimeOffset.FromUnixTimeMilliseconds(unixMs).ToLocalTime();
        var time = local.ToString("t", CultureInfo.CurrentCulture);
        return local.Date == DateTimeOffset.Now.Date
            ? time
            : $"{local.ToString("M", CultureInfo.CurrentCulture)}, {time}";
    }

    private static string WithTerminalPunctuation(string text) =>
        text.EndsWith('.') || text.EndsWith('!') || text.EndsWith('?') ? text : text + ".";

    public string MonitorState
    {
        get => _monitorState;
        private set
        {
            if (SetProperty(ref _monitorState, value))
            {
                PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(MonitorStateDisplay)));
                PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(TrayTooltip)));
                RaiseHeadlineChanged();
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
                RaiseHeadlineChanged();
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
        private set
        {
            if (SetProperty(ref _lastScreenshotAtMs, value))
            {
                PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(TrayTooltip)));
                RaiseHeadlineChanged();
            }
        }
    }

    /// <summary>
    /// The last full status payload read from the core, which the Status
    /// Details dialog renders. Individual properties above stay for the parts
    /// the main window binds to directly.
    /// </summary>
    public MonitorStatusPayload? MonitorStatus
    {
        get => _monitorStatus;
        private set => SetProperty(ref _monitorStatus, value);
    }

    public string MonitorStateDisplay => MonitorState.Replace('_', ' ');

    public string TrayTooltip =>
        BaseTrayTooltip + (_updateReady ? " (update ready)" : string.Empty);

    // Running and starting follow the headline, so the tray never says active
    // while the window still says starting.
    private string BaseTrayTooltip =>
        Phase switch
        {
            MonitorPhase.Loading => "Virtue: loading status",
            MonitorPhase.SignedOut => "Virtue: sign in required",
            MonitorPhase.Error => string.IsNullOrWhiteSpace(MonitorError)
                ? "Virtue: monitoring error"
                : $"Virtue: {MonitorError}",
            MonitorPhase.Stopped => "Virtue: monitoring stopped",
            MonitorPhase.Active => "Virtue: monitoring active",
            _ => "Virtue: starting monitoring",
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

    /// <summary>
    /// Called when a staged update stops being staged without the app restarting — the install
    /// was declined, or the Store retracted the update. Without this the notice card would stay
    /// up forever showing a frozen countdown over a button that no longer does anything. The
    /// check loop re-stages on its own backoff, which rebuilds all of this through
    /// <see cref="NotifyUpdateStaged"/>.
    /// </summary>
    public void NotifyUpdateUnstaged()
    {
        UpdateCountdownText = null;
        UpdateInstalling = false;
        if (SetProperty(ref _updateReady, false, nameof(UpdateReady)))
        {
            PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(TrayTooltip)));
        }
    }

    /// <summary>
    /// Called once the install has actually been started, which ends with the OS terminating
    /// this process and the restart watchdog relaunching it. The notice card says so and its
    /// button goes dead, so the wait doesn't read as the button having done nothing.
    /// </summary>
    public void NotifyUpdateInstalling()
    {
        UpdateInstalling = true;
    }

    public bool UpdateInstalling
    {
        get => _updateInstalling;
        private set => SetProperty(ref _updateInstalling, value);
    }

    public bool UpdateReady => _updateReady;

    /// <summary>
    /// Feedback for the manual "Check for updates" button, rendered beside it. Every check
    /// outcome maps to some text here — an empty line after a click reads as a hang.
    /// </summary>
    public string? UpdateCheckStatusText
    {
        get => _updateCheckStatusText;
        private set => SetProperty(ref _updateCheckStatusText, value);
    }

    /// <summary>True from "Checking for updates..." until that check reports its outcome.</summary>
    public bool IsCheckingForUpdates
    {
        get => _isCheckingForUpdates;
        private set => SetProperty(ref _isCheckingForUpdates, value);
    }

    /// <summary>Sets the update check line (CLIENT-029) and raises <see cref="UpdateCheckReported"/>.</summary>
    public void ReportUpdateCheck(string text, bool checking)
    {
        UpdateCheckStatusText = text;
        IsCheckingForUpdates = checking;
        UpdateCheckReported?.Invoke(this, EventArgs.Empty);
    }

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
        }, "Signing in...");
    }

    public async Task LogoutAsync()
    {
        await RunBusyAsync(async () =>
        {
            await Task.Run(() => _interopClient.Logout());
            await RefreshInternalAsync();
        }, "Signing out...");
    }

    /// <summary>
    /// Runs a test screenshot and returns what it actually did — the interop
    /// call only comes back once the resulting batch has landed or the wait
    /// timed out. Null means the call failed; <see cref="ErrorText"/> says why.
    /// </summary>
    public async Task<ForceCapturePayload?> ForceCaptureAsync()
    {
        ForceCapturePayload? result = null;
        await RunBusyAsync(async () =>
        {
            try
            {
                result = await Task.Run(() => _interopClient.ForceScreenshotAndUpload());
            }
            catch (Exception ex)
            {
                // CLIENT-008: RunBusyAsync shows this message as the error.
                throw new InvalidOperationException($"Test screenshot failed: {ex.Message}", ex);
            }
            await RefreshInternalAsync();
        }, "Screenshot uploading...");
        return result;
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
        MonitorStatus = monitorStatus;

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
        DeviceId = isSignedIn ? (status.DeviceId ?? monitorStatus.DeviceId) : null;
        // Only fall back to the daemon's email while signed in — a stale
        // monitor snapshot must not resurrect an email after a sign-out.
        AccountEmail = status.Email ?? (isSignedIn ? monitorStatus.AccountEmail : null) ?? string.Empty;
        MonitorState = resolvedMonitorState;
        MonitorError = isSignedIn ? monitorStatus.LastError : null;
        PendingRequestCount = isSignedIn ? monitorStatus.PendingRequestCount : 0;
        LastScreenshotAtMs = isSignedIn ? monitorStatus.LastScreenshotAtMs : null;
        MonitorStatus = monitorStatus;
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
        await WaitForIdleAsync();
        await RunBusyAsync(async () =>
        {
            await Task.Run(() => _interopClient.StopMonitoring());
            await RefreshInternalAsync();
        });
    }

    /// <summary>
    /// Exit waits for a running action instead of being ignored (CLIENT-027),
    /// so the user stop is always recorded before the app closes.
    /// </summary>
    public async Task StopMonitoringFromTrayExitAsync()
    {
        await WaitForIdleAsync();
        await RunBusyAsync(async () =>
        {
            await Task.Run(() => _interopClient.StopMonitoringFromTrayExit());
            await RefreshInternalAsync();
        });
    }

    private async Task RunBusyAsync(Func<Task> action, string? transitionMessage = null)
    {
        if (IsBusy)
        {
            return;
        }

        var done = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
        _currentAction = done.Task;
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
        }
        finally
        {
            TransitionMessage = null;
            IsBusy = false;
            done.SetResult();
        }
    }

    private async Task WaitForIdleAsync()
    {
        while (IsBusy)
        {
            await _currentAction;
        }
    }

    /// <summary>Clears a shown error, for example when the user leaves the screen it belongs to.</summary>
    public void ClearError()
    {
        ErrorText = null;
    }

    private void RaiseHeadlineChanged()
    {
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(Phase)));
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(Headline)));
        PropertyChanged?.Invoke(this, new PropertyChangedEventArgs(nameof(StatusSentence)));
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
}
