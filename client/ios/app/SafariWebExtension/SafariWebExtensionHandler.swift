import Foundation
import SafariServices

private let extensionMessageKey = "message"

@_silgen_name("virtue_ios_native_init")
private func virtue_ios_native_init(
    _ configDir: UnsafePointer<CChar>?,
    _ dataDir: UnsafePointer<CChar>?,
    _ baseApiUrl: UnsafePointer<CChar>?,
    _ captureIntervalSeconds: UnsafePointer<CChar>?,
    _ batchWindowSeconds: UnsafePointer<CChar>?
) -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_ios_native_run_daemon_loop")
private func virtue_ios_native_run_daemon_loop() -> UnsafeMutablePointer<CChar>?

@_silgen_name("virtue_ios_free_string")
private func virtue_ios_free_string(_ value: UnsafeMutablePointer<CChar>?)

private final class SafariFrameStore {
    static let shared = SafariFrameStore()

    private let lock = NSLock()
    private var latestFrame: Data?
    private var lastFrameAt: TimeInterval = 0

    private init() {}

    func updateFrame(_ png: Data) {
        let now = Date().timeIntervalSince1970
        lock.lock()
        latestFrame = png
        lastFrameAt = now
        lock.unlock()
    }

    func statusCode() -> Int32 {
        lock.lock()
        defer { lock.unlock() }
        guard latestFrame != nil else {
            return 2
        }
        let age = max(0, Date().timeIntervalSince1970 - lastFrameAt)
        return age <= VirtueShared.safariFrameFreshnessThresholdSeconds ? 0 : 2
    }

    func copyFrame() -> Data? {
        lock.lock()
        defer { lock.unlock() }
        guard let latestFrame else {
            return nil
        }
        let age = max(0, Date().timeIntervalSince1970 - lastFrameAt)
        guard age <= VirtueShared.safariFrameFreshnessThresholdSeconds else {
            return nil
        }
        return latestFrame
    }
}

private final class SafariSharedStateStore {
    static let shared = SafariSharedStateStore()

    private let lock = NSLock()
    private let defaults = UserDefaults(suiteName: VirtueShared.appGroupID)

    private init() {}

    func markMessage() {
        lock.lock()
        defaults?.set(Date().timeIntervalSince1970, forKey: VirtueShared.safariLastMessageAtKey)
        lock.unlock()
    }

    func markFrame(url: String?, title: String?) {
        lock.lock()
        let now = Date().timeIntervalSince1970
        defaults?.set(now, forKey: VirtueShared.safariLastMessageAtKey)
        defaults?.set(now, forKey: VirtueShared.safariLastFrameAtKey)
        if let url, !url.isEmpty {
            defaults?.set(url, forKey: VirtueShared.safariLastURLKey)
        }
        if let title, !title.isEmpty {
            defaults?.set(title, forKey: VirtueShared.safariLastTitleKey)
        }
        defaults?.removeObject(forKey: VirtueShared.safariLastErrorKey)
        lock.unlock()
    }

    func markCaptureError(_ error: String) {
        lock.lock()
        defaults?.set(Date().timeIntervalSince1970, forKey: VirtueShared.safariLastMessageAtKey)
        defaults?.set(error, forKey: VirtueShared.safariLastErrorKey)
        lock.unlock()
    }

    func markDaemonState(running: Bool, error: String?) {
        lock.lock()
        defaults?.set(running, forKey: VirtueShared.safariDaemonRunningKey)
        if let error, !error.isEmpty {
            defaults?.set(error, forKey: VirtueShared.safariDaemonLastErrorKey)
        } else {
            defaults?.removeObject(forKey: VirtueShared.safariDaemonLastErrorKey)
        }
        lock.unlock()
    }
}

private final class SafariNativeRuntime {
    static let shared = SafariNativeRuntime()

    private let lock = NSLock()
    private let daemonQueue = DispatchQueue(label: "org.virtueinitiative.ios.safari.daemon")
    private var initialized = false
    private var daemonRunning = false

    private init() {}

    func ensureInitializedAndRunning() {
        if let initError = initializeIfNeeded() {
            SafariSharedStateStore.shared.markCaptureError("native_init_failed: \(initError)")
            return
        }
        startDaemonIfNeeded()
    }

    private func initializeIfNeeded() -> String? {
        lock.lock()
        if initialized {
            lock.unlock()
            return nil
        }
        lock.unlock()

        let defaults = UserDefaults(suiteName: VirtueShared.appGroupID)
        let overrides = (
            defaults?.string(forKey: VirtueShared.baseApiUrlKey) ?? VirtueShared.defaultBaseApiUrl,
            defaults?.string(forKey: VirtueShared.captureIntervalKey) ?? VirtueShared.defaultCaptureIntervalSeconds,
            defaults?.string(forKey: VirtueShared.batchWindowKey) ?? VirtueShared.defaultBatchWindowSeconds
        )

        guard let root = FileManager.default.containerURL(
            forSecurityApplicationGroupIdentifier: VirtueShared.appGroupID
        ) else {
            return "missing app group container"
        }

        let configDir = root.appendingPathComponent("virtue/config", isDirectory: true)
        let dataDir = root.appendingPathComponent("virtue/data", isDirectory: true)

        do {
            try FileManager.default.createDirectory(at: configDir, withIntermediateDirectories: true)
            try FileManager.default.createDirectory(at: dataDir, withIntermediateDirectories: true)
        } catch {
            return "failed to prepare runtime storage: \(error.localizedDescription)"
        }

        let initError = configDir.path.withCString { configCString in
            dataDir.path.withCString { dataCString in
                overrides.0.withCString { baseURLCString in
                    overrides.1.withCString { captureIntervalCString in
                        overrides.2.withCString { batchWindowCString in
                            virtue_ios_native_init(
                                configCString,
                                dataCString,
                                baseURLCString,
                                captureIntervalCString,
                                batchWindowCString
                            )
                        }
                    }
                }
            }
        }

        if let initError {
            let message = String(cString: initError)
            virtue_ios_free_string(initError)
            return message
        }

        lock.lock()
        initialized = true
        lock.unlock()
        return nil
    }

