namespace Virtue.WindowsApp.Core.Tray;

public interface ITrayIconHost : IDisposable
{
    event EventHandler? OpenRequested;
    event EventHandler? ExitRequested;
    event EventHandler? ReportBugRequested;
    event EventHandler? RestartToUpdateRequested;
    event EventHandler? ForceCaptureRequested;
    event EventHandler? SessionLogoffObserved;
    event EventHandler? SystemShutdownObserved;

    void Initialize();
    void UpdateToolTip(string toolTip);
    void ShowBalloonTip(string title, string text);
    void SetForceCaptureAvailable(bool available);
    void SetRestartToUpdateAvailable(bool available);
}
