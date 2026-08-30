import Foundation

@_silgen_name("virtue_ios_native_init")
private func virtue_ios_native_init(
    _ configDir: UnsafePointer<CChar>?,
    _ dataDir: UnsafePointer<CChar>?
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

@_silgen_name("virtue_ios_native_get_status_json")
private func virtue_ios_native_get_status_json() -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_ios_native_report_issue")
private func virtue_ios_native_report_issue(
    _ message: UnsafePointer<CChar>?,
    _ contactEmail: UnsafePointer<CChar>?,
    _ includeLogs: Bool,
    _ platformDetails: UnsafePointer<CChar>?
) -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_ios_native_run_daemon_loop")
private func virtue_ios_native_run_daemon_loop() -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_ios_native_stop_daemon")
private func virtue_ios_native_stop_daemon() -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_ios_native_request_pause_monitoring")
private func virtue_ios_native_request_pause_monitoring(
    _ source: UnsafePointer<CChar>?
) -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_ios_native_request_resume_monitoring")
private func virtue_ios_native_request_resume_monitoring() -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_ios_free_string")
private func virtue_ios_free_string(_ value: UnsafeMutablePointer<CChar>?)

enum NativeBridge {
    private static let initLock = NSLock()
    private static var initialized = false

    private static let daemonQueue = DispatchQueue(label: "org.virtueinitiative.ios.app.daemon")
    private static let daemonLock = NSLock()
    private static var daemonLoopRefCount = 0

    /// Idempotent, retry-capable init: safe to call from every entry point
    /// (app launch, login, logout, status refresh) since a failed attempt
    /// doesn't permanently brick later calls the way a single one-shot
    /// `initialize` call at app launch would.
    @discardableResult
    static func ensureInitialized(configDir: String, dataDir: String) -> String? {
        initLock.lock()
        defer { initLock.unlock() }

        if initialized {
            return nil
        }

        var error = rawInitialize(configDir: configDir, dataDir: dataDir)
        if let currentError = error, currentError.contains("serialization error") {
            // Corrupted state files — wipe and retry once
            try? FileManager.default.removeItem(atPath: dataDir)
            try? FileManager.default.createDirectory(
                atPath: dataDir,
                withIntermediateDirectories: true
            )
            error = rawInitialize(configDir: configDir, dataDir: dataDir)
        }

        if error == nil {
            initialized = true
        }
        return error
    }

    /// `login`/`logout`/`requestPauseMonitoring` block on a reply from the
    /// daemon's `run_forever()` loop thread, so it must be running in this
    /// process for the call to complete. Unlike Android (one process), the
    /// Safari extension is a *separate OS process* that runs its own
    /// independent `Daemon` against the same on-disk state file with no
    /// cross-process locking — so this process's loop is started only for
    /// the duration of the calls that need it (ref-counted, to tolerate
    /// overlapping calls) and stopped immediately after, rather than left
    /// running continuously, to keep the window where both processes could
    /// tick — and race each other's writes — as small as possible.
    private static func withDaemonLoop<T>(_ body: () -> T) -> T {
        daemonLock.lock()
        daemonLoopRefCount += 1
        let shouldStart = daemonLoopRefCount == 1
        daemonLock.unlock()

        if shouldStart {
            daemonQueue.async {
                if let error = runDaemonLoop() {
                    NSLog("Virtue: daemon loop exited with error: \(error)")
                }
            }
        }

        let result = body()

        daemonLock.lock()
        daemonLoopRefCount -= 1
        let shouldStop = daemonLoopRefCount == 0
        daemonLock.unlock()

        if shouldStop {
            _ = rawStopDaemon()
        }

        return result
    }

    private static func rawInitialize(configDir: String, dataDir: String) -> String? {
        callReturningError {
            configDir.withCString { configDirCString in
                dataDir.withCString { dataDirCString in
                    virtue_ios_native_init(configDirCString, dataDirCString)
                }
            }
        }
    }

    static func login(email: String, password: String, deviceName: String) -> String? {
        withDaemonLoop {
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
    }

    static func logout() -> String? {
        withDaemonLoop {
            callReturningError {
                virtue_ios_native_logout()
            }
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

    static func getStatusJson() -> String? {
        guard let ptr = virtue_ios_native_get_status_json() else {
            return nil
        }

        let value = String(cString: ptr)
        virtue_ios_free_string(ptr)
        return value
    }

    static func reportIssue(
        message: String,
        contactEmail: String?,
        includeLogs: Bool,
        platformDetails: String
    ) -> String? {
        callReturningError {
            message.withCString { messageCString in
                withOptionalCString(contactEmail) { contactEmailCString in
                    platformDetails.withCString { platformDetailsCString in
                        virtue_ios_native_report_issue(
                            messageCString,
                            contactEmailCString,
                            includeLogs,
                            platformDetailsCString
                        )
                    }
                }
            }
        }
    }

    private static func runDaemonLoop() -> String? {
        callReturningError {
            virtue_ios_native_run_daemon_loop()
        }
    }

    private static func rawStopDaemon() -> String? {
        callReturningError {
            virtue_ios_native_stop_daemon()
        }
    }

    static func requestPauseMonitoring(source: String) -> String? {
        withDaemonLoop {
            callReturningError {
                source.withCString { sourceCString in
                    virtue_ios_native_request_pause_monitoring(sourceCString)
                }
            }
        }
    }

    static func requestResumeMonitoring() -> String? {
        withDaemonLoop {
            callReturningError {
                virtue_ios_native_request_resume_monitoring()
            }
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

    private static func withOptionalCString<Result>(
        _ value: String?,
        _ body: (UnsafePointer<CChar>?) -> Result
    ) -> Result {
        guard let value else {
            return body(nil)
        }
        return value.withCString(body)
    }
}
