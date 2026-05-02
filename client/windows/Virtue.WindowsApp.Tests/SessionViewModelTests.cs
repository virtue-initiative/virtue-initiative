using Virtue.WindowsApp.Core.Interop;
using Virtue.WindowsApp.Core.Tray;
using Virtue.WindowsApp.Core.ViewModels;
using Xunit;

namespace Virtue.WindowsApp.Tests;

public sealed class SessionViewModelTests
{
    [Fact]
    public async Task InitializeAsync_LoadsSessionAndRuntimeConfig()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(false, null, "user@example.com", "build-123"),
            MonitorStatus = new MonitorStatusPayload("signed_out", false, 0, null, null),
            RuntimeConfig = new RuntimeConfigPayload("https://api.example.com", 45, 90, @"C:\ProgramData\Virtue\config\config.json", "build-123"),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234");

        await viewModel.InitializeAsync();

        Assert.Equal("build-123", viewModel.BuildLabel);
        Assert.Equal("Build build-123 | Windows package 0.0.5.1234", viewModel.BuildLabelText);
        Assert.Equal("user@example.com", viewModel.EmailInput);
        Assert.Equal("https://api.example.com", viewModel.ApiBaseUrl);
        Assert.Equal("45", viewModel.CaptureIntervalSeconds);
        Assert.Equal("90", viewModel.BatchWindowSeconds);
        Assert.Equal(@"C:\ProgramData\Virtue\config\config.json", viewModel.ConfigPath);
        Assert.Equal(@"C:\ProgramData\Virtue\config\config.json", viewModel.ConfigPathDisplay);
        Assert.Equal("Sign in to start monitoring.", viewModel.StatusText);
    }

    [Fact]
    public async Task LoginAsync_RefreshesStatusAfterSuccess()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(true, "device-1", "user@example.com", "build-123"),
            MonitorStatus = new MonitorStatusPayload("running", true, 0, 123, null),
            RuntimeConfig = new RuntimeConfigPayload("https://api.example.com", 60, 120, @"C:\ProgramData\Virtue\config\config.json", "build-123"),
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
    public async Task SaveSettingsAsync_PassesRuntimeConfigUpdateToInterop()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(false, null, null, "build-123"),
            MonitorStatus = new MonitorStatusPayload("signed_out", false, 0, null, null),
            RuntimeConfig = new RuntimeConfigPayload("https://api.example.com", 45, 90, @"C:\ProgramData\Virtue\config\config.json", "build-123"),
        };
        var viewModel = new SessionViewModel(fakeClient, "0.0.5.1234")
        {
            ApiBaseUrl = "https://dev-api.example.com",
            CaptureIntervalSeconds = "30",
            BatchWindowSeconds = "180",
        };

        await viewModel.SaveSettingsAsync();

        Assert.Equal(new RuntimeConfigUpdate("https://dev-api.example.com", 30, 180), fakeClient.LastRuntimeConfigUpdate);
        Assert.Equal("Runtime settings saved.", viewModel.StatusText);
    }

    [Fact]
    public async Task StopMonitoringFromTrayExitAsync_UsesExplicitTrayExitInterop()
    {
        var fakeClient = new FakeRustInteropClient
        {
            SessionStatus = new SessionStatusPayload(true, "device-1", "user@example.com", "build-123"),
            MonitorStatus = new MonitorStatusPayload("running", true, 0, 123, null),
            RuntimeConfig = new RuntimeConfigPayload("https://api.example.com", 60, 120, @"C:\ProgramData\Virtue\config\config.json", "build-123"),
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
            RuntimeConfig = new RuntimeConfigPayload("https://api.example.com", 60, 120, @"C:\ProgramData\Virtue\config\config.json", "build-123"),
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
            RuntimeConfig = new RuntimeConfigPayload("https://api.example.com", 60, 120, @"C:\ProgramData\Virtue\config\config.json", "build-123"),
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

    private sealed class FakeRustInteropClient : IRustInteropClient
    {
        public SessionStatusPayload SessionStatus { get; set; } = new(false, null, null, "build-unknown");

        public MonitorStatusPayload MonitorStatus { get; set; } = new("stopped", false, 0, null, null);

        public RuntimeConfigPayload RuntimeConfig { get; set; } = new(string.Empty, 300, 3600, string.Empty, "build-unknown");

        public (string Email, string Password, string? DeviceName)? LastLogin { get; private set; }

        public RuntimeConfigUpdate? LastRuntimeConfigUpdate { get; private set; }

        public bool StartMonitoringCalled { get; private set; }

        public bool StopMonitoringCalled { get; private set; }

        public bool StopMonitoringFromTrayExitCalled { get; private set; }

        public void Initialize(RuntimeConfigUpdate? overrides = null)
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

        public RuntimeConfigPayload GetRuntimeConfig() => RuntimeConfig;

        public void SetRuntimeConfig(RuntimeConfigUpdate update)
        {
            LastRuntimeConfigUpdate = update;
            RuntimeConfig = new RuntimeConfigPayload(
                update.ApiBaseUrl ?? RuntimeConfig.ApiBaseUrl,
                update.CaptureIntervalSeconds ?? RuntimeConfig.CaptureIntervalSeconds,
                update.BatchWindowSeconds ?? RuntimeConfig.BatchWindowSeconds,
                RuntimeConfig.ConfigPath,
                RuntimeConfig.BuildLabel);
        }

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
