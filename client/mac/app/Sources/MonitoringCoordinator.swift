import AppKit
import Combine
import Foundation
import ServiceManagement
import VirtueKit

enum PermissionPhase {
    case needsRequest
    case needsRelaunch
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
    @Published private(set) var accountEmail: String?

    @Published private(set) var daemonStatus: DaemonStatus = .stopped
    @Published private(set) var unexpectedStopMessage: String?
    @Published private(set) var permissionPhase: PermissionPhase?
    @Published private(set) var isRelaunching: Bool = false
    @Published private(set) var relaunchError: String?

    @Published private(set) var pendingRequestCount: Int = 0
    @Published private(set) var lastLoopAt: String = "<none>"
    /// The full shared status payload (CORE-010) the Status Details sheet
    /// renders. The scalars above stay for the main window's own bindings.
    @Published private(set) var coreStatus: CoreServiceStatus?

    @Published private(set) var isForceCapturing: Bool = false
    @Published private(set) var forceCaptureMessage: String?

    let buildLabel = NativeBridge.getBuildLabel()

    private var statusTimer: Timer?
    private var isPolling = false
    /// `VirtueBuildLabel` as it was on disk when this process launched, for
    /// detecting that the bundle has since been replaced. See
    /// `checkForReplacedBundle`.
    private let launchedBuildLabel =
        Bundle.main.object(forInfoDictionaryKey: "VirtueBuildLabel") as? String ?? ""
    private var pendingReplacedBundleLabel: String?
    /// One-shot: `openApplication` is async and the poll fires every 2s, so
    /// without this a slow handover would spawn a burst of new instances.
    private var isRelaunchingIntoNewBundle = false
    /// Set by the app once the updater exists; nil in builds without
    /// auto-update wired in.
    weak var updateController: UpdateController?
    private var relaunching = false
    private var gracefulShutdown = false
    private var stoppedSince: Date?
    private var postRelaunchGraceUntil: Date?

    private lazy var daemonExePath: String =
        NativeBridge.daemonExePath(appBundlePath: Bundle.main.bundlePath)

