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
    void Logout();
    ForceCapturePayload ForceScreenshotAndUpload();
    void ReportIssue(string message, string? contactEmail, bool includeLogs);
}
