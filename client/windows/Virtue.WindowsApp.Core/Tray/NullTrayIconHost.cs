namespace Virtue.WindowsApp.Core.Tray;

public sealed class NullTrayIconHost : ITrayIconHost
{
    public event EventHandler? OpenRequested;
    public event EventHandler? ExitRequested;
    public event EventHandler? ReportBugRequested;
    public event EventHandler? RestartToUpdateRequested;
    public event EventHandler? SessionLogoffObserved;
    public event EventHandler? SystemShutdownObserved;

    public void Initialize()
    {
    }

    public void UpdateToolTip(string toolTip)
    {
    }

    public void RequestOpen() => OpenRequested?.Invoke(this, EventArgs.Empty);

    public void RequestExit() => ExitRequested?.Invoke(this, EventArgs.Empty);

    public void RequestReportBug() => ReportBugRequested?.Invoke(this, EventArgs.Empty);

    public void RequestRestartToUpdate() => RestartToUpdateRequested?.Invoke(this, EventArgs.Empty);

    public void RequestSessionLogoff() => SessionLogoffObserved?.Invoke(this, EventArgs.Empty);

    public void RequestSystemShutdown() => SystemShutdownObserved?.Invoke(this, EventArgs.Empty);

    public void Dispose()
    {
    }
}
