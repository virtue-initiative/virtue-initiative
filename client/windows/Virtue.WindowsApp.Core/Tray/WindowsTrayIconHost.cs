using System.Runtime.InteropServices;
namespace Virtue.WindowsApp.Core.Tray;

public sealed class WindowsTrayIconHost : ITrayIconHost
{
    private const int EndSessionLogoff = unchecked((int)0x80000000);
    private const int NifMessage = 0x00000001;
    private const int NifIcon = 0x00000002;
    private const int NifTip = 0x00000004;
    private const int NimAdd = 0x00000000;
    private const int NimModify = 0x00000001;
    private const int NimDelete = 0x00000002;
    private const int WmApp = 0x8000;
    private const int WmCommand = 0x0111;
    private const int WmDestroy = 0x0002;
    private const int WmEndSession = 0x0016;
    private const int WmLButtonUp = 0x0202;
    private const int WmRButtonUp = 0x0205;
    private const int TpmLeftAlign = 0x0000;
    private const int TpmRightButton = 0x0002;
    private const int MfString = 0x0000;
    private const int ImageIcon = 1;
    private const int LrDefaultSize = 0x00000040;
    private const int LrLoadFromFile = 0x00000010;
    private const int IdTrayOpen = 2001;
    private const int IdTrayExit = 2002;
    private const int IdTrayReportBug = 2003;
    private const int WindowId = 1;
    private static readonly uint WmTrayIcon = WmApp + 1;

    private readonly WndProc _wndProc;
    private readonly string _windowClassName = $"VirtueTrayWindow_{Guid.NewGuid():N}";
    private IntPtr _windowHandle;
    private IntPtr _menuHandle;
    private IntPtr _iconHandle;
    private bool _initialized;
    private bool _iconAdded;
    private uint _taskbarCreatedMessage;
    private string _toolTip = "Virtue";

    public WindowsTrayIconHost()
    {
        _wndProc = WindowProc;
    }

    public event EventHandler? OpenRequested;
    public event EventHandler? ExitRequested;
    public event EventHandler? ReportBugRequested;
    public event EventHandler? SessionLogoffObserved;
    public event EventHandler? SystemShutdownObserved;

    public void Initialize()
    {
        if (_initialized)
        {
            return;
        }

        _taskbarCreatedMessage = RegisterWindowMessage("TaskbarCreated");
        RegisterWindowClass();
        _windowHandle = CreateWindowEx(
            0,
            _windowClassName,
            "Virtue Tray",
            0,
            0,
            0,
            0,
            0,
            IntPtr.Zero,
            IntPtr.Zero,
            GetModuleHandle(null),
            IntPtr.Zero);

        if (_windowHandle == IntPtr.Zero)
        {
            throw new InvalidOperationException("Failed to create tray host window.");
        }

        _menuHandle = CreatePopupMenu();
        _ = AppendMenu(_menuHandle, MfString, (UIntPtr)IdTrayOpen, "Open");
        _ = AppendMenu(_menuHandle, MfString, (UIntPtr)IdTrayReportBug, "Report a Bug");
        _ = AppendMenu(_menuHandle, MfString, (UIntPtr)IdTrayExit, "Exit");

        AddOrUpdateIcon(NimAdd);
        _initialized = true;
    }

    public void UpdateToolTip(string toolTip)
    {
        _toolTip = string.IsNullOrWhiteSpace(toolTip) ? "Virtue" : toolTip.Trim();
        if (_iconAdded)
        {
            AddOrUpdateIcon(NimModify);
        }
    }

    public void Dispose()
    {
        RemoveIcon();
        if (_menuHandle != IntPtr.Zero)
        {
            DestroyMenu(_menuHandle);
            _menuHandle = IntPtr.Zero;
        }

        if (_windowHandle != IntPtr.Zero)
        {
            DestroyWindow(_windowHandle);
            _windowHandle = IntPtr.Zero;
        }

        if (_iconHandle != IntPtr.Zero)
        {
            DestroyIcon(_iconHandle);
            _iconHandle = IntPtr.Zero;
        }
    }

