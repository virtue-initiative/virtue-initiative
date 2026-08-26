namespace Virtue.WindowsApp.Core.Tray;

public sealed class TrayMenuController : IDisposable
{
    private readonly ITrayIconHost _host;

    public TrayMenuController(ITrayIconHost? host = null)
    {
        _host = host ?? BuildDefaultHost();
        _host.OpenRequested += (_, _) => OpenRequested?.Invoke(this, EventArgs.Empty);
        _host.ExitRequested += (_, _) => ExitRequested?.Invoke(this, EventArgs.Empty);
        _host.ReportBugRequested += (_, _) => ReportBugRequested?.Invoke(this, EventArgs.Empty);
        _host.ForceCaptureRequested += (_, _) => ForceCaptureRequested?.Invoke(this, EventArgs.Empty);
        _host.SessionLogoffObserved += (_, _) => SessionLogoffObserved?.Invoke(this, EventArgs.Empty);
        _host.SystemShutdownObserved += (_, _) => SystemShutdownObserved?.Invoke(this, EventArgs.Empty);
    }

    public event EventHandler? OpenRequested;
    public event EventHandler? ExitRequested;
    public event EventHandler? ReportBugRequested;
    public event EventHandler? ForceCaptureRequested;
    public event EventHandler? SessionLogoffObserved;
    public event EventHandler? SystemShutdownObserved;

    /// <summary>The tray host's hidden window — see <see cref="ITrayIconHost.WindowHandle"/>.</summary>
    public IntPtr WindowHandle => _host.WindowHandle;

    public void Initialize() => _host.Initialize();

    public void UpdateToolTip(string toolTip) => _host.UpdateToolTip(toolTip);

    public void ShowBalloonTip(string title, string text) => _host.ShowBalloonTip(title, text);

    public void SetForceCaptureAvailable(bool available) => _host.SetForceCaptureAvailable(available);

    public void RequestOpen() => OpenRequested?.Invoke(this, EventArgs.Empty);

    public void RequestExit() => ExitRequested?.Invoke(this, EventArgs.Empty);

    public void Dispose() => _host.Dispose();

    private static ITrayIconHost BuildDefaultHost() =>
        OperatingSystem.IsWindows() ? new WindowsTrayIconHost() : new NullTrayIconHost();
}
