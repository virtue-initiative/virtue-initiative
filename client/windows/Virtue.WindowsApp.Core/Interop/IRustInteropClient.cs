namespace Virtue.WindowsApp.Core.Interop;

public interface IRustInteropClient
{
    void Initialize();
    void StartMonitoring();
    void StopMonitoring();
    void StopMonitoringFromTrayExit();
    SessionStatusPayload GetSessionStatus();
    MonitorStatusPayload GetMonitorStatus();
    /// <param name="server">A self-hosted server address, or null for the default server (CLIENT-034).</param>
    void Login(string email, string password, string? deviceName = null, string? server = null);
    void Logout();
    ForceCapturePayload ForceScreenshotAndUpload();
    void ReportIssue(string message, string? contactEmail, bool includeLogs);
}
