import Foundation

@_silgen_name("virtue_mac_native_init")
private func virtue_mac_native_init() -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_mac_native_login")
private func virtue_mac_native_login(
    _ email: UnsafePointer<CChar>?,
    _ password: UnsafePointer<CChar>?,
    _ deviceName: UnsafePointer<CChar>?
) -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_mac_native_logout")
private func virtue_mac_native_logout() -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_mac_native_is_logged_in")
private func virtue_mac_native_is_logged_in() -> Bool

@_silgen_name("virtue_mac_native_get_device_id")
private func virtue_mac_native_get_device_id() -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_mac_native_get_status_json")
private func virtue_mac_native_get_status_json() -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_mac_native_poll_daemon_status")
private func virtue_mac_native_poll_daemon_status() -> Int32

@_silgen_name("virtue_mac_native_request_user_stop")
private func virtue_mac_native_request_user_stop(
    _ source: UnsafePointer<CChar>?
) -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_mac_native_has_capture_permission")
private func virtue_mac_native_has_capture_permission() -> Bool

@_silgen_name("virtue_mac_native_request_capture_permission")
private func virtue_mac_native_request_capture_permission() -> Bool

@_silgen_name("virtue_mac_native_ensure_daemon_running")
private func virtue_mac_native_ensure_daemon_running(
    _ daemonExePath: UnsafePointer<CChar>?
) -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_mac_native_stop_daemon")
private func virtue_mac_native_stop_daemon(_ userInitiated: Bool) -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_mac_native_relaunch_daemon")
private func virtue_mac_native_relaunch_daemon(
    _ daemonExePath: UnsafePointer<CChar>?
) -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_mac_native_agent_is_registered")
private func virtue_mac_native_agent_is_registered() -> Bool

@_silgen_name("virtue_mac_native_get_build_label")
private func virtue_mac_native_get_build_label() -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_mac_native_default_device_name")
private func virtue_mac_native_default_device_name() -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_mac_native_default_capture_interval_seconds")
private func virtue_mac_native_default_capture_interval_seconds() -> UInt64

@_silgen_name("virtue_mac_native_default_batch_window_seconds")
private func virtue_mac_native_default_batch_window_seconds() -> UInt64

@_silgen_name("virtue_mac_native_daemon_exe_path")
private func virtue_mac_native_daemon_exe_path(
    _ appBundlePath: UnsafePointer<CChar>?
) -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_mac_native_free_string")
private func virtue_mac_native_free_string(_ value: UnsafeMutablePointer<CChar>?)

/// Result of a single daemon status poll. The daemon's lifecycle module
/// always reports `is_running: true` when it can answer a `StatusRequest`,
/// so the meaningful distinction is *how* the poll failed: a refused
/// connection means the daemon is genuinely gone, while a timeout means it
/// is alive but busy (e.g. blocked on a login network call).
enum DaemonStatus: Int32 {
    case running = 0
    case stopped = 1
    case unreachable = 2
}

enum NativeBridge {
    static func initialize() -> String? {
        callReturningError {
            virtue_mac_native_init()
        }
    }

    static func login(email: String, password: String, deviceName: String) -> String? {
        callReturningError {
            email.withCString { emailCString in
                password.withCString { passwordCString in
                    deviceName.withCString { deviceNameCString in
                        virtue_mac_native_login(emailCString, passwordCString, deviceNameCString)
                    }
                }
            }
        }
    }

    static func logout() -> String? {
        callReturningError {
            virtue_mac_native_logout()
        }
    }

    static func isLoggedIn() -> Bool {
        virtue_mac_native_is_logged_in()
    }

    static func getDeviceId() -> String? {
        consumeOptionalString(virtue_mac_native_get_device_id())
    }

    static func getStatusJson() -> String? {
        consumeOptionalString(virtue_mac_native_get_status_json())
    }

    static func pollDaemonStatus() -> DaemonStatus {
        DaemonStatus(rawValue: virtue_mac_native_poll_daemon_status()) ?? .stopped
    }

    static func requestUserStop(source: String) -> String? {
        callReturningError {
            source.withCString { sourceCString in
                virtue_mac_native_request_user_stop(sourceCString)
            }
        }
    }

    static func hasCapturePermission() -> Bool {
        virtue_mac_native_has_capture_permission()
    }

    static func requestCapturePermission() -> Bool {
        virtue_mac_native_request_capture_permission()
    }

    static func ensureDaemonRunning(daemonExePath: String) -> String? {
        callReturningError {
            daemonExePath.withCString { pathCString in
                virtue_mac_native_ensure_daemon_running(pathCString)
            }
        }
    }

    static func stopDaemon(userInitiated: Bool) -> String? {
        callReturningError {
            virtue_mac_native_stop_daemon(userInitiated)
        }
    }

    static func relaunchDaemon(daemonExePath: String) -> String? {
        callReturningError {
            daemonExePath.withCString { pathCString in
                virtue_mac_native_relaunch_daemon(pathCString)
            }
        }
    }

    static func agentIsRegistered() -> Bool {
        virtue_mac_native_agent_is_registered()
    }

    static func getBuildLabel() -> String {
        consumeOptionalString(virtue_mac_native_get_build_label()) ?? "unknown"
    }

    static func defaultDeviceName() -> String {
        consumeOptionalString(virtue_mac_native_default_device_name()) ?? "mac-device"
    }

    static func defaultCaptureIntervalSeconds() -> UInt64 {
        virtue_mac_native_default_capture_interval_seconds()
    }

    static func defaultBatchWindowSeconds() -> UInt64 {
        virtue_mac_native_default_batch_window_seconds()
    }

    static func daemonExePath(appBundlePath: String) -> String {
        appBundlePath.withCString { pathCString in
            consumeOptionalString(virtue_mac_native_daemon_exe_path(pathCString)) ?? ""
        }
    }

    private static func consumeOptionalString(_ ptr: UnsafeMutablePointer<CChar>?) -> String? {
        guard let ptr else {
            return nil
        }
        let value = String(cString: ptr)
        virtue_mac_native_free_string(ptr)
        return value
    }

    private static func callReturningError(
        _ call: () -> UnsafeMutablePointer<CChar>?
    ) -> String? {
        consumeOptionalString(call())
    }
}
