namespace Virtue.WindowsApp.Core.Tray;

public interface ITrayIconHost : IDisposable
{
    event EventHandler? OpenRequested;
    event EventHandler? ExitRequested;
    event EventHandler? ReportBugRequested;
    event EventHandler? ForceCaptureRequested;
    event EventHandler? SessionLogoffObserved;
    event EventHandler? SystemShutdownObserved;

    /// <summary>
    /// The host's hidden top-level window, or <see cref="IntPtr.Zero"/> when there isn't one.
    /// In resident/no-window mode — the app's normal state — this is the only HWND the process
    /// has, so it's what WinRT APIs that require an owner window (notably <c>StoreContext</c>)
    /// are initialized against. Deliberately a raw <see cref="IntPtr"/> so this assembly stays
    /// on plain <c>net8.0</c> with no WinRT projection.
    /// </summary>
    IntPtr WindowHandle { get; }

    void Initialize();
    void UpdateToolTip(string toolTip);
    void ShowBalloonTip(string title, string text);
    void SetForceCaptureAvailable(bool available);
}
