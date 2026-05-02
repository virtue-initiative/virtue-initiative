namespace Virtue.WindowsApp.Core.Tray;

public sealed class NullTrayIconHost : ITrayIconHost
{
    public event EventHandler? OpenRequested;
    public event EventHandler? ExitRequested;
    public event EventHandler? SessionLogonObserved;
    public event EventHandler? SessionLogoffObserved;
    public event EventHandler? SystemShutdownObserved;
    public event EventHandler? SuspendObserved;
    public event EventHandler? ResumeObserved;

    public void Initialize()
    {
    }

    public void UpdateToolTip(string toolTip)
    {
    }

    public void RequestOpen() => OpenRequested?.Invoke(this, EventArgs.Empty);

    public void RequestExit() => ExitRequested?.Invoke(this, EventArgs.Empty);

    public void RequestSessionLogon() => SessionLogonObserved?.Invoke(this, EventArgs.Empty);

    public void RequestSessionLogoff() => SessionLogoffObserved?.Invoke(this, EventArgs.Empty);

    public void RequestSystemShutdown() => SystemShutdownObserved?.Invoke(this, EventArgs.Empty);

    public void RequestSuspend() => SuspendObserved?.Invoke(this, EventArgs.Empty);

    public void RequestResume() => ResumeObserved?.Invoke(this, EventArgs.Empty);

    public void Dispose()
    {
    }
}
