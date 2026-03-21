import Combine
import Foundation
import UIKit

private struct CoreServiceStatus: Decodable {
    let isRunning: Bool
    let lastLoopAtMs: Int64?
    let lastScreenshotAtMs: Int64?
    let lastBatchAtMs: Int64?
    let pendingRequestCount: Int

    private enum CodingKeys: String, CodingKey {
        case isRunning = "is_running"
        case lastLoopAtMs = "last_loop_at_ms"
        case lastScreenshotAtMs = "last_screenshot_at_ms"
        case lastBatchAtMs = "last_batch_at_ms"
        case pendingRequestCount = "pending_request_count"
    }
}

private struct CoreDeviceSettings: Decodable {
    let enabled: Bool
}

private struct CorePendingRequest: Decodable {}

final class MonitoringCoordinator: ObservableObject {
    @Published var email: String = ""
    @Published var password: String = ""
    @Published var baseApiUrlOverride: String = ""
    @Published var captureIntervalOverride: String = ""
    @Published var batchWindowOverride: String = ""

    @Published private(set) var statusMessage: String = "Not initialized"
    @Published private(set) var loggedIn: Bool = false
    @Published private(set) var deviceId: String = "<none>"
    @Published private(set) var monitorSummary: String = "idle"
    @Published private(set) var pendingRequestCount: Int = 0
    @Published private(set) var currentApiBaseUrl: String = VirtueShared.defaultBaseApiUrl
    @Published private(set) var lastCoreLoop: String = "<none>"
    @Published private(set) var lastCoreScreenshot: String = "<none>"
    @Published private(set) var lastCoreBatch: String = "<none>"

    @Published private(set) var safariCaptureHealth: String = "No Safari extension heartbeat yet"
    @Published private(set) var safariLastHeartbeat: String = "<none>"
    @Published private(set) var safariLastFrame: String = "<none>"
    @Published private(set) var safariLastPage: String = "<none>"
    @Published private(set) var safariLastError: String = "<none>"
    @Published private(set) var safariDaemonStatus: String = "No daemon state yet"

    private var didBecomeActiveObserver: NSObjectProtocol?
    private var willEnterForegroundObserver: NSObjectProtocol?
    private var statusRefreshTimer: Timer?

    private var sharedDefaults: UserDefaults? {
        UserDefaults(suiteName: VirtueShared.appGroupID)
    }

    private let configDir: URL
    private let dataDir: URL

    init() {
        let root: URL = {
            if let groupRoot = FileManager.default.containerURL(
                forSecurityApplicationGroupIdentifier: VirtueShared.appGroupID
            ) {
                return groupRoot.appendingPathComponent("virtue", isDirectory: true)
            }
            let appSupport = FileManager.default.urls(
                for: .applicationSupportDirectory,
                in: .userDomainMask
            ).first ?? URL(fileURLWithPath: NSTemporaryDirectory())
            return appSupport.appendingPathComponent("virtue", isDirectory: true)
        }()

        configDir = root.appendingPathComponent("config", isDirectory: true)
        dataDir = root.appendingPathComponent("data", isDirectory: true)

        loadOverrideInputs()
        initializeCore()
        bindAppLifecycleState()
        refreshSessionState()
        refreshCoreStatus()
        refreshSafariStatus()
        startStatusRefreshTimerIfNeeded()
    }

    deinit {
        if let didBecomeActiveObserver {
            NotificationCenter.default.removeObserver(didBecomeActiveObserver)
        }
        if let willEnterForegroundObserver {
            NotificationCenter.default.removeObserver(willEnterForegroundObserver)
        }
        stopStatusRefreshTimer()
    }

    func applyOverrides() {
        let overrides = runtimeOverrides()
        persistOverrides(overrides)

        if let error = NativeBridge.setOverrides(overrides) {
            statusMessage = "Override update failed: \(error)"
            return
        }

        refreshCoreStatus()
        statusMessage = "Runtime overrides updated"
    }

    func login() {
        guard !email.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            statusMessage = "Email is required"
            return
        }
        guard !password.isEmpty else {
            statusMessage = "Password is required"
            return
        }

        let deviceName = UIDevice.current.name
        if let error = NativeBridge.login(email: email, password: password, deviceName: deviceName) {
            statusMessage = "Login failed: \(error)"
            return
        }

