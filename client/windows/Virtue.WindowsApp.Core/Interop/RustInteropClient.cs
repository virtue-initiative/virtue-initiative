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

    public void Initialize()
    {
        var error = NativeMethods.virtue_windows_init();
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

    public void StopMonitoringForOsSessionEnd()
    {
        var error = NativeMethods.virtue_windows_stop_monitoring_for_os_session_end();
        ThrowIfError(error);
    }

    public SessionStatusPayload GetSessionStatus() =>
        ReadJsonPayload<SessionStatusPayload>(NativeMethods.virtue_windows_get_session_status_json());

    public MonitorStatusPayload GetMonitorStatus() =>
        ReadJsonPayload<MonitorStatusPayload>(NativeMethods.virtue_windows_get_monitor_status_json());

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
        internal static extern IntPtr virtue_windows_init();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_get_session_status_json();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_start_monitoring();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_stop_monitoring();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_stop_monitoring_from_tray_exit();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_stop_monitoring_for_os_session_end();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_get_monitor_status_json();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_login(
            [MarshalAs(UnmanagedType.LPUTF8Str)] string requestJson);

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern IntPtr virtue_windows_logout();

        [DllImport(NativeLibraryName, CallingConvention = CallingConvention.Cdecl)]
        internal static extern void virtue_windows_free_string(IntPtr value);
    }
}
