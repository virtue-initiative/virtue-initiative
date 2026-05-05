namespace Virtue.WindowsApp.Core.Interop;

public interface IRustInteropClient
{
    void Initialize(RuntimeConfigUpdate? overrides = null);
    void StartMonitoring();
    void StopMonitoring();
    void StopMonitoringFromTrayExit();
    SessionStatusPayload GetSessionStatus();
    MonitorStatusPayload GetMonitorStatus();
    RuntimeConfigPayload GetRuntimeConfig();
    void SetRuntimeConfig(RuntimeConfigUpdate update);
    void Login(string email, string password, string? deviceName = null);
    void Logout();
}