    private void RegisterWindowClass()
    {
        var windowClass = new WndClass
        {
            lpfnWndProc = Marshal.GetFunctionPointerForDelegate(_wndProc),
            lpszClassName = _windowClassName,
            hInstance = GetModuleHandle(null),
        };

        var atom = RegisterClass(ref windowClass);
        if (atom == 0)
        {
            throw new InvalidOperationException("Failed to register tray host window class.");
        }
    }

    private void AddOrUpdateIcon(int message)
    {
        var data = new NotifyIconData
        {
            cbSize = Marshal.SizeOf<NotifyIconData>(),
            hWnd = _windowHandle,
            uID = WindowId,
            uFlags = NifMessage | NifIcon | NifTip,
            uCallbackMessage = WmTrayIcon,
            hIcon = EnsureIconHandle(),
            szTip = BuildToolTip(_toolTip),
            szInfo = string.Empty,
            szInfoTitle = string.Empty,
        };

        _iconAdded = Shell_NotifyIcon(message, ref data);
    }

    private void RemoveIcon()
    {
        if (!_iconAdded || _windowHandle == IntPtr.Zero)
        {
            return;
        }

        var data = new NotifyIconData
        {
            cbSize = Marshal.SizeOf<NotifyIconData>(),
            hWnd = _windowHandle,
            uID = WindowId,
        };

        _ = Shell_NotifyIcon(NimDelete, ref data);
        _iconAdded = false;
    }

    private IntPtr EnsureIconHandle()
    {
        if (_iconHandle != IntPtr.Zero)
        {
            return _iconHandle;
        }

        foreach (var candidate in ResolveIconCandidates())
        {
            if (!File.Exists(candidate))
            {
                continue;
            }

            var loaded = LoadImage(
                IntPtr.Zero,
                candidate,
                ImageIcon,
                0,
                0,
                LrLoadFromFile | LrDefaultSize);
            if (loaded != IntPtr.Zero)
            {
                _iconHandle = loaded;
                return _iconHandle;
            }
        }

        _iconHandle = LoadIcon(IntPtr.Zero, (IntPtr)0x7F00);
        return _iconHandle;
    }

    private static IEnumerable<string> ResolveIconCandidates()
    {
        yield return Path.Combine(AppContext.BaseDirectory, "app-icon.ico");
        yield return Path.Combine(AppContext.BaseDirectory, "Assets", "app-icon.ico");
    }

    private static string BuildToolTip(string toolTip)
    {
        const int maxChars = 127;
        return toolTip.Length <= maxChars ? toolTip : toolTip[..maxChars];
    }

    private IntPtr WindowProc(IntPtr hwnd, uint msg, IntPtr wParam, IntPtr lParam)
    {
        if (msg == _taskbarCreatedMessage)
        {
            _iconAdded = false;
            AddOrUpdateIcon(NimAdd);
            return IntPtr.Zero;
        }

        switch (msg)
        {
            case WmCommand:
            {
                var command = unchecked((ushort)(wParam.ToInt64() & 0xFFFF));
                if (command == IdTrayOpen)
                {
                    OpenRequested?.Invoke(this, EventArgs.Empty);
                }
                else if (command == IdTrayReportBug)
                {
                    ReportBugRequested?.Invoke(this, EventArgs.Empty);
                }
                else if (command == IdTrayExit)
                {
                    ExitRequested?.Invoke(this, EventArgs.Empty);
                }

                return IntPtr.Zero;
            }
            case WmDestroy:
                RemoveIcon();
                return IntPtr.Zero;
            case WmEndSession:
                if (wParam != IntPtr.Zero)
                {
                    var isLogoff = (unchecked((int)lParam.ToInt64()) & EndSessionLogoff) != 0;
                    if (isLogoff)
                    {
                        SessionLogoffObserved?.Invoke(this, EventArgs.Empty);
                    }
                    else
                    {
                        SystemShutdownObserved?.Invoke(this, EventArgs.Empty);
                    }
                }

                return IntPtr.Zero;
        }

        if (msg == WmTrayIcon)
        {
            var eventId = lParam.ToInt32();
            if (eventId == WmLButtonUp)
            {
                OpenRequested?.Invoke(this, EventArgs.Empty);
            }
            else if (eventId == WmRButtonUp)
            {
                ShowContextMenu();
            }

            return IntPtr.Zero;
        }

        return DefWindowProc(hwnd, msg, wParam, lParam);
    }

