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
    }
}
