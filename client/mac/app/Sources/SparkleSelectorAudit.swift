import Foundation
import Sparkle

/// Compile-time guard that Sparkle's delegate/driver hooks are actually wired
/// up.
///
/// Every `SPUUpdaterDelegate` method is `@optional`, so a method whose Swift
/// name doesn't match the protocol requirement compiles perfectly happily and
/// simply never gets called — the updater keeps working, but silently drops
/// whatever that hook controlled. `allowedChannels(for:)` is the dangerous
/// one: if it stops binding, dev-channel builds quietly stop seeing
/// dev-channel updates forever, with no error anywhere.
///
/// Swift only infers `@objc` for a member that satisfies an `@objc` protocol
/// requirement, so `#selector` on each of these fails to compile the moment a
/// name stops matching — which is exactly the signal we want when bumping
/// Sparkle. Verified to have teeth: renaming any method here fails the build
/// with "not exposed to Objective-C".
enum SparkleSelectorAudit {
    @MainActor static let selectors: [Selector] = [
        #selector(UpdateController.allowedChannels(for:)),
        #selector(UpdateController.updater(_:didFindValidUpdate:)),
        #selector(UpdateController.updaterWillRelaunchApplication(_:)),
        #selector(UpdateController.updater(_:didFinishUpdateCycleFor:error:)),
        #selector(VirtueUpdateDriver.show(_:reply:)),
        #selector(VirtueUpdateDriver.showUpdateFound(with:state:reply:)),
        #selector(VirtueUpdateDriver.showReady(toInstallAndRelaunch:)),
        #selector(VirtueUpdateDriver.showInstallingUpdate(withApplicationTerminated:retryTerminatingApplication:)),
        #selector(VirtueUpdateDriver.dismissUpdateInstallation),
    ]
}
