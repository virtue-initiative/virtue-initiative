import Combine
import Foundation
import UIKit

/// One entry in the daemon's recent-errors ring — `virtue_core::StatusError`.
struct CoreStatusError: Decodable {
    let atMs: Int64
    let context: String
    let message: String

    private enum CodingKeys: String, CodingKey {
        case atMs = "at_ms"
        case context
        case message
    }
}

/// Why the most recent capture attempt produced no screenshot —
/// `virtue_core::StatusSkipReason`.
enum CoreSkipReason: String, Decodable {
    case staticScreen = "static_screen"
    case lockedOrScreensaver = "locked_or_screensaver"
    case captureFailed = "capture_failed"
    case unknown

    init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = CoreSkipReason(rawValue: raw) ?? .unknown
    }

    var label: String {
        switch self {
        case .staticScreen: return "Screen unchanged since the last upload"
        case .lockedOrScreensaver: return "Screen locked or screensaver active"
        case .captureFailed: return "Capture failed"
        case .unknown: return "Unknown"
        }
    }
}

/// Mirrors `virtue_core::ServiceStatus` — the shared status-page payload
/// (`client/core/SPEC.md` CORE-010) every platform renders. Kept as a local
/// copy because the iOS app doesn't link `shared-swift`'s VirtueKit the way
/// the Mac app does.
struct CoreServiceStatus: Decodable {
    let isRunning: Bool
    let isAuthenticated: Bool
    let accountEmail: String?
    let deviceId: String?
    let deviceName: String?
    let partnerCount: Int?
    let pendingHashCount: Int?
    let pendingBatchCount: Int?
    let pendingRequestCount: Int
    let lastLoopAtMs: Int64?
    let lastScreenshotAttemptAtMs: Int64?
    let lastScreenshotAtMs: Int64?
    let lastSkipReason: CoreSkipReason?
    let lastBatchAtMs: Int64?
    let recentErrors: [CoreStatusError]?
    let apiBaseUrl: String?
    let hashBaseUrl: String?
    let captureIntervalSeconds: Int64?
    let batchWindowSeconds: Int64?

    private enum CodingKeys: String, CodingKey {
        case isRunning = "is_running"
        case isAuthenticated = "is_authenticated"
        case accountEmail = "account_email"
        case deviceId = "device_id"
        case deviceName = "device_name"
        case partnerCount = "partner_count"
        case pendingHashCount = "pending_hash_count"
        case pendingBatchCount = "pending_batch_count"
        case pendingRequestCount = "pending_request_count"
        case lastLoopAtMs = "last_loop_at_ms"
        case lastScreenshotAttemptAtMs = "last_screenshot_attempt_at_ms"
        case lastScreenshotAtMs = "last_screenshot_at_ms"
        case lastSkipReason = "last_skip_reason"
        case lastBatchAtMs = "last_batch_at_ms"
        case recentErrors = "recent_errors"
        case apiBaseUrl = "api_base_url"
        case hashBaseUrl = "hash_base_url"
        case captureIntervalSeconds = "capture_interval_seconds"
        case batchWindowSeconds = "batch_window_seconds"
    }
}

final class MonitoringCoordinator: ObservableObject {
    @Published var email: String = ""
    @Published var password: String = ""
    @Published var deviceName: String = UIDevice.current.name

    @Published private(set) var statusMessage: String = "Not initialized"
    @Published private(set) var isSigningIn: Bool = false
    @Published private(set) var isSigningOut: Bool = false
    @Published private(set) var loginError: String? = nil
    /// False while the pairing-code view is showing (CORE-020), true once the
    /// user has picked "Use a password instead" (CORE-008).
    @Published private(set) var usePasswordLogin: Bool = false
    @Published private(set) var pendingUserCode: String?
    @Published private(set) var isRequestingCode: Bool = false
    @Published private(set) var loggedIn: Bool = false
    @Published private(set) var deviceId: String = "<none>"
    @Published private(set) var accountEmail: String?
    @Published private(set) var monitoringEnabled: Bool = VirtueShared.defaultMonitoringEnabled
    @Published private(set) var monitorSummary: String = "idle"
    @Published private(set) var pendingRequestCount: Int = 0
    @Published private(set) var currentApiBaseUrl: String = VirtueShared.defaultBaseApiUrl
    @Published private(set) var lastCoreLoop: String = "<none>"
    @Published private(set) var lastCoreScreenshot: String = "<none>"
    @Published private(set) var lastCoreBatch: String = "<none>"
    @Published private(set) var lastCoreScreenshotAttempt: String = "<none>"
    /// The full shared status payload (CORE-010) the Status Details sheet
    /// renders; the scalars above stay for the main screen's own bindings.
    @Published private(set) var coreStatus: CoreServiceStatus?

    @Published private(set) var safariCaptureHealth: String = "No Safari extension heartbeat yet"
    @Published private(set) var safariPermissionSummary: String = "Unknown"
    @Published private(set) var safariLastHeartbeat: String = "<none>"
    @Published private(set) var safariLastFrame: String = "<none>"
    @Published private(set) var safariLastPage: String = "<none>"
    @Published private(set) var safariLastError: String = "<none>"
    @Published private(set) var safariDaemonStatus: String = "Not started yet"

