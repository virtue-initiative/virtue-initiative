namespace Virtue.WindowsApp.Core.Interop;

public interface IRustInteropClient
{
    void Initialize();
    void StartMonitoring();
    void StopMonitoring();
    void StopMonitoringFromTrayExit();
    SessionStatusPayload GetSessionStatus();
    MonitorStatusPayload GetMonitorStatus();
    void Login(string email, string password, string? deviceName = null);
    BeginCodeLoginPayload BeginCodeLogin(string? deviceName = null);
    PollCodeLoginPayload PollCodeLogin();
    void Logout();
    ForceCapturePayload ForceScreenshotAndUpload();
    void ReportIssue(string message, string? contactEmail, bool includeLogs);
}
