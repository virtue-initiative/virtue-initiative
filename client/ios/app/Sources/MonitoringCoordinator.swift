import Combine
import Foundation
import UIKit

private struct CoreServiceStatus: Decodable {
    let isRunning: Bool
    let isAuthenticated: Bool
    let lastLoopAtMs: Int64?
    let lastScreenshotAtMs: Int64?
    let lastBatchAtMs: Int64?
    let pendingRequestCount: Int
    let lifecycle: CoreLifecycleStatus?

    private enum CodingKeys: String, CodingKey {
        case isRunning = "is_running"
        case isAuthenticated = "is_authenticated"
        case lastLoopAtMs = "last_loop_at_ms"
        case lastScreenshotAtMs = "last_screenshot_at_ms"
        case lastBatchAtMs = "last_batch_at_ms"
        case pendingRequestCount = "pending_request_count"
        case lifecycle
    }
}

private struct CoreLifecycleStatus: Decodable {
    let snapshot: CoreLifecycleSnapshot
}

private struct CoreLifecycleSnapshot: Decodable {
    let userSession: String
    let primaryService: String
    let capturePermission: String
    let captureAvailability: String

    private enum CodingKeys: String, CodingKey {
        case userSession = "user_session"
        case primaryService = "primary_service"
        case capturePermission = "capture_permission"
        case captureAvailability = "capture_availability"
    }
}

final class MonitoringCoordinator: ObservableObject {
    @Published var email: String = ""
    @Published var password: String = ""
    @Published var deviceName: String = UIDevice.current.name
    @Published var baseApiUrlOverride: String = ""
    @Published var captureIntervalOverride: String = ""
    @Published var batchWindowOverride: String = ""

    @Published private(set) var statusMessage: String = "Not initialized"
    @Published private(set) var loggedIn: Bool = false
    @Published private(set) var deviceId: String = "<none>"
    @Published private(set) var monitoringEnabled: Bool = VirtueShared.defaultMonitoringEnabled
    @Published private(set) var monitorSummary: String = "idle"
    @Published private(set) var pendingRequestCount: Int = 0
    @Published private(set) var currentApiBaseUrl: String = VirtueShared.defaultBaseApiUrl
    @Published private(set) var lastCoreLoop: String = "<none>"
    @Published private(set) var lastCoreScreenshot: String = "<none>"
    @Published private(set) var lastCoreBatch: String = "<none>"
    @Published private(set) var coreUserSession: String = "unknown"
    @Published private(set) var corePrimaryService: String = "unknown"
    @Published private(set) var coreCapturePermission: String = "unknown"
    @Published private(set) var coreCaptureAvailability: String = "unknown"

    @Published private(set) var safariCaptureHealth: String = "No Safari extension heartbeat yet"
    @Published private(set) var safariPermissionSummary: String = "Unknown"
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

        statusMessage = "Signing in..."
        let trimmedEmail = email.trimmingCharacters(in: .whitespacesAndNewlines)
        let pw = password
        let trimmedDeviceName = deviceName.trimmingCharacters(in: .whitespacesAndNewlines)
        let resolvedDeviceName = trimmedDeviceName.isEmpty ? UIDevice.current.name : trimmedDeviceName

