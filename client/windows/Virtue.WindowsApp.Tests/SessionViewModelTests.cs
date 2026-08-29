using Virtue.WindowsApp.Core.Interop;
using Virtue.WindowsApp.Core.Tray;
using Virtue.WindowsApp.Core.ViewModels;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class SessionViewModelTests
{
    [Fact]
    public void BeforeAnyRefresh_ReportsLoadingState()
    {
        var fakeClient = new FakeRustInteropClient();
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        Assert.False(viewModel.HasLoadedStatus);
        Assert.Equal("loading", viewModel.MonitorState);
        Assert.Equal("Loading...", viewModel.LoggedInText);
        Assert.Equal("Loading...", viewModel.AccountSummary);
        Assert.Equal("Virtue: loading status", viewModel.TrayTooltip);
    }

    [Fact]
    public void NotifyUpdateStaged_AppendsUpdateReadySuffixToTrayTooltip()
    {
        var fakeClient = new FakeRustInteropClient();
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        viewModel.NotifyUpdateStaged();

        Assert.True(viewModel.UpdateReady);
        Assert.Equal("Virtue: loading status (update ready)", viewModel.TrayTooltip);
    }

    [Fact]
    public void SetUpdateCountdownText_UpdatesProperty()
    {
        var fakeClient = new FakeRustInteropClient();
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        Assert.Null(viewModel.UpdateCountdownText);

        viewModel.SetUpdateCountdownText("42m");

        Assert.Equal("42m", viewModel.UpdateCountdownText);
    }

    [Fact]
    public void NotifyUpdateUnstaged_ClearsTheNoticeCountdownAndTooltipSuffix()
    {
        var fakeClient = new FakeRustInteropClient();
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");
        viewModel.NotifyUpdateStaged();
        viewModel.SetUpdateCountdownText("42m");

        var changed = new List<string?>();
        viewModel.PropertyChanged += (_, e) => changed.Add(e.PropertyName);

        viewModel.NotifyUpdateUnstaged();

        Assert.False(viewModel.UpdateReady);
        Assert.Null(viewModel.UpdateCountdownText);
        Assert.Equal("Virtue: loading status", viewModel.TrayTooltip);
        Assert.Contains(nameof(SessionViewModel.UpdateReady), changed);
        Assert.Contains(nameof(SessionViewModel.UpdateCountdownText), changed);
        Assert.Contains(nameof(SessionViewModel.TrayTooltip), changed);
    }

    [Fact]
    public void UpdateCheckStatusText_RaisesPropertyChanged()
    {
        var fakeClient = new FakeRustInteropClient();
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        Assert.Null(viewModel.UpdateCheckStatusText);

        var changed = new List<string?>();
        viewModel.PropertyChanged += (_, e) => changed.Add(e.PropertyName);
        viewModel.UpdateCheckStatusText = "No updates found.";

        Assert.Equal("No updates found.", viewModel.UpdateCheckStatusText);
        Assert.Contains(nameof(SessionViewModel.UpdateCheckStatusText), changed);
    }

    [Fact]
    public async Task LoginAsync_RefreshesStatusAfterSuccess()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(true, "device-1", "user@example.com", "build-123"),
            MonitorStatus = new MonitorStatusPayload("running", true, 0, 123, null),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234")
        {
            EmailInput = "user@example.com",
            PasswordInput = "secret",
        };

        await viewModel.LoginAsync();

        Assert.Equal(("user@example.com", "secret", Environment.MachineName), fakeClient.LastLogin);
        Assert.True(viewModel.LoggedIn);
        Assert.Equal("Monitoring is active on this device.", viewModel.StatusText);
        Assert.Equal(string.Empty, viewModel.PasswordInput);
    }

    [Fact]
    public async Task RefreshAsync_ExposesTheFullStatusPagePayload()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(true, "device-1", "user@example.com", "build-123"),
            MonitorStatus = new MonitorStatusPayload(
                "running",
                true,
                2,
                123,
                null,
                AccountEmail: "user@example.com",
                DeviceId: "device-1",
                DeviceName: "Work Laptop",
                PartnerCount: 2,
                PendingHashCount: 3,
                PendingBatchCount: 4,
                LastLoopAtMs: 500,
                LastScreenshotAttemptAtMs: 400,
                LastSkipReason: "Screen locked or screensaver active",
                LastBatchAtMs: 300,
                RecentErrors: new[] { new StatusErrorPayload(200, "batch_upload", "boom") },
                ApiBaseUrl: "https://api.example.org",
                HashBaseUrl: "https://hash.example.org",
                CaptureIntervalSeconds: 300,
                BatchWindowSeconds: 60,
                LogDirectory: @"C:\ProgramData\Virtue\data\logs"),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        await viewModel.RefreshAsync();

        var status = viewModel.MonitorStatus;
        Assert.NotNull(status);
        Assert.Equal("Work Laptop", status!.DeviceName);
        Assert.Equal(2, status.PartnerCount);
        Assert.Equal(3, status.PendingHashCount);
        Assert.Equal(4, status.PendingBatchCount);
        Assert.Equal(400, status.LastScreenshotAttemptAtMs);
        Assert.Equal("Screen locked or screensaver active", status.LastSkipReason);
        Assert.Single(status.RecentErrors!);
        Assert.Equal("https://api.example.org", status.ApiBaseUrl);
        Assert.Equal(@"C:\ProgramData\Virtue\data\logs", status.LogDirectory);
        Assert.Equal(123, viewModel.LastScreenshotAtMs);
    }

    [Fact]
    public async Task RefreshAsync_FallsBackToTheDaemonsDeviceIdWhenTheSessionHasNone()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(true, null, null, "build-123"),
            MonitorStatus = new MonitorStatusPayload(
                "running",
                true,
                0,
                null,
                null,
                AccountEmail: "user@example.com",
                DeviceId: "device-from-daemon"),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        await viewModel.RefreshAsync();

        Assert.Equal("device-from-daemon", viewModel.DeviceId);
        Assert.Equal("user@example.com", viewModel.AccountEmail);
    }

    [Fact]
    public async Task StopMonitoringFromTrayExitAsync_UsesExplicitTrayExitInterop()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(true, "device-1", "user@example.com", "build-123"),
            MonitorStatus = new MonitorStatusPayload("running", true, 0, 123, null),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        await viewModel.StopMonitoringFromTrayExitAsync();

        Assert.True(fakeClient.StopMonitoringFromTrayExitCalled);
        Assert.Equal("Monitoring is stopped on this device.", viewModel.StatusText);
    }

    [Fact]
    public async Task RefreshAsync_ClearsStaleMonitorDetailsWhenSignedOut()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(false, null, null, "build-123"),
            MonitorStatus = new MonitorStatusPayload("running", true, 4, 123, "stale error"),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234")
        {
            EmailInput = "stale@example.com",
        };

        await viewModel.RefreshAsync();

        Assert.False(viewModel.LoggedIn);
        Assert.Equal("signed_out", viewModel.MonitorState);
        Assert.Null(viewModel.DeviceId);
        Assert.Null(viewModel.MonitorError);
        Assert.Equal(0, viewModel.PendingRequestCount);
        Assert.Null(viewModel.LastScreenshotAtMs);
        Assert.Equal("stale@example.com", viewModel.EmailInput);
        Assert.Equal("Sign in to start monitoring.", viewModel.StatusText);
    }

    [Fact]
    public async Task RefreshAsync_TreatsSignedOutMonitorStateAsStartingWhenSessionIsSignedIn()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(true, "device-1", "user@example.com", "build-123"),
            MonitorStatus = new MonitorStatusPayload("signed_out", false, 0, null, null),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        await viewModel.RefreshAsync();

        Assert.True(viewModel.LoggedIn);
        Assert.Equal("starting", viewModel.MonitorState);
        Assert.Equal("Monitoring is starting for this device.", viewModel.StatusText);
    }

    [Fact]
    public void TrayMenuController_RoutesOpenAndExitEvents()
    {
        var host = new NullTrayIconHost();
        var controller = new TrayMenuController(host);
        var openRaised = false;
        var exitRaised = false;

        controller.OpenRequested += (_, _) => openRaised = true;
        controller.ExitRequested += (_, _) => exitRaised = true;

        host.RequestOpen();
        host.RequestExit();

        Assert.True(openRaised);
        Assert.True(exitRaised);
    }

    [Fact]
    public void RustInteropJson_ThrowsForNonJsonPayload()
    {
        var ex = Assert.Throws<InvalidOperationException>(() => RustInteropJson.DeserializePayload<SessionStatusPayload>("login failed"));
        Assert.Contains("non-JSON payload", ex.Message);
    }

    [Fact]
    public void RustInteropJson_DeserializesValidPayload()
    {
        const string json = """{"loggedIn":true,"deviceId":"dev-1","email":"u@example.com","buildLabel":"build-42"}""";

        var result = RustInteropJson.DeserializePayload<SessionStatusPayload>(json);

        Assert.True(result.LoggedIn);
        Assert.Equal("dev-1", result.DeviceId);
        Assert.Equal("u@example.com", result.Email);
        Assert.Equal("build-42", result.BuildLabel);
    }

    [Fact]
    public async Task LoginAsync_FailsWithMissingEmail()
    {
        var fakeClient = new FakeRustInteropClient();
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234")
        {
            EmailInput = "   ",
            PasswordInput = "secret",
        };

        await viewModel.LoginAsync();

        Assert.Null(fakeClient.LastLogin);
        Assert.Equal("Email is required.", viewModel.ErrorText);
    }

    [Fact]
    public async Task LoginAsync_FailsWithMissingPassword()
    {
        var fakeClient = new FakeRustInteropClient();
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234")
        {
            EmailInput = "user@example.com",
            PasswordInput = string.Empty,
        };

        await viewModel.LoginAsync();

        Assert.Null(fakeClient.LastLogin);
        Assert.Equal("Password is required.", viewModel.ErrorText);
    }

    [Fact]
    public async Task LogoutAsync_SignsOutAndUpdatesStatusText()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(true, "device-1", "user@example.com", "build-123"),
            MonitorStatus = new MonitorStatusPayload("running", true, 0, null, null),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        await viewModel.LogoutAsync();

        Assert.False(viewModel.LoggedIn);
        Assert.Equal("Sign in to start monitoring.", viewModel.StatusText);
    }

    [Fact]
    public async Task StopMonitoringAsync_CallsStopMonitoringAndUpdatesStatusText()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(true, "device-1", "user@example.com", "build-123"),
            MonitorStatus = new MonitorStatusPayload("running", true, 0, 123, null),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        await viewModel.StopMonitoringAsync();

        Assert.True(fakeClient.StopMonitoringCalled);
        Assert.Equal("Monitoring is stopped on this device.", viewModel.StatusText);
    }

    [Theory]
    [InlineData("running", true, null, "Virtue: monitoring active")]
    [InlineData("stopped", true, null, "Virtue: monitoring stopped")]
    [InlineData("error", true, "upload failed", "Virtue: upload failed")]
    [InlineData("error", true, null, "Virtue: monitoring error")]
    [InlineData("signed_out", false, null, "Virtue: sign in required")]
    public async Task RefreshAsync_TrayTooltipReflectsMonitorState(
        string monitorState, bool loggedIn, string? lastError, string expectedTooltip)
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(loggedIn, loggedIn ? "device-1" : null, loggedIn ? "user@example.com" : null, "build-123"),
            MonitorStatus = new MonitorStatusPayload(monitorState, loggedIn, 0, null, lastError),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        await viewModel.RefreshAsync();

        Assert.Equal(expectedTooltip, viewModel.TrayTooltip);
    }

    [Fact]
    public async Task RefreshAsync_MonitorStateDisplayReplacesUnderscoresWithSpaces()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(false, null, null, "build-123"),
            MonitorStatus = new MonitorStatusPayload("signed_out", false, 0, null, null),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        await viewModel.RefreshAsync();

        Assert.Equal("signed out", viewModel.MonitorStateDisplay);
    }

    [Fact]
    public async Task RefreshAsync_AccountSummaryAndLoggedInTextWhenSignedIn()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(true, "device-1", "user@example.com", "build-123"),
            MonitorStatus = new MonitorStatusPayload("running", true, 0, null, null),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        await viewModel.RefreshAsync();

        Assert.Equal("Yes", viewModel.LoggedInText);
        Assert.Equal("user@example.com", viewModel.AccountSummary);
    }

    [Fact]
    public async Task RefreshAsync_AccountSummaryAndLoggedInTextWhenSignedOut()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(false, null, null, "build-123"),
            MonitorStatus = new MonitorStatusPayload("signed_out", false, 0, null, null),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        await viewModel.RefreshAsync();

        Assert.Equal("No", viewModel.LoggedInText);
        Assert.Equal("Not signed in", viewModel.AccountSummary);
    }

    [Fact]
    public void BuildLabelText_WithNoWindowsPackageVersion()
    {
        var fakeClient = new FakeRustInteropClient();
        var viewModel = new SessionViewModel(fakeClient);

        Assert.Equal("Build unknown", viewModel.BuildLabelText);
    }

    [Fact]
    public async Task RefreshAsync_PreservesUserEditedEmailWhenSignedOut()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(false, null, "server@example.com", "build-123"),
            MonitorStatus = new MonitorStatusPayload("signed_out", false, 0, null, null),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");
        viewModel.EmailInput = "typed@example.com";

        await viewModel.RefreshAsync();

        Assert.Equal("typed@example.com", viewModel.EmailInput);
    }

    [Fact]
    public async Task RefreshAsync_OverwritesEmailInputWhenSignedIn()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(true, "device-1", "server@example.com", "build-123"),
            MonitorStatus = new MonitorStatusPayload("running", true, 0, null, null),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");
        viewModel.EmailInput = "typed@example.com";

        await viewModel.RefreshAsync();

        Assert.Equal("server@example.com", viewModel.EmailInput);
    }

    [Fact]
    public async Task SubmitBugReportAsync_ReturnsTrueAndForwardsFieldsOnSuccess()
    {
        var fakeClient = new FakeRustInteropClient();
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        var result = await viewModel.SubmitBugReportAsync("Screenshots stopped uploading", "me@example.com", true);

        Assert.True(result);
        Assert.Equal(("Screenshots stopped uploading", "me@example.com", true), fakeClient.LastReportIssue);
        Assert.Null(viewModel.ErrorText);
    }

    [Fact]
    public async Task SubmitBugReportAsync_ReturnsFalseAndSetsErrorTextOnFailure()
    {
        var fakeClient = new FakeRustInteropClient
        {
            ReportIssueError = new InvalidOperationException("Too many requests"),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        var result = await viewModel.SubmitBugReportAsync("Screenshots stopped uploading", null, false);

        Assert.False(result);
        Assert.Equal("Too many requests", viewModel.ErrorText);
    }

    [Fact]
    public async Task ForceCaptureAsync_ReturnsTheOutcomeAndInvokesInteropOnSuccess()
    {
        var fakeClient = new FakeRustInteropClient();
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        var result = await viewModel.ForceCaptureAsync();

        Assert.NotNull(result);
        Assert.Equal("uploaded", result!.Outcome);
        Assert.True(fakeClient.ForceScreenshotAndUploadCalled);
        Assert.Null(viewModel.ErrorText);
    }

    [Fact]
    public async Task ForceCaptureAsync_PassesThroughAnOutcomeThatIsNotAnUpload()
    {
        var fakeClient = new FakeRustInteropClient
        {
            ForceScreenshotAndUploadResult = new("not_captured", "No screenshot was taken."),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        var result = await viewModel.ForceCaptureAsync();

        Assert.NotNull(result);
        Assert.Equal("not_captured", result!.Outcome);
        Assert.Equal("No screenshot was taken.", result.Message);
        Assert.Null(viewModel.ErrorText);
    }

    [Fact]
    public async Task ForceCaptureAsync_ReturnsNullAndSetsErrorTextOnFailure()
    {
        var fakeClient = new FakeRustInteropClient
        {
            ForceScreenshotAndUploadError = new InvalidOperationException("monitoring is not running"),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        var result = await viewModel.ForceCaptureAsync();

        Assert.Null(result);
        Assert.Equal("monitoring is not running", viewModel.ErrorText);
    }

    [Fact]
    public void TrayMenuController_RoutesReportBugEvent()
    {
        var host = new NullTrayIconHost();
        var controller = new TrayMenuController(host);
        var reportBugRaised = false;

        controller.ReportBugRequested += (_, _) => reportBugRaised = true;

        host.RequestReportBug();

        Assert.True(reportBugRaised);
    }

    [Fact]
    public void NullTrayIconHost_HasNoWindowHandle()
    {
        Assert.Equal(IntPtr.Zero, new NullTrayIconHost().WindowHandle);
    }

    [Fact]
    public void TrayMenuController_ForwardsWindowHandleFromHost()
    {
        var host = new FakeTrayIconHost { WindowHandle = new IntPtr(0x1234) };
        var controller = new TrayMenuController(host);

        Assert.Equal(new IntPtr(0x1234), controller.WindowHandle);
    }

    [Fact]
    public void TrayMenuController_RoutesForceCaptureEvent()
    {
        var host = new NullTrayIconHost();
        var controller = new TrayMenuController(host);
        var forceCaptureRaised = false;

        controller.ForceCaptureRequested += (_, _) => forceCaptureRaised = true;

        host.RequestForceCapture();

        Assert.True(forceCaptureRaised);
    }

    [Fact]
    public void ForceScreenshotAndUpload_InvokesInterop()
    {
        var fakeClient = new FakeRustInteropClient();

        fakeClient.ForceScreenshotAndUpload();

        Assert.True(fakeClient.ForceScreenshotAndUploadCalled);
    }

    [Fact]
    public void TrayMenuController_RoutesAllSystemEvents()
    {
        var host = new NullTrayIconHost();
        var controller = new TrayMenuController(host);
        var logoffRaised = false;
        var shutdownRaised = false;

        controller.SessionLogoffObserved += (_, _) => logoffRaised = true;
        controller.SystemShutdownObserved += (_, _) => shutdownRaised = true;

        host.RequestSessionLogoff();
        host.RequestSystemShutdown();

        Assert.True(logoffRaised);
        Assert.True(shutdownRaised);
    }

    [Fact]
    public async Task BackgroundRefreshAsync_DoesNotClearLoggedInState()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(true, "device-1", "user@example.com", "build-123"),
            MonitorStatus = new MonitorStatusPayload("running", true, 0, null, null),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234")
        {
            EmailInput = "user@example.com",
            PasswordInput = "secret",
        };
        await viewModel.LoginAsync();
        Assert.True(viewModel.LoggedIn);

        fakeClient.SessionStatus = fakeClient.SessionStatus with { LoggedIn = false };

        await viewModel.BackgroundRefreshAsync();

        Assert.True(viewModel.LoggedIn);
    }

    /// <summary>Minimal host used to prove <see cref="TrayMenuController"/> forwards its host's HWND.</summary>
    private sealed class FakeTrayIconHost : ITrayIconHost
    {
        public event EventHandler? OpenRequested;
        public event EventHandler? ExitRequested;
        public event EventHandler? ReportBugRequested;
        public event EventHandler? ForceCaptureRequested;
        public event EventHandler? SessionLogoffObserved;
        public event EventHandler? SystemShutdownObserved;

        public IntPtr WindowHandle { get; set; }

        public void Initialize()
        {
            _ = OpenRequested;
            _ = ExitRequested;
            _ = ReportBugRequested;
            _ = ForceCaptureRequested;
            _ = SessionLogoffObserved;
            _ = SystemShutdownObserved;
        }

        public void UpdateToolTip(string toolTip)
        {
        }

        public void ShowBalloonTip(string title, string text)
        {
        }

        public void SetForceCaptureAvailable(bool available)
        {
        }

        public void Dispose()
        {
        }
    }

    [Fact]
    public void NotifyUpdateInstalling_FlagsTheNoticeAndIsClearedByUnstaging()
    {
        var fakeClient = new FakeRustInteropClient();
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");
        var changed = new List<string?>();
        viewModel.PropertyChanged += (_, e) => changed.Add(e.PropertyName);

        viewModel.NotifyUpdateStaged();
        viewModel.NotifyUpdateInstalling();

        Assert.True(viewModel.UpdateInstalling);
        Assert.Contains(nameof(SessionViewModel.UpdateInstalling), changed);

        viewModel.NotifyUpdateUnstaged();

        Assert.False(viewModel.UpdateInstalling);
    }

    private sealed class FakeRustInteropClient : IRustInteropClient
    {
        public SessionStatusPayload SessionStatus { get; set; } = new(false, null, null, "build-unknown");

        public MonitorStatusPayload MonitorStatus { get; set; } = new("stopped", false, 0, null, null);

        public (string Email, string Password, string? DeviceName)? LastLogin { get; private set; }

        public bool StartMonitoringCalled { get; private set; }

        public bool StopMonitoringCalled { get; private set; }

        public bool StopMonitoringFromTrayExitCalled { get; private set; }

        public void Initialize()
        {
        }

        public void StartMonitoring()
        {
            StartMonitoringCalled = true;
        }

        public void StopMonitoring()
        {
            StopMonitoringCalled = true;
            MonitorStatus = MonitorStatus with { State = "stopped" };
        }

        public void StopMonitoringFromTrayExit()
        {
            StopMonitoringFromTrayExitCalled = true;
            MonitorStatus = MonitorStatus with { State = "stopped" };
        }

        public SessionStatusPayload GetSessionStatus() => SessionStatus;

        public MonitorStatusPayload GetMonitorStatus() => MonitorStatus;

        public void Login(string email, string password, string? deviceName = null)
        {
            LastLogin = (email, password, deviceName);
            SessionStatus = SessionStatus with { LoggedIn = true, Email = email };
            MonitorStatus = MonitorStatus with { State = "running", LoggedIn = true, LastError = null };
        }

        public void Logout()
        {
            SessionStatus = SessionStatus with { LoggedIn = false };
            MonitorStatus = MonitorStatus with { State = "signed_out", LoggedIn = false, LastError = null };
        }

        public (string Message, string? ContactEmail, bool IncludeLogs)? LastReportIssue { get; private set; }

        public Exception? ReportIssueError { get; set; }

        public void ReportIssue(string message, string? contactEmail, bool includeLogs)
        {
            if (ReportIssueError is not null)
            {
                throw ReportIssueError;
            }

            LastReportIssue = (message, contactEmail, includeLogs);
        }

        public bool ForceScreenshotAndUploadCalled { get; private set; }

        public Exception? ForceScreenshotAndUploadError { get; set; }

        public ForceCapturePayload ForceScreenshotAndUploadResult { get; set; } =
            new("uploaded", "Screenshot uploaded. Check the web logs page to view it.");

        public ForceCapturePayload ForceScreenshotAndUpload()
        {
            if (ForceScreenshotAndUploadError is not null)
            {
                throw ForceScreenshotAndUploadError;
            }

            ForceScreenshotAndUploadCalled = true;
            return ForceScreenshotAndUploadResult;
        }
    }
}