    private void ShowContextMenu()
    {
        if (_windowHandle == IntPtr.Zero || _menuHandle == IntPtr.Zero)
        {
            return;
        }

        if (!GetCursorPos(out var point))
        {
            return;
        }

        _ = SetForegroundWindow(_windowHandle);
        _ = TrackPopupMenu(_menuHandle, TpmLeftAlign | TpmRightButton, point.X, point.Y, 0, _windowHandle, IntPtr.Zero);
    }

    private delegate IntPtr WndProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct WndClass
    {
        public uint style;
        public IntPtr lpfnWndProc;
        public int cbClsExtra;
        public int cbWndExtra;
        public IntPtr hInstance;
        public IntPtr hIcon;
        public IntPtr hCursor;
        public IntPtr hbrBackground;
        [MarshalAs(UnmanagedType.LPWStr)]
        public string? lpszMenuName;
        [MarshalAs(UnmanagedType.LPWStr)]
        public string lpszClassName;
    }

    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct NotifyIconData
    {
        public int cbSize;
        public IntPtr hWnd;
        public int uID;
        public int uFlags;
        public uint uCallbackMessage;
        public IntPtr hIcon;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 128)]
        public string szTip;
        public int dwState;
        public int dwStateMask;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 256)]
        public string szInfo;
        public uint uTimeoutOrVersion;
        [MarshalAs(UnmanagedType.ByValTStr, SizeConst = 64)]
        public string szInfoTitle;
        public int dwInfoFlags;
        public Guid guidItem;
        public IntPtr hBalloonIcon;
    }

    [StructLayout(LayoutKind.Sequential)]
    private struct Point
    {
        public int X;
        public int Y;
    }

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern ushort RegisterClass(ref WndClass lpWndClass);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr CreateWindowEx(
        int dwExStyle,
        string lpClassName,
        string lpWindowName,
        int dwStyle,
        int x,
        int y,
        int nWidth,
        int nHeight,
        IntPtr hWndParent,
        IntPtr hMenu,
        IntPtr hInstance,
        IntPtr lpParam);

    [DllImport("user32.dll")]
    private static extern IntPtr DefWindowProc(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool DestroyWindow(IntPtr hWnd);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern IntPtr CreatePopupMenu();

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool AppendMenu(IntPtr hMenu, uint uFlags, UIntPtr uIDNewItem, string lpNewItem);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool DestroyMenu(IntPtr hMenu);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool TrackPopupMenu(
        IntPtr hMenu,
        uint uFlags,
        int x,
        int y,
        int nReserved,
        IntPtr hWnd,
        IntPtr prcRect);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool SetForegroundWindow(IntPtr hWnd);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool GetCursorPos(out Point lpPoint);

    [DllImport("user32.dll", CharSet = CharSet.Unicode)]
    private static extern uint RegisterWindowMessage(string lpString);

    [DllImport("shell32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool Shell_NotifyIcon(int dwMessage, ref NotifyIconData lpData);

    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr GetModuleHandle(string? lpModuleName);

    [DllImport("user32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern IntPtr LoadImage(
        IntPtr hInst,
        string lpszName,
        uint uType,
        int cxDesired,
        int cyDesired,
        uint fuLoad);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern IntPtr LoadIcon(IntPtr hInstance, IntPtr lpIconName);

    [DllImport("user32.dll", SetLastError = true)]
    private static extern bool DestroyIcon(IntPtr hIcon);
}
