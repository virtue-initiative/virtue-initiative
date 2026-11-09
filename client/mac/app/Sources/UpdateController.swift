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
    /// The update check line (CLIENT-026): the latest check's state, for
    /// scheduled checks too. Nil until a check has run, and in a build
    /// without auto-update.
    @Published private(set) var statusMessage: String?
    /// Mirrors `SPUUpdater.canCheckForUpdates`, which is false while a check
    /// or install is running. Published so the buttons re-enable on their own.
    @Published private(set) var canCheckForUpdates: Bool = false

    /// Set by Check for Updates, so only a check the user asked for ends in
    /// an alert. Scheduled checks stay silent.
    private var userInitiatedCheck = false

    /// Written when Check for Updates finds an update, just before Sparkle
    /// quits to install it, so the relaunched app can say it was updated.
    /// Holds the update's `versionString` and `displayVersionString`.
    private static let pendingNoticeKey = "VirtuePendingUpdateNotice"

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

        guard !Self.feedURL.isEmpty else { return }

        Self.showPendingUpdateNotice()

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
            updater.publisher(for: \.canCheckForUpdates)
                .receive(on: DispatchQueue.main)
                .assign(to: &$canCheckForUpdates)
        } catch {
            // A failed updater must never take the app down with it —
            // monitoring matters more than updating. `isEnabled` stays false,
            // so Check for Updates is hidden.
            NSLog("Sparkle failed to start: \(error)")
        }
    }

    /// True while Sparkle is mid-check/download/install. `MonitoringCoordinator`
    /// uses this to stay out of the way of Sparkle's own relaunch.
    var isUpdateSessionInProgress: Bool {
        updater?.sessionInProgress ?? false
    }

    /// Menu-driven "Check for Updates". Runs the same silent path a scheduled
    /// check does — if an update is found it installs and relaunches without
    /// asking. Otherwise the result shows in an alert (CLIENT-026).
    func checkForUpdates() {
        guard let updater, updater.canCheckForUpdates else { return }
        statusMessage = "Checking for updates…"
        userInitiatedCheck = true
        updater.checkForUpdates()
    }

    /// Sets the check line and, when the user asked for this check, shows
    /// the result in an alert.
    private func finishCheck(status: String, alertTitle: String, alertText: String) {
        statusMessage = status
        guard userInitiatedCheck else { return }
        userInitiatedCheck = false
        // Let Sparkle finish ending its cycle before the alert's modal loop.
        DispatchQueue.main.async {
            Self.showAlert(title: alertTitle, text: alertText)
        }
    }

    /// After a relaunch that Check for Updates started, says whether the
    /// update installed, then forgets it so it shows once (CLIENT-026).
    private static func showPendingUpdateNotice() {
        let defaults = UserDefaults.standard
        guard let notice = defaults.dictionary(forKey: pendingNoticeKey) as? [String: String]
        else { return }
        defaults.removeObject(forKey: pendingNoticeKey)

        let display = notice["display"] ?? ""
        let current = Bundle.main.object(forInfoDictionaryKey: "CFBundleVersion") as? String
        let (title, text) =
            current == notice["version"]
            ? ("Virtue updated", "Virtue is now on the latest version, \(display).")
            : (
                "Update didn't install",
                "Virtue couldn't install version \(display). It will try again at the next update check."
            )
        // Give the window a moment to open first, so the alert lands on top.
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
            showAlert(title: title, text: text)
        }
    }

    /// A native alert, like the menu bar's Stop Monitoring confirmation: the
    /// menu bar's dropdown closes at once and leaves no view to show it in.
    private static func showAlert(title: String, text: String) {
        let alert = NSAlert()
        alert.messageText = title
        alert.informativeText = text
        alert.addButton(withTitle: "OK")
        NSApp.activate(ignoringOtherApps: true)
        alert.runModal()
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
        MainActor.assumeIsolated {
            statusMessage = "Update found; installing…"
            // No alert now, since one would hold up the quit Sparkle needs to
            // install. The relaunched app shows it instead.
            if userInitiatedCheck {
                UserDefaults.standard.set(
                    ["version": item.versionString, "display": item.displayVersionString],
                    forKey: Self.pendingNoticeKey
                )
            }
        }
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
                !(error.domain == SUSparkleErrorDomain
                    && error.code == Int(SUError.noUpdateError.rawValue))
            {
                NSLog("Sparkle update cycle failed: \(error)")
                // A failed download or install never relaunches, so its
                // notice must not wait for the next launch.
                UserDefaults.standard.removeObject(forKey: Self.pendingNoticeKey)
                finishCheck(
                    status: "Update check failed: \(error.localizedDescription)",
                    alertTitle: "Update check failed",
                    alertText: error.localizedDescription
                )
            } else if statusMessage == "Update found; installing…" {
                // The update is installing, and the relaunch reports it.
                userInitiatedCheck = false
            } else {
                // Sparkle reports "no update" as noUpdateError.
                finishCheck(
                    status: "No updates found.",
                    alertTitle: "No updates found",
                    alertText: "This is the latest version of Virtue."
                )
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