    private var didBecomeActiveObserver: NSObjectProtocol?
    private var willEnterForegroundObserver: NSObjectProtocol?
    private var statusRefreshTimer: Timer?
    /// Drives CORE-021 at the interval the server asked for, separately from
    /// the app-wide status refresh above.
    private var codePollTimer: Timer?

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

        monitoringEnabled = readMonitoringEnabledPreference(defaults: sharedDefaults ?? UserDefaults.standard)
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
        stopCodePolling()
    }

    // MARK: - Pairing-code login (CORE-020 / CORE-021)

    func showPasswordLogin() {
        stopCodePolling()
        pendingUserCode = nil
        loginError = nil
        usePasswordLogin = true
    }

    func showCodeLogin() {
        loginError = nil
        usePasswordLogin = false
    }

    /// Asks the core for a pairing code and starts polling for its approval.
    /// Nothing about the signed-in state changes until a poll comes back
    /// approved — no device exists before then.
    func beginCodeLogin() {
        stopCodePolling()
        loginError = nil
        isRequestingCode = true
        statusMessage = "Getting a code..."
        let resolvedDeviceName = resolvedDeviceName()
        let configDirPath = configDir.path
        let dataDirPath = dataDir.path

        Task { @MainActor in
            let result = await Task.detached(priority: .userInitiated) {
                () -> Result<CodeLoginStart, String> in
                if let initError = NativeBridge.ensureInitialized(
                    configDir: configDirPath,
                    dataDir: dataDirPath
                ) {
                    return .failure(initError)
                }
                return NativeBridge.beginCodeLogin(deviceName: resolvedDeviceName)
            }.value
            isRequestingCode = false

            switch result {
            case .failure(let message):
                pendingUserCode = nil
                loginError = message
                statusMessage = "Could not get a code: \(message)"
            case .success(let start):
                pendingUserCode = start.userCode
                statusMessage = "Enter the code shown here on the Virtue website."
                startCodePolling(
                    intervalSeconds: start.intervalSeconds ?? defaultCodeLoginIntervalSeconds
                )
            }
        }
    }

    private func startCodePolling(intervalSeconds: Int) {
        let interval = TimeInterval(max(1, intervalSeconds))
        codePollTimer = Timer.scheduledTimer(withTimeInterval: interval, repeats: true) {
            [weak self] _ in
            Task { @MainActor in
                self?.pollCodeLogin()
            }
        }
    }

    private func stopCodePolling() {
        codePollTimer?.invalidate()
        codePollTimer = nil
    }

    private func pollCodeLogin() {
        let configDirPath = configDir.path
        let dataDirPath = dataDir.path

        Task { @MainActor in
            let outcome = await Task.detached(priority: .userInitiated) { () -> CodeLoginPoll in
                if let initError = NativeBridge.ensureInitialized(
                    configDir: configDirPath,
                    dataDir: dataDirPath
                ) {
                    return .failed(initError)
                }
                return NativeBridge.pollCodeLogin()
            }.value

            switch outcome {
            case .pending:
                return
            case .failed:
                // Usually a transient network blip. Keep the code on screen and
                // try again on the next tick rather than making the user fetch
                // a new one.
                return
            case .approved(let accountEmail):
                stopCodePolling()
                pendingUserCode = nil
                password = ""
                setMonitoringEnabled(true)
                // App-layer state used only to prefill the bug-report contact
                // field. In this flow the device never sees the email until the
                // server sends it (API-045).
                if let accountEmail {
                    UserDefaults.standard.set(accountEmail, forKey: VirtueShared.accountEmailKey)
                }
                refreshSessionState()
                refreshCoreStatus()
                refreshSafariStatus()
                statusMessage = "Signed in. Enable Virtue Safari extension in Safari settings."
            case .expired:
                stopCodePolling()
                pendingUserCode = nil
                loginError = "That code expired. Get a new one to sign in."
                statusMessage = "That code expired. Get a new one to sign in."
            }
        }
    }

    private func resolvedDeviceName() -> String {
        let trimmed = deviceName.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? UIDevice.current.name : trimmed
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
        loginError = nil
        isSigningIn = true
        let trimmedEmail = email.trimmingCharacters(in: .whitespacesAndNewlines)
        let pw = password
        let resolvedDeviceName = resolvedDeviceName()
        let configDirPath = configDir.path
        let dataDirPath = dataDir.path

        Task { @MainActor in
            let error = await Task.detached(priority: .userInitiated) { () -> String? in
                if let initError = NativeBridge.ensureInitialized(
                    configDir: configDirPath,
                    dataDir: dataDirPath
                ) {
                    return initError
                }
                return NativeBridge.login(email: trimmedEmail, password: pw, deviceName: resolvedDeviceName)
            }.value
            isSigningIn = false
            if let error {
                statusMessage = "Login failed: \(error)"
                loginError = error
                return
            }
            password = ""
            setMonitoringEnabled(true)
            // Core's AuthState/DeviceCredentials don't carry the account email, so this
            // is app-layer state persisted here purely to prefill the bug-report form's
            // contact-email field — mirrors the Android client's AccountEmailStore.
            UserDefaults.standard.set(trimmedEmail, forKey: VirtueShared.accountEmailKey)
            refreshSessionState()
            refreshCoreStatus()
            refreshSafariStatus()
            statusMessage = "Signed in. Enable Virtue Safari extension in Safari settings."
        }
    }

    func logout() {
        stopCodePolling()
        pendingUserCode = nil
        setMonitoringEnabled(false)
        statusMessage = "Signing out..."

        isSigningOut = true
        let configDirPath = configDir.path
        let dataDirPath = dataDir.path
        Task { @MainActor in
            let error = await Task.detached(priority: .userInitiated) {
                NativeBridge.ensureInitialized(configDir: configDirPath, dataDir: dataDirPath)
                return NativeBridge.logout()
            }.value
            isSigningOut = false
            statusMessage = error.map { "Logout warning: \($0)" } ?? "Signed out"
            UserDefaults.standard.removeObject(forKey: VirtueShared.accountEmailKey)
            refreshSessionState()
            refreshCoreStatus()
            refreshSafariStatus()
        }
    }

    func toggleMonitoring() {
        let nextValue = !monitoringEnabled
        guard !nextValue else {
            Task { @MainActor in
                let error = await Task.detached(priority: .userInitiated) {
                    NativeBridge.requestResumeMonitoring()
                }.value
                if let error {
                    statusMessage = "Resume request failed: \(error)"
                    refreshCoreStatus()
                    refreshSafariStatus()
                    return
                }
                setMonitoringEnabled(true)
                refreshCoreStatus()
                refreshSafariStatus()
                statusMessage = "Monitoring resumed. Open Safari to restart capture."
            }
            return
        }

        Task { @MainActor in
            let error = await Task.detached(priority: .userInitiated) {
                NativeBridge.requestPauseMonitoring(source: "ios_pause_button")
            }.value
            if let error {
                statusMessage = "Pause request failed: \(error)"
                refreshCoreStatus()
                refreshSafariStatus()
                return
            }
            setMonitoringEnabled(false)
            refreshCoreStatus()
            refreshSafariStatus()
            statusMessage = "Monitoring paused. Safari capture will stop on the next extension heartbeat."
        }
    }

    /// Submits a bug report, invoking `completion` with `nil` on success or an
    /// error message on failure. Off-main like every other native call that
    /// touches the network/daemon.
    func submitBugReport(
        message: String,
        contactEmail: String?,
        includeLogs: Bool,
        completion: @escaping (String?) -> Void
    ) {
        let details = platformDetails()
        Task { @MainActor in
            let error = await Task.detached(priority: .userInitiated) {
                NativeBridge.reportIssue(
                    message: message,
                    contactEmail: contactEmail,
                    includeLogs: includeLogs,
                    platformDetails: details
                )
            }.value
            completion(error)
        }
    }

    /// Best-effort "iOS <version> (<model>)" string, e.g. `"iOS 17.5.1 (iPhone)"`.
    private func platformDetails() -> String {
        let device = UIDevice.current
        return "\(device.systemName) \(device.systemVersion) (\(device.model))"
    }

    private func initializeCore() {
        do {
            try FileManager.default.createDirectory(at: configDir, withIntermediateDirectories: true)
            try FileManager.default.createDirectory(at: dataDir, withIntermediateDirectories: true)
        } catch {
            statusMessage = "Directory setup failed: \(error.localizedDescription)"
            return
        }

        let error = NativeBridge.ensureInitialized(
            configDir: configDir.path,
            dataDir: dataDir.path
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
        accountEmail = UserDefaults.standard.string(forKey: VirtueShared.accountEmailKey)
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
            safariDaemonStatus = "Not started yet"
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
        let serviceStatus = loadCoreStatus()

        pendingRequestCount = serviceStatus?.pendingRequestCount ?? 0
        lastCoreLoop = formatMillisTimestamp(serviceStatus?.lastLoopAtMs)
        lastCoreScreenshot = formatMillisTimestamp(serviceStatus?.lastScreenshotAtMs)
        lastCoreScreenshotAttempt = formatMillisTimestamp(serviceStatus?.lastScreenshotAttemptAtMs)
        lastCoreBatch = formatMillisTimestamp(serviceStatus?.lastBatchAtMs)
        coreStatus = serviceStatus
        if let apiBaseUrl = serviceStatus?.apiBaseUrl, !apiBaseUrl.isEmpty {
            currentApiBaseUrl = apiBaseUrl
        }

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

    /// Local time plus a relative age, for the status sheet's timestamps —
    /// "when did this last work?" is what those rows are really answering.
    func formatStatusTimestamp(_ timestampMs: Int64?) -> String {
        guard let timestampMs else {
            return "<none>"
        }
        let date = Date(timeIntervalSince1970: TimeInterval(timestampMs) / 1000)
        let relative = RelativeDateTimeFormatter()
        relative.unitsStyle = .short
        return "\(formatAbsoluteTime(date.timeIntervalSince1970)) (\(relative.localizedString(for: date, relativeTo: Date())))"
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
