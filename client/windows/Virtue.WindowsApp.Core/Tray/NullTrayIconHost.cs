namespace Virtue.WindowsApp.Core.Tray;

public sealed class NullTrayIconHost : ITrayIconHost
{
    public event EventHandler? OpenRequested;
    public event EventHandler? ExitRequested;
    public event EventHandler? ReportBugRequested;
    public event EventHandler? ForceCaptureRequested;
    public event EventHandler? SessionLogoffObserved;
    public event EventHandler? SystemShutdownObserved;

    public IntPtr WindowHandle => IntPtr.Zero;

    public void Initialize()
    {
    }

    public void UpdateToolTip(string toolTip)
    {
    }

    public void ShowBalloonTip(string title, string text)
    {
    }

    public void SetForceCaptureAvailable(bool available)
    {
    }

    public void RequestOpen() => OpenRequested?.Invoke(this, EventArgs.Empty);

    public void RequestExit() => ExitRequested?.Invoke(this, EventArgs.Empty);

    public void RequestReportBug() => ReportBugRequested?.Invoke(this, EventArgs.Empty);

    public void RequestForceCapture() => ForceCaptureRequested?.Invoke(this, EventArgs.Empty);

    public void RequestSessionLogoff() => SessionLogoffObserved?.Invoke(this, EventArgs.Empty);

    public void RequestSystemShutdown() => SystemShutdownObserved?.Invoke(this, EventArgs.Empty);

    public void Dispose()
    {
    }
}
