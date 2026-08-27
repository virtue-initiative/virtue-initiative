import AppKit
import SwiftUI
import VirtueKit

@main
struct VirtueMacApp: App {
    @StateObject private var coordinator = MonitoringCoordinator()
    @StateObject private var updateController = UpdateController()
    @Environment(\.openWindow) private var openWindow

    var body: some Scene {
        // `LSUIElement` (menu-bar-only) apps don't get their `Window` scene
        // auto-shown at launch the way a normal app's WindowGroup would —
        // macOS suppresses automatic window display for accessory-policy
        // apps. Port `run_tray`'s explicit `open_app_dialog` call at
        // startup: `.onAppear` on the label view fires once, at true launch
        // time, since the label (unlike the lazily-built menu content) is
        // rendered immediately.
        MenuBarExtra {
            MenuBarMenuContent(coordinator: coordinator, updateController: updateController)
        } label: {
            Image("TrayIcon")
                .onAppear {
                    // Lets the coordinator avoid relaunching itself while
                    // Sparkle is mid-install. See `checkForReplacedBundle`.
                    coordinator.updateController = updateController
                    openWindow(id: "main")
                    NSApp.activate(ignoringOtherApps: true)
                }
        }

        Window("Virtue", id: "main") {
            ContentView(coordinator: coordinator)
                .tint(VirtueBrand.accent)
                .preferredColorScheme(.light)
        }
        .windowResizability(.contentMinSize)
    }
}

/// Ports `build_tray_menu` from `main.rs`: Open Virtue / Log In / Stop
/// Monitoring and Quit·Quit. Logout is intentionally not offered here — the
/// user opens the app window for that.
private struct MenuBarMenuContent: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @ObservedObject var updateController: UpdateController
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Button("Open Virtue") {
            openWindow(id: "main")
            NSApp.activate(ignoringOtherApps: true)
        }

        if !coordinator.loggedIn {
            Divider()
            Button("Log In") {
                openWindow(id: "main")
                NSApp.activate(ignoringOtherApps: true)
            }
        } else {
            Divider()
            Button("Force Screenshot & Upload") {
                coordinator.forceCapture()
            }
            .disabled(coordinator.isForceCapturing)
        }

        if updateController.isEnabled {
            Divider()
            Button("Check for Updates") {
                updateController.checkForUpdates()
            }
            .disabled(!updateController.canCheckForUpdates)
        }

        Divider()

        Button(coordinator.loggedIn ? "Stop Monitoring and Quit" : "Quit") {
            if coordinator.loggedIn {
                guard confirmStopMonitoring() else { return }
            }
            coordinator.stopMonitoringAndQuit()
        }
    }
}

/// The menu-bar dropdown closes the instant an item is chosen, so there's no
/// SwiftUI view left to attach `.alert()` to — use a native modal instead,
/// same message as the in-window confirmation shows.
private func confirmStopMonitoring() -> Bool {
    let alert = NSAlert()
    alert.messageText = "Stop monitoring and quit?"
    alert.informativeText =
        "This will stop monitoring on this device and quit Virtue. People monitoring you may be alerted."
    alert.alertStyle = .warning
    alert.addButton(withTitle: "Stop Monitoring and Quit")
    alert.addButton(withTitle: "Cancel")
    NSApp.activate(ignoringOtherApps: true)
    return alert.runModal() == .alertFirstButtonReturn
}