    private func startDaemonIfNeeded() {
        lock.lock()
        if daemonRunning {
            lock.unlock()
            return
        }
        daemonRunning = true
        lock.unlock()

        SafariSharedStateStore.shared.markDaemonState(running: true, error: nil)

        daemonQueue.async { [weak self] in
            guard let self else { return }
            let daemonError = virtue_ios_native_run_daemon_loop()

            var daemonMessage: String?
            if let daemonError {
                daemonMessage = String(cString: daemonError)
                virtue_ios_free_string(daemonError)
            }

            self.lock.lock()
            self.daemonRunning = false
            self.lock.unlock()

            SafariSharedStateStore.shared.markDaemonState(
                running: false,
                error: daemonMessage
            )
        }
    }
}

@_cdecl("virtue_ios_capture_status")
public func virtue_ios_capture_status() -> Int32 {
    SafariFrameStore.shared.statusCode()
}

@_cdecl("virtue_ios_capture_png_write")
public func virtue_ios_capture_png_write(
    _ outBuffer: UnsafeMutablePointer<UnsafePointer<UInt8>?>?,
    _ outLength: UnsafeMutablePointer<Int>?
) -> Int32 {
    guard let outBuffer, let outLength else {
        return -1
    }
    guard let frame = SafariFrameStore.shared.copyFrame() else {
        return 1
    }

    let raw = malloc(frame.count)
    guard let raw else {
        return -2
    }
    frame.copyBytes(to: raw.assumingMemoryBound(to: UInt8.self), count: frame.count)
    outBuffer.pointee = UnsafePointer(raw.assumingMemoryBound(to: UInt8.self))
    outLength.pointee = frame.count
    return 0
}

@_cdecl("virtue_ios_capture_png_release")
public func virtue_ios_capture_png_release(_ buffer: UnsafePointer<UInt8>?, _ length: Int) {
    _ = length
    guard let buffer else { return }
    free(UnsafeMutableRawPointer(mutating: buffer))
}

final class SafariWebExtensionHandler: NSObject, NSExtensionRequestHandling {
    func beginRequest(with context: NSExtensionContext) {
        let responsePayload = handleRequest(context)
        let response = NSExtensionItem()
        response.userInfo = [extensionMessageKey: responsePayload]
        context.completeRequest(returningItems: [response], completionHandler: nil)
    }

    private func handleRequest(_ context: NSExtensionContext) -> [String: Any] {
        SafariSharedStateStore.shared.markMessage()

        guard
            let item = context.inputItems.first as? NSExtensionItem,
            let userInfo = item.userInfo,
            let payload = userInfo[extensionMessageKey] as? [String: Any]
        else {
            return ["ok": false, "error": "missing_payload"]
        }

        let type = payload["type"] as? String ?? "unknown"
        switch type {
        case "capture_frame":
            return handleCaptureFrame(payload)
        case "capture_error":
            let errorMessage = payload["error"] as? String ?? "capture_error"
            SafariSharedStateStore.shared.markCaptureError(errorMessage)
            return ["ok": true]
        case "ping":
            SafariNativeRuntime.shared.ensureInitializedAndRunning()
            return ["ok": true]
        default:
            return ["ok": false, "error": "unsupported_type", "type": type]
        }
    }

    private func handleCaptureFrame(_ payload: [String: Any]) -> [String: Any] {
        guard let png = decodePNG(payload) else {
            SafariSharedStateStore.shared.markCaptureError("invalid_frame_payload")
            return ["ok": false, "error": "invalid_frame_payload"]
        }

        SafariFrameStore.shared.updateFrame(png)
        SafariSharedStateStore.shared.markFrame(
            url: payload["url"] as? String,
            title: payload["title"] as? String
        )
        SafariNativeRuntime.shared.ensureInitializedAndRunning()

        return ["ok": true, "bytes": png.count]
    }

    private func decodePNG(_ payload: [String: Any]) -> Data? {
        if let pngBase64 = payload["png_base64"] as? String {
            return Data(base64Encoded: pngBase64, options: [.ignoreUnknownCharacters])
        }

        if let dataURL = payload["png_data_url"] as? String {
            let parts = dataURL.split(separator: ",", maxSplits: 1, omittingEmptySubsequences: false)
            guard parts.count == 2 else {
                return nil
            }
            return Data(
                base64Encoded: String(parts[1]),
                options: [.ignoreUnknownCharacters]
            )
        }

        return nil
    }
}
