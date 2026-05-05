using System.Reflection;
using System.Runtime.InteropServices;
using System.Text.Json;

namespace Virtue.WindowsApp.Core.Interop;

public sealed class RustInteropClient : IRustInteropClient
{
    private const string NativeLibraryName = "virtue_windows";
    private static readonly object ResolverLock = new();
    private static bool _resolverInstalled;

    public RustInteropClient()
    {
        EnsureResolverInstalled();
    }

    public void Initialize(RuntimeConfigUpdate? overrides = null)
    {
        var error = NativeMethods.virtue_windows_init(
            overrides?.ApiBaseUrl ?? string.Empty,
            overrides?.CaptureIntervalSeconds?.ToString() ?? string.Empty,
            overrides?.BatchWindowSeconds?.ToString() ?? string.Empty);
        ThrowIfError(error);
    }

    public void StartMonitoring()
    {
        var error = NativeMethods.virtue_windows_start_monitoring();
        ThrowIfError(error);
    }

    public void StopMonitoring()
    {
        var error = NativeMethods.virtue_windows_stop_monitoring();
        ThrowIfError(error);
    }

    public void StopMonitoringFromTrayExit()
    {
        var error = NativeMethods.virtue_windows_stop_monitoring_from_tray_exit();
        ThrowIfError(error);
    }

    public void StopMonitoringForSystemShutdown()
    {
        var error = NativeMethods.virtue_windows_stop_monitoring_for_system_shutdown();
        ThrowIfError(error);
    }

    public void StopMonitoringForSessionLogoff()
    {
        var error = NativeMethods.virtue_windows_stop_monitoring_for_session_logoff();
        ThrowIfError(error);
    }

    public void NotifySessionLogon()
    {
        var error = NativeMethods.virtue_windows_notify_session_logon();
        ThrowIfError(error);
    }

    public void NotifySessionLogoff()
    {
        var error = NativeMethods.virtue_windows_notify_session_logoff();
        ThrowIfError(error);
    }

    public void NotifySuspend()
    {
        var error = NativeMethods.virtue_windows_notify_suspend();
        ThrowIfError(error);
    }

    public void NotifyResume()
    {
        var error = NativeMethods.virtue_windows_notify_resume();
        ThrowIfError(error);
    }

    public SessionStatusPayload GetSessionStatus() =>
        ReadJsonPayload<SessionStatusPayload>(NativeMethods.virtue_windows_get_session_status_json());

    public MonitorStatusPayload GetMonitorStatus() =>
        ReadJsonPayload<MonitorStatusPayload>(NativeMethods.virtue_windows_get_monitor_status_json());

    public RuntimeConfigPayload GetRuntimeConfig() =>
        ReadJsonPayload<RuntimeConfigPayload>(NativeMethods.virtue_windows_get_runtime_config_json());

    public void SetRuntimeConfig(RuntimeConfigUpdate update)
    {
        var payload = RustInteropJson.Serialize(update);
        var error = NativeMethods.virtue_windows_set_runtime_config_json(payload);
        ThrowIfError(error);
    }

    public void Login(string email, string password, string? deviceName = null)
    {
        var payload = RustInteropJson.Serialize(new LoginRequest(email, password, deviceName));
        var error = NativeMethods.virtue_windows_login(payload);
        ThrowIfError(error);
    }

    public void Logout()
    {
        var error = NativeMethods.virtue_windows_logout();
        ThrowIfError(error);
    }

    private static T ReadJsonPayload<T>(IntPtr pointer)
    {
        var raw = TakeOwnedUtf8(pointer);
        return RustInteropJson.DeserializePayload<T>(raw);
    }

    private static void ThrowIfError(IntPtr pointer)
    {
        if (pointer == IntPtr.Zero)
        {
            return;
        }

        var error = TakeOwnedUtf8(pointer);
        throw new InvalidOperationException(error);
    }

    private static string TakeOwnedUtf8(IntPtr pointer)
    {
        if (pointer == IntPtr.Zero)
        {
            return string.Empty;
        }

        try
        {
            return Marshal.PtrToStringUTF8(pointer) ?? string.Empty;
        }
        finally
        {
            NativeMethods.virtue_windows_free_string(pointer);
        }
    }

    private static void EnsureResolverInstalled()
    {
        lock (ResolverLock)
        {
            if (_resolverInstalled)
            {
                return;
            }

            NativeLibrary.SetDllImportResolver(typeof(RustInteropClient).Assembly, ResolveNativeLibrary);
            _resolverInstalled = true;
        }
    }

    private static IntPtr ResolveNativeLibrary(string libraryName, Assembly assembly, DllImportSearchPath? searchPath)
    {
        if (!string.Equals(libraryName, NativeLibraryName, StringComparison.Ordinal))
        {
            return IntPtr.Zero;
        }

        if (!OperatingSystem.IsWindows())
        {
            throw new PlatformNotSupportedException("The Windows interop library can only be loaded on Windows.");
        }

        var candidates = new[]
        {
            Environment.GetEnvironmentVariable("VIRTUE_WINDOWS_DLL_PATH"),
            Path.Combine(AppContext.BaseDirectory, "Payload", "virtue_windows.dll"),
            Path.Combine(AppContext.BaseDirectory, "virtue_windows.dll"),
        };

        foreach (var candidate in candidates.Where(static path => !string.IsNullOrWhiteSpace(path)))
        {
            if (File.Exists(candidate) && NativeLibrary.TryLoad(candidate!, out var handle))
            {
                return handle;
            }
        }

        return NativeLibrary.Load(NativeLibraryName, assembly, searchPath);
    }

    private static class NativeMethods
    {
        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_init(
            [MarshalAs(UnmanagedType.LPUTF8Str)] string apiBaseUrl,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string captureIntervalSeconds,
            [MarshalAs(UnmanagedType.LPUTF8Str)] string batchWindowSeconds);

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_get_session_status_json();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_start_monitoring();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_stop_monitoring();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_stop_monitoring_from_tray_exit();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_stop_monitoring_for_system_shutdown();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_stop_monitoring_for_session_logoff();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_notify_session_logon();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_notify_session_logoff();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_notify_suspend();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_notify_resume();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_get_monitor_status_json();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_login(
            [MarshalAs(UnmanagedType.LPUTF8Str)] string requestJson);

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_logout();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_get_runtime_config_json();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_set_runtime_config_json(
            [MarshalAs(UnmanagedType.LPUTF8Str)] string configJson);

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern void virtue_windows_free_string(IntPtr value);
    }
}
