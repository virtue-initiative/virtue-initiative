namespace Virtue.WindowsApp.Core.Tray;

public interface ITrayIconHost : IDisposable
{
    event EventHandler? OpenRequested;
    event EventHandler? ExitRequested;
    event EventHandler? SessionLogoffObserved;
    event EventHandler? SystemShutdownObserved;

    void Initialize();
    void UpdateToolTip(string toolTip);
}
