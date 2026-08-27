import AppKit
import Combine
import Foundation
import Sparkle

/// Silent auto-update via Sparkle (issues #233 / #539).
///
/// Two deliberate departures from a stock Sparkle integration:
///
/// 1. **No user interaction, ever.** `SPUStandardUserDriver` asks the user to
///    approve each update and, by default, defers installation until the app
///    quits. Neither works here: this is an `LSUIElement` menu bar app that
///    is registered as a login item and effectively never quits, so
///    "install on quit" would mean "never install"; and a dismissible
///    "update later" prompt on a monitoring client is a monitoring bypass —
///    a user could sit on a version with a known capture bug indefinitely.
///    `VirtueUpdateDriver` below therefore answers every Sparkle question
///    with "yes, install now" and renders no UI.
///
/// 2. **The daemon is intentionally left running across the install.**
///    `virtue-daemon` is a separate launchd process executing a binary inside
///    the bundle Sparkle is about to replace. It would be tempting to stop it
///    first, but that fails unsafe: if the post-install relaunch never
///    happens, monitoring would be silently off. Leaving it alone is both
///    safe and correct — the running process keeps its old inode after
///    Sparkle moves the old bundle aside, and the relaunched app's
///    `ensureDaemonRunning` (`launchctl kickstart -k`) swaps it for the new
///    binary within seconds of relaunch. That gap matters: `lifecycle::tick`
///    (CORE-002) alerts on a single late wakeup over 2 minutes, so the daemon
///    must never be down for anywhere near that long.
///
/// Auto-update is off unless a feed URL was baked in at build time — see
/// `SUFeedURL` in `Info.plist` and `VIRTUE_ENABLE_AUTO_UPDATE` in
/// `scripts/build-app.sh`. A locally built app never updates itself.
@MainActor
final class UpdateController: NSObject, ObservableObject {
    /// Nil when auto-update is disabled for this build, which is what every
    /// local and PR build is.
    private var updater: SPUUpdater?
    private let driver = VirtueUpdateDriver()

    @Published private(set) var isEnabled: Bool = false
    @Published private(set) var statusMessage: String?

