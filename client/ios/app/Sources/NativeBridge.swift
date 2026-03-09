import Foundation

@_silgen_name("virtue_ios_native_init")
private func virtue_ios_native_init(
    _ configDir: UnsafePointer<CChar>?,
    _ dataDir: UnsafePointer<CChar>?,
    _ baseApiUrl: UnsafePointer<CChar>?,
    _ captureIntervalSeconds: UnsafePointer<CChar>?,
    _ batchWindowSeconds: UnsafePointer<CChar>?
) -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_ios_native_set_overrides")
private func virtue_ios_native_set_overrides(
    _ baseApiUrl: UnsafePointer<CChar>?,
    _ captureIntervalSeconds: UnsafePointer<CChar>?,
    _ batchWindowSeconds: UnsafePointer<CChar>?
) -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_ios_native_login")
private func virtue_ios_native_login(
    _ email: UnsafePointer<CChar>?,
    _ password: UnsafePointer<CChar>?,
    _ deviceName: UnsafePointer<CChar>?
) -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_ios_native_logout")
private func virtue_ios_native_logout() -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_ios_native_is_logged_in")
private func virtue_ios_native_is_logged_in() -> Bool

@_silgen_name("virtue_ios_native_get_device_id")
private func virtue_ios_native_get_device_id() -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_ios_native_run_daemon_loop")
private func virtue_ios_native_run_daemon_loop() -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_ios_native_stop_daemon")
private func virtue_ios_native_stop_daemon() -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_ios_free_string")
private func virtue_ios_free_string(_ value: UnsafeMutablePointer<CChar>?)

struct RuntimeOverrides {
    var baseApiUrl: String = ""
    var captureIntervalSeconds: String = ""
    var batchWindowSeconds: String = ""
}

enum NativeBridge {
    static func initialize(configDir: String, dataDir: String, overrides: RuntimeOverrides) -> String? {
        callReturningError {
            configDir.withCString { configDirCString in
                dataDir.withCString { dataDirCString in
                    overrides.baseApiUrl.withCString { baseApiCString in
                        overrides.captureIntervalSeconds.withCString { captureIntervalCString in
                            overrides.batchWindowSeconds.withCString { batchWindowCString in
                                virtue_ios_native_init(
                                    configDirCString,
                                    dataDirCString,
                                    baseApiCString,
                                    captureIntervalCString,
                                    batchWindowCString
                                )
                            }
                        }
                    }
                }
            }
        }
    }

    static func setOverrides(_ overrides: RuntimeOverrides) -> String? {
        callReturningError {
            overrides.baseApiUrl.withCString { baseApiCString in
                overrides.captureIntervalSeconds.withCString { captureIntervalCString in
                    overrides.batchWindowSeconds.withCString { batchWindowCString in
                        virtue_ios_native_set_overrides(
                            baseApiCString,
                            captureIntervalCString,
                            batchWindowCString
                        )
                    }
                }
            }
        }
    }

    static func login(email: String, password: String, deviceName: String) -> String? {
        callReturningError {
            email.withCString { emailCString in
                password.withCString { passwordCString in
                    deviceName.withCString { deviceNameCString in
                        virtue_ios_native_login(emailCString, passwordCString, deviceNameCString)
                    }
                }
            }
        }
    }

    static func logout() -> String? {
        callReturningError {
            virtue_ios_native_logout()
        }
    }

    static func isLoggedIn() -> Bool {
        virtue_ios_native_is_logged_in()
    }

    static func getDeviceId() -> String? {
        guard let ptr = virtue_ios_native_get_device_id() else {
            return nil
        }

        let value = String(cString: ptr)
        virtue_ios_free_string(ptr)
        return value
    }

    static func runDaemonLoop() -> String? {
        callReturningError {
            virtue_ios_native_run_daemon_loop()
        }
    }

    static func stopDaemon() -> String? {
        callReturningError {
            virtue_ios_native_stop_daemon()
        }
    }

    private static func callReturningError(
        _ call: () -> UnsafeMutablePointer<CChar>?
    ) -> String? {
        guard let errorPtr = call() else {
            return nil
        }
        let message = String(cString: errorPtr)
        virtue_ios_free_string(errorPtr)
        return message
    }
}