        refreshSessionState()
        refreshCoreStatus()
        statusMessage = "Signed in. Enable Virtue Safari extension in Safari settings."
    }

    func logout() {
        let error = NativeBridge.logout()
        if let error {
            statusMessage = "Logout warning: \(error)"
        } else {
            statusMessage = "Signed out"
        }
        refreshSessionState()
        refreshCoreStatus()
    }

    private func initializeCore() {
        do {
            try FileManager.default.createDirectory(at: configDir, withIntermediateDirectories: true)
            try FileManager.default.createDirectory(at: dataDir, withIntermediateDirectories: true)
        } catch {
            statusMessage = "Directory setup failed: \(error.localizedDescription)"
            return
        }

        let error = NativeBridge.initialize(
            configDir: configDir.path,
            dataDir: dataDir.path,
            overrides: runtimeOverrides()
        )

        if let error {
            statusMessage = "Core initialization failed: \(error)"
        } else {
            statusMessage = "Core ready"
        }
    }

    private func refreshSessionState() {
        loggedIn = NativeBridge.isLoggedIn()
        deviceId = NativeBridge.getDeviceId() ?? "<none>"
    }

    private func bindAppLifecycleState() {
        willEnterForegroundObserver = NotificationCenter.default.addObserver(
            forName: UIApplication.willEnterForegroundNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            self?.handleAppForegroundEvent()
        }

        didBecomeActiveObserver = NotificationCenter.default.addObserver(
            forName: UIApplication.didBecomeActiveNotification,
            object: nil,
            queue: .main
        ) { [weak self] _ in
            self?.handleAppForegroundEvent()
        }
    }

    private func handleAppForegroundEvent() {
        refreshSessionState()
        refreshCoreStatus()
        refreshSafariStatus()
        startStatusRefreshTimerIfNeeded()
    }

    private func startStatusRefreshTimerIfNeeded() {
        guard statusRefreshTimer == nil else {
            return
        }
        statusRefreshTimer = Timer.scheduledTimer(withTimeInterval: 2, repeats: true) { [weak self] _ in
            self?.refreshCoreStatus()
            self?.refreshSafariStatus()
        }
    }

    private func stopStatusRefreshTimer() {
        statusRefreshTimer?.invalidate()
        statusRefreshTimer = nil
    }

    private func refreshSafariStatus() {
        guard let defaults = sharedDefaults else {
            safariCaptureHealth = "App Group unavailable"
            safariLastHeartbeat = "<none>"
            safariLastFrame = "<none>"
            safariLastPage = "<none>"
            safariLastError = "<none>"
            safariDaemonStatus = "Unavailable"
            return
        }

        let now = Date().timeIntervalSince1970
        let lastHeartbeatAt = timestamp(forKey: VirtueShared.safariLastMessageAtKey, defaults: defaults)
        let lastFrameAt = timestamp(forKey: VirtueShared.safariLastFrameAtKey, defaults: defaults)

        if let lastHeartbeatAt {
            let heartbeatAge = max(0, now - lastHeartbeatAt)
            safariLastHeartbeat = "\(Int(heartbeatAge.rounded()))s ago (\(formatAbsoluteTime(lastHeartbeatAt)))"
            if heartbeatAge <= VirtueShared.safariHeartbeatStaleThresholdSeconds {
                safariCaptureHealth = "Active in Safari"
            } else {
                safariCaptureHealth = "Stale (open Safari to resume capture)"
            }
        } else {
            safariCaptureHealth = "No Safari extension heartbeat yet"
            safariLastHeartbeat = "<none>"
        }

        if let lastFrameAt {
            let frameAge = max(0, now - lastFrameAt)
            safariLastFrame = "\(Int(frameAge.rounded()))s ago (\(formatAbsoluteTime(lastFrameAt)))"
        } else {
            safariLastFrame = "<none>"
        }

        let pageTitle = defaults.string(forKey: VirtueShared.safariLastTitleKey)?.trimmingCharacters(
            in: .whitespacesAndNewlines
        )
        let pageURL = defaults.string(forKey: VirtueShared.safariLastURLKey)?.trimmingCharacters(
            in: .whitespacesAndNewlines
        )

        if let pageTitle, !pageTitle.isEmpty, let pageURL, !pageURL.isEmpty {
            safariLastPage = "\(pageTitle) — \(pageURL)"
        } else if let pageURL, !pageURL.isEmpty {
            safariLastPage = pageURL
        } else {
            safariLastPage = "<none>"
        }

        let lastError = defaults.string(forKey: VirtueShared.safariLastErrorKey)?.trimmingCharacters(
            in: .whitespacesAndNewlines
        )
        safariLastError = (lastError?.isEmpty == false) ? lastError! : "<none>"

        if defaults.object(forKey: VirtueShared.safariDaemonRunningKey) == nil {
            safariDaemonStatus = "No daemon state yet"
        } else {
            let running = defaults.bool(forKey: VirtueShared.safariDaemonRunningKey)
            if running {
                safariDaemonStatus = "Running in Safari extension"
            } else if let daemonError = defaults.string(forKey: VirtueShared.safariDaemonLastErrorKey), !daemonError.isEmpty {
                safariDaemonStatus = "Stopped with error: \(daemonError)"
            } else {
                safariDaemonStatus = "Stopped"
            }
        }
    }

    private func refreshCoreStatus() {
        currentApiBaseUrl = runtimeOverrides().baseApiUrl.isEmpty
            ? VirtueShared.defaultBaseApiUrl
            : runtimeOverrides().baseApiUrl

        let serviceStatus = loadJSONFile(named: "status.json", as: CoreServiceStatus.self)
        let pendingRequests = loadJSONFile(named: "pending_requests.json", as: [CorePendingRequest].self)
            ?? []
        let deviceSettings = loadJSONFile(named: "device_settings.json", as: CoreDeviceSettings?.self) ?? nil

        pendingRequestCount = serviceStatus?.pendingRequestCount ?? pendingRequests.count
        lastCoreLoop = formatMillisTimestamp(serviceStatus?.lastLoopAtMs)
        lastCoreScreenshot = formatMillisTimestamp(serviceStatus?.lastScreenshotAtMs)
        lastCoreBatch = formatMillisTimestamp(serviceStatus?.lastBatchAtMs)

        if !loggedIn {
            monitorSummary = "signed out"
        } else if deviceSettings?.enabled == false {
            monitorSummary = "disabled by device settings"
        } else if serviceStatus?.isRunning == true {
            monitorSummary = "active"
        } else {
            monitorSummary = "idle"
        }
    }

    private func runtimeOverrides() -> RuntimeOverrides {
        RuntimeOverrides(
            baseApiUrl: baseApiUrlOverride.trimmingCharacters(in: .whitespacesAndNewlines),
            captureIntervalSeconds: captureIntervalOverride.trimmingCharacters(in: .whitespacesAndNewlines),
            batchWindowSeconds: batchWindowOverride.trimmingCharacters(in: .whitespacesAndNewlines)
        )
    }

    private func persistOverrides(_ overrides: RuntimeOverrides) {
        let write = { (defaults: UserDefaults) in
            defaults.set(overrides.baseApiUrl, forKey: VirtueShared.baseApiUrlKey)
            defaults.set(overrides.captureIntervalSeconds, forKey: VirtueShared.captureIntervalKey)
            defaults.set(overrides.batchWindowSeconds, forKey: VirtueShared.batchWindowKey)
        }
        write(UserDefaults.standard)
        if let sharedDefaults {
            write(sharedDefaults)
        }
    }

    private func loadOverrideInputs() {
        let preferredDefaults = sharedDefaults ?? UserDefaults.standard
        baseApiUrlOverride = storedOverride(
            forKey: VirtueShared.baseApiUrlKey,
            defaults: preferredDefaults,
            fallback: VirtueShared.defaultBaseApiUrl
        )
        captureIntervalOverride = storedOverride(
            forKey: VirtueShared.captureIntervalKey,
            defaults: preferredDefaults,
            fallback: VirtueShared.defaultCaptureIntervalSeconds
        )
        batchWindowOverride = storedOverride(
            forKey: VirtueShared.batchWindowKey,
            defaults: preferredDefaults,
            fallback: VirtueShared.defaultBatchWindowSeconds
        )
        persistOverrides(runtimeOverrides())
    }

    private func storedOverride(forKey key: String, defaults: UserDefaults, fallback: String) -> String {
        let value = defaults.string(forKey: key)?.trimmingCharacters(in: .whitespacesAndNewlines)
        return (value?.isEmpty == false) ? value! : fallback
    }

    private func loadJSONFile<T: Decodable>(named name: String, as type: T.Type) -> T? {
        let fileURL = dataDir.appendingPathComponent(name, isDirectory: false)
        guard let data = try? Data(contentsOf: fileURL), !data.isEmpty else {
            return nil
        }
        return try? JSONDecoder().decode(type, from: data)
    }

    private func timestamp(forKey key: String, defaults: UserDefaults) -> TimeInterval? {
        guard defaults.object(forKey: key) != nil else {
            return nil
        }
        return defaults.double(forKey: key)
    }

    private func formatMillisTimestamp(_ timestampMs: Int64?) -> String {
        guard let timestampMs else {
            return "<none>"
        }
        let timestamp = TimeInterval(timestampMs) / 1000
        return formatAbsoluteTime(timestamp)
    }

    private func formatAbsoluteTime(_ timestamp: TimeInterval) -> String {
        let formatter = DateFormatter()
        formatter.dateStyle = .none
        formatter.timeStyle = .medium
        return formatter.string(from: Date(timeIntervalSince1970: timestamp))
    }
}
