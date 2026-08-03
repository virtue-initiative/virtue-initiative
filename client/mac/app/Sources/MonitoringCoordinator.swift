import AppKit
import Combine
import Foundation
import ServiceManagement
import VirtueKit

enum PermissionPhase {
    case needsRequest
    case needsRelaunch
}

/// UserDefaults keys + built-in defaults for runtime overrides. Capture/batch
/// defaults are read from the Rust core via NativeBridge rather than
/// duplicated here. Blank fields mean "use the built-in default" — the FFI
/// layer omits blank keys from the override JSON entirely.
private enum OverrideDefaults {
    static let baseApiUrlKey = "VIRTUE_BASE_API_URL"
    static let captureIntervalKey = "VIRTUE_CAPTURE_INTERVAL_SECONDS"
    static let batchWindowKey = "VIRTUE_BATCH_WINDOW_SECONDS"

    static let baseApiUrl = "https://api.virtueinitiative.org"
    static let captureIntervalSeconds = String(NativeBridge.defaultCaptureIntervalSeconds())
    static let batchWindowSeconds = String(NativeBridge.defaultBatchWindowSeconds())
}

/// Faithfully ports `main.rs`'s tray event loop state machine: the daemon
/// poll cadence, the `STOPPED_TIMEOUT` grace period that tolerates a brief
/// launchd restart race, the "Unreachable" (alive-but-busy) distinction, and
/// the post-relaunch grace window. Getting this wrong means the UI could
/// flicker "stopped" during a normal launchd restart or a slow post-wake
/// batch flush.
@MainActor
final class MonitoringCoordinator: ObservableObject {
    private static let statusPollInterval: TimeInterval = 2
    private static let stoppedTimeout: TimeInterval = 20
    private static let postRelaunchGrace: TimeInterval = 30

    @Published var email: String = ""
    @Published var password: String = ""
    @Published var deviceName: String = NativeBridge.defaultDeviceName()

    @Published private(set) var loggedIn: Bool = false
    @Published private(set) var isSigningIn: Bool = false
    @Published private(set) var isSigningOut: Bool = false
    @Published private(set) var loginError: String?
    @Published private(set) var deviceId: String = "<none>"

    @Published private(set) var daemonStatus: DaemonStatus = .stopped
    @Published private(set) var unexpectedStopMessage: String?
    @Published private(set) var permissionPhase: PermissionPhase?
    @Published private(set) var isRelaunching: Bool = false
    @Published private(set) var relaunchError: String?

    @Published private(set) var pendingRequestCount: Int = 0
    @Published private(set) var lastLoopAt: String = "<none>"

    @Published var baseApiUrlOverride: String = ""
    @Published var captureIntervalOverride: String = ""
    @Published var batchWindowOverride: String = ""
    @Published private(set) var overridesMessage: String?

    let buildLabel = NativeBridge.getBuildLabel()

    private var statusTimer: Timer?
    private var isPolling = false
    private var relaunching = false
    private var gracefulShutdown = false
    private var stoppedSince: Date?
    private var postRelaunchGraceUntil: Date?

    private lazy var daemonExePath: String =
        NativeBridge.daemonExePath(appBundlePath: Bundle.main.bundlePath)

    init() {
        registerAsLoginItem()
        loadOverrideInputs()

        let initError = NativeBridge.initialize()
        if let initError {
            unexpectedStopMessage = "Core initialization failed: \(initError)"
        }
        refreshSessionState()
        // Only ever set from `login()` before this — meaning a relaunch
        // while already logged in (persisted credentials from a prior
        // session/install) left this at its default `nil` forever, so the
        // permission card silently never appeared regardless of actual TCC
        // status. This is a mere Preflight check, not a request, so it's
        // safe to call unconditionally at every launch.
        if loggedIn {
            refreshPermissionPhase()
        }

        // `ensure_daemon_running` below does a `launchctl kickstart -k`,
        // which force-restarts the daemon on *every* launch — not just
        // explicit relaunches — including right after the system's own
        // "Restart" button relaunches the app post permission-grant. Without
        // this grace window, a normal cold start that takes longer than
        // `stoppedTimeout` to come back up gets misreported as "stopped
        // unexpectedly" before it's had a chance to finish starting.
        postRelaunchGraceUntil = Date().addingTimeInterval(Self.postRelaunchGrace)

        Task {
            let error = await Task.detached(priority: .userInitiated) { [daemonExePath, overrides = runtimeOverrides()] () -> String? in
                _ = NativeBridge.setOverrides(overrides)
                return NativeBridge.ensureDaemonRunning(daemonExePath: daemonExePath)
            }.value
            // Previously discarded entirely: if `launchctl bootstrap` fails
            // (e.g. leftover launchd state from a prior crash/kill), the app
            // would silently have no running daemon and only ever report a
            // generic "stopped unexpectedly" after the grace window expired,
            // with no indication of the actual cause.
            if let error {
                unexpectedStopMessage = "Failed to start background service: \(error)"
            }
        }

        startStatusTimerIfNeeded()
    }

    deinit {
        statusTimer?.invalidate()
    }