    /// Empty in a build without auto-update wired in.
    static var feedURL: String {
        let raw = Bundle.main.object(forInfoDictionaryKey: "SUFeedURL") as? String ?? ""
        return raw.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    static var releaseChannel: String {
        let raw = Bundle.main.object(forInfoDictionaryKey: "VirtueReleaseChannel") as? String ?? ""
        return raw.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    override init() {
        super.init()

        guard !Self.feedURL.isEmpty else {
            statusMessage = "Automatic updates are not enabled for this build."
            return
        }

        let updater = SPUUpdater(
            hostBundle: Bundle.main,
            applicationBundle: Bundle.main,
            userDriver: driver,
            delegate: self
        )
        // Info.plist carries these as first-launch defaults, but they're
        // user defaults once written, so a build that ever ran with them off
        // would stay off. Assert them on every launch instead.
        updater.automaticallyChecksForUpdates = true
        updater.automaticallyDownloadsUpdates = true
        updater.updateCheckInterval = 21600

        do {
            try updater.start()
            self.updater = updater
            isEnabled = true
        } catch {
            // A failed updater must never take the app down with it —
            // monitoring matters more than updating.
            statusMessage = "Automatic updates unavailable: \(error.localizedDescription)"
            NSLog("Sparkle failed to start: \(error)")
        }
    }

    var canCheckForUpdates: Bool {
        updater?.canCheckForUpdates ?? false
    }

    /// True while Sparkle is mid-check/download/install. `MonitoringCoordinator`
    /// uses this to stay out of the way of Sparkle's own relaunch.
    var isUpdateSessionInProgress: Bool {
        updater?.sessionInProgress ?? false
    }

    /// Menu-driven "Check for Updates". Runs the same silent path a scheduled
    /// check does — if an update is found it installs and relaunches without
    /// asking.
    func checkForUpdates() {
        guard let updater else { return }
        statusMessage = "Checking for updates…"
        updater.checkForUpdates()
    }
}

extension UpdateController: SPUUpdaterDelegate {
    /// Dev-channel builds accept dev-channel appcast items; stable builds see
    /// only untagged (stable) items. One feed serves both — see
    /// `landing/scripts/build-appcast.mjs`.
    nonisolated func allowedChannels(for updater: SPUUpdater) -> Set<String> {
        MainActor.assumeIsolated {
            Self.releaseChannel == "dev" ? ["dev"] : []
        }
    }

    nonisolated func updater(_ updater: SPUUpdater, didFindValidUpdate item: SUAppcastItem) {
        NSLog("Sparkle found update \(item.displayVersionString) (\(item.versionString))")
    }

    nonisolated func updaterWillRelaunchApplication(_ updater: SPUUpdater) {
        // The daemon is deliberately left running here; see the class doc.
        NSLog("Sparkle installed an update; relaunching")
    }

    nonisolated func updater(
        _ updater: SPUUpdater,
        didFinishUpdateCycleFor updateCheck: SPUUpdateCheck,
        error: (any Error)?
    ) {
        MainActor.assumeIsolated {
            if let error = error as NSError?,
               error.code != Int(SUError.noUpdateError.rawValue) {
                statusMessage = "Update check failed: \(error.localizedDescription)"
                NSLog("Sparkle update cycle failed: \(error)")
            } else {
                statusMessage = "Up to date."
            }
        }
    }
}

/// A headless `SPUUserDriver`: answers every prompt with "install now" and
/// draws nothing. See `UpdateController`'s doc comment for why the standard
/// driver is unusable here.
@MainActor
final class VirtueUpdateDriver: NSObject, SPUUserDriver {
    func show(
        _ request: SPUUpdatePermissionRequest,
        reply: @escaping (SUUpdatePermissionResponse) -> Void
    ) {
        // Only reached if SUEnableAutomaticChecks is ever removed from
        // Info.plist. Opt in to checks, opt out of system profile reporting.
        reply(SUUpdatePermissionResponse(automaticUpdateChecks: true, sendSystemProfile: false))
    }

    func showUserInitiatedUpdateCheck(cancellation: @escaping () -> Void) {}

    func showUpdateFound(
        with appcastItem: SUAppcastItem,
        state: SPUUserUpdateState,
        reply: @escaping (SPUUserUpdateChoice) -> Void
    ) {
        reply(.install)
    }

    func showUpdateReleaseNotes(with downloadData: SPUDownloadData) {}

    func showUpdateReleaseNotesFailedToDownloadWithError(_ error: any Error) {}

    func showUpdateNotFoundWithError(_ error: any Error, acknowledgement: @escaping () -> Void) {
        acknowledgement()
    }

    func showUpdaterError(_ error: any Error, acknowledgement: @escaping () -> Void) {
        NSLog("Sparkle updater error: \(error)")
        acknowledgement()
    }

    func showDownloadInitiated(cancellation: @escaping () -> Void) {}

    func showDownloadDidReceiveExpectedContentLength(_ expectedContentLength: UInt64) {}

    func showDownloadDidReceiveData(ofLength length: UInt64) {}

    func showDownloadDidStartExtractingUpdate() {}

    func showExtractionReceivedProgress(_ progress: Double) {}

    func showReady(toInstallAndRelaunch reply: @escaping (SPUUserUpdateChoice) -> Void) {
        reply(.install)
    }

    func showInstallingUpdate(
        withApplicationTerminated applicationTerminated: Bool,
        retryTerminatingApplication: @escaping () -> Void
    ) {
        // Sparkle needs the app to exit before it can swap the bundle. The
        // app has no documents and no quit confirmation, so termination is
        // always safe to retry immediately.
        if !applicationTerminated {
            retryTerminatingApplication()
        }
    }

    func showUpdateInstalledAndRelaunched(
        _ relaunched: Bool,
        acknowledgement: @escaping () -> Void
    ) {
        acknowledgement()
    }

    func dismissUpdateInstallation() {}
}
