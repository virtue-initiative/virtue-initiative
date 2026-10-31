using Virtue.WindowsApp.Core.Interop;
using Virtue.WindowsApp.Core.Tray;
using Virtue.WindowsApp.Core.ViewModels;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class SessionViewModelTests
{
    // CLIENT-027: before the first status load the app shows Loading... and the loading state.
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

    // CLIENT-027: the tray tooltip gains " (update ready)" while an update is staged.
    [Fact]
    public void NotifyUpdateStaged_AppendsUpdateReadySuffixToTrayTooltip()
    {
        var fakeClient = new FakeRustInteropClient();
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        viewModel.NotifyUpdateStaged();

        Assert.True(viewModel.UpdateReady);
        Assert.Equal("Virtue: loading status (update ready)", viewModel.TrayTooltip);
    }

    // CLIENT-029: the restart countdown text is exposed to the window (design test).
    [Fact]
    public void SetUpdateCountdownText_UpdatesProperty()
    {
        var fakeClient = new FakeRustInteropClient();
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        Assert.Null(viewModel.UpdateCountdownText);

        viewModel.SetUpdateCountdownText("42m");

        Assert.Equal("42m", viewModel.UpdateCountdownText);
    }

    // CLIENT-029: leaving the staged state clears the notice, countdown and tooltip suffix.
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

    // CLIENT-029: the update check line is exposed to the window (design test).
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

    // CLIENT-004: sign-in uses the machine name as the default device name, clears the password and refreshes status.
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

    // CLIENT-007: Status Details receives every CORE-010 field.
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

    // CLIENT-027: the device id and email fall back to the daemon's values.
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

    // CLIENT-011: tray Exit stops monitoring through its own tagged call.
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

    // CLIENT-027: signed out clears the device id, error, queue count and last screenshot but keeps the typed email.
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

    // CLIENT-027: a signed-in session whose monitor reports signed out shows starting.
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

    // CLIENT-027: the tray menu offers Open and Exit (design test).
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

    // CLIENT-015: a status payload that is not JSON surfaces as an error.
    [Fact]
    public void RustInteropJson_ThrowsForNonJsonPayload()
    {
        var ex = Assert.Throws<InvalidOperationException>(() => RustInteropJson.DeserializePayload<SessionStatusPayload>("login failed"));
        Assert.Contains("non-JSON payload", ex.Message);
    }

    // CLIENT-015: session status JSON carries loggedIn, deviceId, email and buildLabel.
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

    // CLIENT-004: sign-in without an email is refused before any request.
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

    // CLIENT-004: sign-in without a password is refused before any request.
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

    // CLIENT-005: signing out reads as signed out.
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

    // CLIENT-011: Stop Monitoring stops monitoring and the status reads stopped.
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

    // CLIENT-027: the tray tooltip text for each monitor state.
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

    // CLIENT-027: displayed states show underscores as spaces.
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

    // CLIENT-027: once loaded, signed in reads "Yes" and the account is the email.
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

    // CLIENT-027: once loaded, signed out reads "No" and "Not signed in".
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

    // CLIENT-027: the build line without a package version reads "Build unknown" before load.
    [Fact]
    public void BuildLabelText_WithNoWindowsPackageVersion()
    {
        var fakeClient = new FakeRustInteropClient();
        var viewModel = new SessionViewModel(fakeClient);

        Assert.Equal("Build unknown", viewModel.BuildLabelText);
    }

    // CLIENT-027: the email field keeps what the user typed while signed out.
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

    // CLIENT-027: the email field follows the account while signed in.
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

    // CLIENT-016: a report is sent with its message, contact email and log choice.
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

    // CLIENT-016: a failed report shows the error.
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

    // CLIENT-008: Test Screenshot requests a capture and reports the uploaded outcome.
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

    // CLIENT-008: a not-captured outcome and its message are passed through.
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

    // CLIENT-008: a failed Test Screenshot request shows "Test screenshot failed: <error>".
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
        Assert.Equal("Test screenshot failed: monitoring is not running", viewModel.ErrorText);
    }

    // CLIENT-027: the tray menu offers Report an Issue (design test).
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

    // CLIENT-027: where no tray icon can be hosted, the tray has no window handle.
    [Fact]
    public void NullTrayIconHost_HasNoWindowHandle()
    {
        Assert.Equal(IntPtr.Zero, new NullTrayIconHost().WindowHandle);
    }

    // CLIENT-027: the tray exposes its host's window handle (design test).
    [Fact]
    public void TrayMenuController_ForwardsWindowHandleFromHost()
    {
        var host = new FakeTrayIconHost { WindowHandle = new IntPtr(0x1234) };
        var controller = new TrayMenuController(host);

        Assert.Equal(new IntPtr(0x1234), controller.WindowHandle);
    }

    // CLIENT-008: the tray menu offers Test Screenshot (design test).
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

    // CLIENT-008: Test Screenshot calls the native capture (design test, exercises only the test fake).
    [Fact]
    public void ForceScreenshotAndUpload_InvokesInterop()
    {
        var fakeClient = new FakeRustInteropClient();

        fakeClient.ForceScreenshotAndUpload();

        Assert.True(fakeClient.ForceScreenshotAndUploadCalled);
    }

    // CLIENT-027: the tray reports session logoff and system shutdown (CLIENT-012).
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

    // CLIENT-027: the periodic refresh does not change the signed-in state.
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

    // CLIENT-029: the installing state is shown for the staged update and cleared when it is unstaged.
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