        Task { @MainActor in
            let error = await Task.detached(priority: .userInitiated) {
                NativeBridge.login(email: trimmedEmail, password: pw, deviceName: resolvedDeviceName)
            }.value
            if let error {
                statusMessage = "Login failed: \(error)"
                return
            }
            setMonitoringEnabled(true)
            refreshSessionState()
            refreshCoreStatus()
            refreshSafariStatus()
            statusMessage = "Signed in. Enable Virtue Safari extension in Safari settings."
        }
    }

    func logout() {
        setMonitoringEnabled(false)
        statusMessage = "Signing out..."

        Task { @MainActor in
            let error = await Task.detached(priority: .userInitiated) {
                NativeBridge.logout()
            }.value
            statusMessage = error.map { "Logout warning: \($0)" } ?? "Signed out"
            refreshSessionState()
            refreshCoreStatus()
            refreshSafariStatus()
        }
    }

    func toggleMonitoring() {
        let nextValue = !monitoringEnabled
        if !nextValue {
            if let error = NativeBridge.requestPauseMonitoring(source: "ios_pause_button") {
                statusMessage = "Pause request failed: \(error)"
                refreshCoreStatus()
                refreshSafariStatus()
                return
            }
        }
        setMonitoringEnabled(nextValue)
        refreshCoreStatus()
        refreshSafariStatus()
        statusMessage = nextValue
            ? "Monitoring resumed. Open Safari to restart capture."
            : "Monitoring paused. Safari capture will stop on the next extension heartbeat."
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
        monitoringEnabled = readMonitoringEnabledPreference()
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
            safariCaptureHealth = "App group unavailable"
            safariPermissionSummary = "Unavailable"
            safariLastHeartbeat = "<none>"
            safariLastFrame = "<none>"
            safariLastPage = "<none>"
            safariLastError = "<none>"
            safariDaemonStatus = "Unavailable"
            return
        }

        monitoringEnabled = readMonitoringEnabledPreference(defaults: defaults)

        if !monitoringEnabled {
            safariCaptureHealth = "Paused in Virtue"
            safariPermissionSummary = "Monitoring paused by user"
            safariLastHeartbeat = "<none>"
            safariLastFrame = "<none>"
            safariLastPage = "<none>"
            safariLastError = "<none>"
            safariDaemonStatus = "Paused"
            return
        }

        let now = Date().timeIntervalSince1970
        let lastHeartbeatAt = timestamp(forKey: VirtueShared.safariLastMessageAtKey, defaults: defaults)
        let lastFrameAt = timestamp(forKey: VirtueShared.safariLastFrameAtKey, defaults: defaults)
        let captureStateCode = defaults.object(forKey: VirtueShared.safariCaptureStateCodeKey) != nil
            ? defaults.integer(forKey: VirtueShared.safariCaptureStateCodeKey)
            : VirtueShared.captureStateUnknown

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
        safariPermissionSummary = deriveSafariPermissionSummary(
            captureStateCode: captureStateCode,
            lastError: lastError,
            hasRecentHeartbeat: lastHeartbeatAt != nil
        )

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

        let serviceStatus = loadCoreStatus()

        pendingRequestCount = serviceStatus?.pendingRequestCount ?? 0
        lastCoreLoop = formatMillisTimestamp(serviceStatus?.lastLoopAtMs)
        lastCoreScreenshot = formatMillisTimestamp(serviceStatus?.lastScreenshotAtMs)
        lastCoreBatch = formatMillisTimestamp(serviceStatus?.lastBatchAtMs)
        coreUserSession = normalizedLifecycleValue(serviceStatus?.lifecycle?.snapshot.userSession)
        corePrimaryService = normalizedLifecycleValue(serviceStatus?.lifecycle?.snapshot.primaryService)
        coreCapturePermission = normalizedLifecycleValue(
            serviceStatus?.lifecycle?.snapshot.capturePermission
        )
        coreCaptureAvailability = normalizedLifecycleValue(
            serviceStatus?.lifecycle?.snapshot.captureAvailability
        )

        if !loggedIn {
            monitorSummary = "signed out"
        } else if !monitoringEnabled {
            monitorSummary = "paused"
        } else if serviceStatus?.isRunning == true {
            monitorSummary = safariCaptureHealth == "Active in Safari" ? "active" : "waiting for Safari"
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
        monitoringEnabled = readMonitoringEnabledPreference(defaults: preferredDefaults)
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

    private func setMonitoringEnabled(_ enabled: Bool) {
        monitoringEnabled = enabled
        UserDefaults.standard.set(enabled, forKey: VirtueShared.monitoringEnabledKey)
        sharedDefaults?.set(enabled, forKey: VirtueShared.monitoringEnabledKey)
        if enabled {
            UserDefaults.standard.set(false, forKey: VirtueShared.safariPauseStopIssuedKey)
            sharedDefaults?.set(false, forKey: VirtueShared.safariPauseStopIssuedKey)
        }
    }

    private func readMonitoringEnabledPreference(defaults: UserDefaults? = nil) -> Bool {
        let defaults = defaults ?? sharedDefaults ?? UserDefaults.standard
        if defaults.object(forKey: VirtueShared.monitoringEnabledKey) == nil {
            defaults.set(
                VirtueShared.defaultMonitoringEnabled,
                forKey: VirtueShared.monitoringEnabledKey
            )
            return VirtueShared.defaultMonitoringEnabled
        }
        return defaults.bool(forKey: VirtueShared.monitoringEnabledKey)
    }

    private func storedOverride(forKey key: String, defaults: UserDefaults, fallback: String) -> String {
        let value = defaults.string(forKey: key)?.trimmingCharacters(in: .whitespacesAndNewlines)
        if key == VirtueShared.baseApiUrlKey, value == "http://10.7.7.4:8787" {
            return fallback
        }
        return (value?.isEmpty == false) ? value! : fallback
    }

    private func loadCoreStatus() -> CoreServiceStatus? {
        guard let json = NativeBridge.getStatusJson(),
              let data = json.data(using: .utf8),
              !data.isEmpty
        else {
            return nil
        }
        return try? JSONDecoder().decode(CoreServiceStatus.self, from: data)
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

    private func normalizedLifecycleValue(_ value: String?) -> String {
        guard let value, !value.isEmpty else {
            return "unknown"
        }
        return value.replacingOccurrences(of: "_", with: " ")
    }

    private func deriveSafariPermissionSummary(
        captureStateCode: Int,
        lastError: String?,
        hasRecentHeartbeat: Bool
    ) -> String {
        switch captureStateCode {
        case VirtueShared.captureStateReady:
            return "Ready in Safari"
        case VirtueShared.captureStatePermissionMissing:
            if let lastError, !lastError.isEmpty {
                return "Access missing: \(lastError)"
            }
            return "Safari extension access is missing"
        case VirtueShared.captureStateSessionUnavailable:
            return hasRecentHeartbeat
                ? "Safari extension reachable, but capture is unavailable for the current page"
                : "Open Safari to refresh extension state"
        default:
            return hasRecentHeartbeat
                ? "Waiting for a capturable Safari page"
                : "Extension state unknown until Safari sends a heartbeat"
        }
    }
}