    init() {
        terminateOtherInstances()
        registerAsLoginItem()

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
            let error = await Task.detached(priority: .userInitiated) { [daemonExePath] () -> String? in
                NativeBridge.ensureDaemonRunning(daemonExePath: daemonExePath)
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

    /// Submits a bug report, invoking `completion` with `nil` on success or an
    /// error message on failure. Off-main like every other native call that
    /// touches the network/daemon.
    func submitBugReport(
        message: String,
        contactEmail: String?,
        includeLogs: Bool,
        completion: @escaping (String?) -> Void
    ) {
        Task {
            let error = await Task.detached(priority: .userInitiated) {
                NativeBridge.reportIssue(message: message, contactEmail: contactEmail, includeLogs: includeLogs)
            }.value
            completion(error)
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

    /// Forces an immediate screenshot capture, bypassing the normal
    /// interval-due gate (still honors the locked/screensaver and
    /// fingerprint-dedup gates). Shows a transient confirmation/error
    /// message that clears itself after a few seconds, rather than sticking
    /// around forever.
    func forceCapture() {
        guard loggedIn, !isForceCapturing else {
            return
        }
        isForceCapturing = true
        forceCaptureMessage = "Capturing screenshot…"
        Task {
            let error = await Task.detached(priority: .userInitiated) {
                NativeBridge.forceCapture()
            }.value
            isForceCapturing = false
            let message = error.map { "Force screenshot failed: \($0)" } ?? "Screenshot captured and uploading"
            forceCaptureMessage = message
            try? await Task.sleep(nanoseconds: 3_000_000_000)
            // Only clear if a later call hasn't already replaced this message.
            if forceCaptureMessage == message {
                forceCaptureMessage = nil
            }
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
        checkForReplacedBundle()
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
            coreStatus = status
        }
        // Local TCC cache check, not IPC — cheap enough to run on the main actor.
        if NativeBridge.hasCapturePermission() {
            permissionPhase = nil
        }
    }

    private func refreshSessionState() {
        loggedIn = NativeBridge.isLoggedIn()
        deviceId = NativeBridge.getDeviceId() ?? "<none>"
        accountEmail = NativeBridge.getAccountEmail()
    }

    private func refreshPermissionPhase() {
        permissionPhase = NativeBridge.hasCapturePermission() ? nil : .needsRequest
    }

    // MARK: - Stale instances (issue #539)

    /// Dragging a new `Virtue.app` over a running one leaves the *old* app
    /// process alive, and opening the new one then yields two menu bar icons
    /// backed by two different app versions — both talking to one daemon.
    /// Newest launch wins: terminate any other running instance of this
    /// bundle id before doing anything else.
    ///
    /// This is only about duplicate *app* processes. The daemon is a
    /// LaunchAgent, singleton by construction, and is restarted a few lines
    /// later by `ensureDaemonRunning`.
    private func terminateOtherInstances() {
        let others = NSRunningApplication.runningApplications(
            withBundleIdentifier: Bundle.main.bundleIdentifier ?? ""
        ).filter { $0.processIdentifier != ProcessInfo.processInfo.processIdentifier }

        for other in others {
            // Not `forceTerminate`: a normal terminate lets the old instance
            // unwind cleanly. It has no unsaved state and no quit handler
            // that records a user stop, so this does not look like tampering
            // to the daemon.
            other.terminate()
        }
    }

    /// The other half of the drag-install problem: this process is now
    /// running code from a bundle that has been replaced on disk. Detect it
    /// by re-reading `VirtueBuildLabel` from the on-disk `Info.plist` (the
    /// in-memory `Bundle` copy is cached at launch and never changes) and
    /// relaunch into the new version, which will in turn kickstart the
    /// daemon onto the new binary.
    ///
    /// Requires two consecutive polls to agree before acting, so a bundle
    /// caught mid-copy can't trigger a relaunch into a half-written app.
    private func checkForReplacedBundle() {
        guard !isRelaunchingIntoNewBundle else {
            return
        }
        // Sparkle does its own quit-install-relaunch dance; racing it with a
        // second relaunch would be a mess.
        guard !isUpdateInProgress() else {
            pendingReplacedBundleLabel = nil
            return
        }

        let infoPlistURL = Bundle.main.bundleURL
            .appendingPathComponent("Contents")
            .appendingPathComponent("Info.plist")
        guard
            let onDisk = NSDictionary(contentsOf: infoPlistURL),
            let diskLabel = onDisk["VirtueBuildLabel"] as? String,
            !diskLabel.isEmpty,
            diskLabel != launchedBuildLabel
        else {
            pendingReplacedBundleLabel = nil
            return
        }

        guard pendingReplacedBundleLabel == diskLabel else {
            pendingReplacedBundleLabel = diskLabel
            return
        }

        relaunchSelf()
    }

    private func isUpdateInProgress() -> Bool {
        updateController?.isUpdateSessionInProgress ?? false
    }

    /// Relaunch this app from its (new) bundle and exit. `open` is used
    /// rather than re-exec'ing so the replacement process is started by
    /// launchservices against the new bundle, not inherited from this one.
    private func relaunchSelf() {
        isRelaunchingIntoNewBundle = true
        let bundleURL = Bundle.main.bundleURL
        let configuration = NSWorkspace.OpenConfiguration()
        configuration.createsNewApplicationInstance = true
        NSWorkspace.shared.openApplication(at: bundleURL, configuration: configuration) { _, error in
            if let error {
                NSLog("Failed to relaunch after bundle replacement: \(error)")
                // Let a later poll try again rather than sitting on a stale
                // bundle forever.
                Task { @MainActor in
                    self.isRelaunchingIntoNewBundle = false
                }
                return
            }
            // The new instance's own `terminateOtherInstances()` would get
            // us anyway; exiting here just makes the handover immediate.
            Task { @MainActor in
                NSApp.terminate(nil)
            }
        }
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

    /// Local time plus a relative age — "when did this last work?" is the
    /// question every timestamp on the status sheet is really answering.
    func formatStatusTimestamp(_ timestampMs: Int64?) -> String {
        guard let timestampMs else { return "<none>" }
        let date = Date(timeIntervalSince1970: TimeInterval(timestampMs) / 1000)
        let formatter = DateFormatter()
        formatter.dateStyle = .short
        formatter.timeStyle = .medium
        let relative = RelativeDateTimeFormatter()
        relative.unitsStyle = .short
        return "\(formatter.string(from: date)) (\(relative.localizedString(for: date, relativeTo: Date())))"
    }

    /// Where this app's daemon writes its rolling log files — surfaced on the
    /// status sheet so a user can find them without knowing the convention.
    var logDirectory: URL {
        FileManager.default.homeDirectoryForCurrentUser
            .appendingPathComponent("Library")
            .appendingPathComponent("Logs")
    }

    private func formatMillisTimestamp(_ timestampMs: Int64) -> String {
        let formatter = DateFormatter()
        formatter.dateStyle = .none
        formatter.timeStyle = .medium
        return formatter.string(from: Date(timeIntervalSince1970: TimeInterval(timestampMs) / 1000))
    }
}