    // MARK: - Login / logout

    func login() {
        guard !email.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            loginError = "Email is required"
            return
        }
        guard !password.isEmpty else {
            loginError = "Password is required"
            return
        }

        loginError = nil
        isSigningIn = true
        let trimmedEmail = email.trimmingCharacters(in: .whitespacesAndNewlines)
        let pw = password
        let trimmedDeviceName = deviceName.trimmingCharacters(in: .whitespacesAndNewlines)
        let resolvedDeviceName = trimmedDeviceName.isEmpty
            ? NativeBridge.defaultDeviceName()
            : trimmedDeviceName

        Task {
            let error = await Task.detached(priority: .userInitiated) {
                NativeBridge.login(email: trimmedEmail, password: pw, deviceName: resolvedDeviceName)
            }.value
            isSigningIn = false
            if let error {
                loginError = loginErrorMessage(error)
                return
            }
            password = ""
            refreshSessionState()
            // Do not auto-request screen-capture access here: macOS shows its
            // prompt only once per launch, so triggering it now would consume
            // the one-shot before the user clicks "Request Permissions".
            refreshPermissionPhase()
        }
    }

    func logout() {
        isSigningOut = true
        Task {
            _ = await Task.detached(priority: .userInitiated) {
                NativeBridge.logout()
            }.value
            isSigningOut = false
            refreshSessionState()
        }
    }

    /// Stops the background daemon (if registered) and quits. When the user
    /// was logged in, this is tagged as a user-initiated stop so the daemon
    /// records a clean user stop (fires a stop-time alert) rather than being
    /// classified as an unexpected `Other` stop that would trigger an
    /// unexpected-start alert on next launch. When logged out, the daemon is
    /// still stopped (matching `main.rs`'s plain "Quit" behavior) but not
    /// tagged as a user stop.
    func stopMonitoringAndQuit() {
        gracefulShutdown = true
        let wasLoggedIn = loggedIn
        Task {
            if NativeBridge.agentIsRegistered() {
                _ = await Task.detached(priority: .userInitiated) {
                    NativeBridge.stopDaemon(userInitiated: wasLoggedIn)
                }.value
            }
            NSApplication.shared.terminate(nil)
        }
    }

    // MARK: - Permissions

    func requestPermissions() {
        // Spawns a throwaway `screencapture` subprocess to force the TCC
        // prompt; keep it off the main actor like every other native call
        // that can block.
        Task {
            let granted = await Task.detached(priority: .userInitiated) {
                NativeBridge.requestCapturePermission()
            }.value
            permissionPhase = granted ? nil : .needsRelaunch
        }
    }

    func relaunchToAcceptPermissions() {
        relaunching = true
        isRelaunching = true
        relaunchError = nil
        Task {
            let error = await Task.detached(priority: .userInitiated) { [daemonExePath] in
                NativeBridge.relaunchDaemon(daemonExePath: daemonExePath)
            }.value
            relaunching = false
            isRelaunching = false
            if let error {
                relaunchError = error
                return
            }
            postRelaunchGraceUntil = Date().addingTimeInterval(Self.postRelaunchGrace)
            // The app process itself doesn't restart, so a local TCC query may
            // still return false even though the relaunched daemon has the
            // permission. Update the UI directly rather than waiting for a poll.
            permissionPhase = nil
            unexpectedStopMessage = nil
        }
    }

    // MARK: - Runtime overrides

    /// Writes overrides to `config.json`, which the daemon's `ConfigModule`
    /// hot-reloads on its next `Ping` — no relaunch required.
    func applyOverrides() {
        let overrides = runtimeOverrides()
        persistOverrides(overrides)
        overridesMessage = "Applying…"
        Task {
            let error = await Task.detached(priority: .userInitiated) {
                NativeBridge.setOverrides(overrides)
            }.value
            overridesMessage = error.map { "Override update failed: \($0)" } ?? "Runtime overrides updated"
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
        let defaults = UserDefaults.standard
        defaults.set(overrides.baseApiUrl, forKey: OverrideDefaults.baseApiUrlKey)
        defaults.set(overrides.captureIntervalSeconds, forKey: OverrideDefaults.captureIntervalKey)
        defaults.set(overrides.batchWindowSeconds, forKey: OverrideDefaults.batchWindowKey)
    }

    private func loadOverrideInputs() {
        let defaults = UserDefaults.standard
        baseApiUrlOverride = storedOverride(
            forKey: OverrideDefaults.baseApiUrlKey,
            defaults: defaults,
            fallback: OverrideDefaults.baseApiUrl
        )
        captureIntervalOverride = storedOverride(
            forKey: OverrideDefaults.captureIntervalKey,
            defaults: defaults,
            fallback: OverrideDefaults.captureIntervalSeconds
        )
        batchWindowOverride = storedOverride(
            forKey: OverrideDefaults.batchWindowKey,
            defaults: defaults,
            fallback: OverrideDefaults.batchWindowSeconds
        )
    }

    private func storedOverride(forKey key: String, defaults: UserDefaults, fallback: String) -> String {
        let value = defaults.string(forKey: key)?.trimmingCharacters(in: .whitespacesAndNewlines)
        return (value?.isEmpty == false) ? value! : fallback
    }

    // MARK: - Status polling

    private func startStatusTimerIfNeeded() {
        guard statusTimer == nil else {
            return
        }
        statusTimer = Timer.scheduledTimer(withTimeInterval: Self.statusPollInterval, repeats: true) { [weak self] _ in
            Task { @MainActor in
                self?.pollTick()
            }
        }
    }

    /// Everything a poll tick needs, fetched off-main in one batch. `pollDaemonStatus`
    /// and `getStatusJson` both do a real blocking socket round-trip to the daemon
    /// (unlike iOS, which reads in-process state) — every native call that touches
    /// the socket must run off the main actor, or the UI freezes for however long
    /// the daemon takes to answer. This bit us before; don't regress it.
    private struct PolledSnapshot {
        let status: DaemonStatus
        let statusJson: String?
        let loggedIn: Bool
        let deviceId: String?
    }

    private func pollTick() {
        guard !relaunching, !isPolling else {
            return
        }
        isPolling = true
        Task {
            let snapshot = await Task.detached(priority: .utility) { () -> PolledSnapshot in
                let status = NativeBridge.pollDaemonStatus()
                guard status == .running else {
                    return PolledSnapshot(status: status, statusJson: nil, loggedIn: false, deviceId: nil)
                }
                return PolledSnapshot(
                    status: status,
                    statusJson: NativeBridge.getStatusJson(),
                    loggedIn: NativeBridge.isLoggedIn(),
                    deviceId: NativeBridge.getDeviceId()
                )
            }.value
            applyPolledSnapshot(snapshot)
            isPolling = false
        }
    }

    private func applyPolledSnapshot(_ snapshot: PolledSnapshot) {
        // Only suppresses the "stopped unexpectedly" false-negative during a
        // relaunch/launch restart race — must NOT also suppress recognizing
        // a genuinely successful `.running` poll, or the UI is stuck showing
        // "Starting…" for the whole grace window even when the daemon came
        // up within a second or two, as it normally does.
        let inGracePeriod = relaunching || gracefulShutdown
            || (postRelaunchGraceUntil.map { Date() < $0 } ?? false)

        switch snapshot.status {
        case .unreachable:
            // Connected but slow to answer (e.g. flushing a batch after
            // wake) — the daemon is alive but busy. Never treat as gone.
            stoppedSince = nil
            return
        case .stopped:
            if inGracePeriod {
                stoppedSince = nil
                return
            }
            let since = stoppedSince ?? Date()
            stoppedSince = since
            if Date().timeIntervalSince(since) < Self.stoppedTimeout {
                return
            }
            daemonStatus = .stopped
            unexpectedStopMessage =
                "Virtue background service stopped unexpectedly.\n\nRelaunch the Virtue app to continue monitoring."
            return
        case .running:
            stoppedSince = nil
            daemonStatus = .running
            unexpectedStopMessage = nil
        }

        loggedIn = snapshot.loggedIn
        deviceId = snapshot.deviceId ?? "<none>"
        if let json = snapshot.statusJson, let status = CoreServiceStatus.decode(fromJson: json) {
            pendingRequestCount = status.pendingRequestCount
            lastLoopAt = status.lastLoopAtMs.map(formatMillisTimestamp) ?? "<none>"
        }
        // Local TCC cache check, not IPC — cheap enough to run on the main actor.
        if NativeBridge.hasCapturePermission() {
            permissionPhase = nil
        }
    }

    private func refreshSessionState() {
        loggedIn = NativeBridge.isLoggedIn()
        deviceId = NativeBridge.getDeviceId() ?? "<none>"
    }

    private func refreshPermissionPhase() {
        permissionPhase = NativeBridge.hasCapturePermission() ? nil : .needsRequest
    }

    /// Only the daemon is a `LaunchAgent` (`RunAtLoad`); the app itself (and
    /// therefore the tray icon) has no way to come back after a restart
    /// unless it's separately registered as a login item. `SMAppService`
    /// (macOS 13+) does this without us hand-managing a second LaunchAgent
    /// plist — register is idempotent, so it's safe to call on every launch.
    private func registerAsLoginItem() {
        guard SMAppService.mainApp.status != .enabled else {
            return
        }
        do {
            try SMAppService.mainApp.register()
        } catch {
            print("Failed to register Virtue as a login item: \(error)")
        }
    }

    private func loginErrorMessage(_ raw: String) -> String {
        let lower = raw.lowercased()
        if lower.contains("unauthorized") || lower.contains("bad request") || lower.contains("login failed") {
            return "Login failed. Check your email and password and try again."
        }
        return "Login failed: \(raw)"
    }

    private func formatMillisTimestamp(_ timestampMs: Int64) -> String {
        let formatter = DateFormatter()
        formatter.dateStyle = .none
        formatter.timeStyle = .medium
        return formatter.string(from: Date(timeIntervalSince1970: TimeInterval(timestampMs) / 1000))
    }
}
