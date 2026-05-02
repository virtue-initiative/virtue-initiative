namespace Virtue.WindowsApp.Core.Tray;

public interface ITrayIconHost : IDisposable
{
    event EventHandler? OpenRequested;
    event EventHandler? ExitRequested;
    event EventHandler? SessionLogonObserved;
    event EventHandler? SessionLogoffObserved;
    event EventHandler? SystemShutdownObserved;
    event EventHandler? SuspendObserved;
    event EventHandler? ResumeObserved;

    void Initialize();
    void UpdateToolTip(string toolTip);
}
