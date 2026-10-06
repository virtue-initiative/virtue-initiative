import AppKit
import SwiftUI
import VirtueKit

struct ContentView: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @State private var showStopConfirmation = false
    @State private var showLogoutConfirmation = false
    @State private var showStatusSheet = false
    @State private var showReportBugSheet = false
    @State private var showReportBugConfirmation = false
    @Environment(\.openURL) private var openURL

    /// Partners are managed in the web app, not from this client.
    private let partnersURL = URL(string: "https://app.virtueinitiative.org/partners")!

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                headerCard
                // First, above the fold: nothing else works until it's done.
                if coordinator.needsScreenRecording {
                    permissionCard
                }
                // Signed out, the sign-in form is the next step, so it comes
                // before the status and help cards rather than below the fold.
                if coordinator.loggedIn {
                    statusCard
                    monitoringCard
                    accountCard
                } else {
                    accountCard
                    monitoringCard
                    statusCard
                }
            }
            .padding(20)
        }
        .frame(minWidth: 420, idealWidth: 420, maxWidth: .infinity)
        .background(VirtueBrand.bg)
        .sheet(isPresented: $showStatusSheet) {
            StatusSheet(coordinator: coordinator)
        }
        .sheet(isPresented: $showReportBugSheet) {
            ReportBugSheet(coordinator: coordinator) {
                showReportBugConfirmation = true
            }
        }
        .alert("Report Sent", isPresented: $showReportBugConfirmation) {
            Button("OK") {}
        } message: {
            Text("Thanks — your report was sent to the Virtue Initiative team.")
        }
        .alert("Stop monitoring and quit?", isPresented: $showStopConfirmation) {
            Button("Cancel", role: .cancel) {}
            Button("Stop Monitoring and Quit", role: .destructive) {
                coordinator.stopMonitoringAndQuit()
            }
        } message: {
            Text("This will stop monitoring on this device and quit Virtue. People monitoring you may be alerted.")
        }
        .alert("Sign out?", isPresented: $showLogoutConfirmation) {
            Button("Cancel", role: .cancel) {}
            Button("Sign Out", role: .destructive) {
                coordinator.logout()
            }
        } message: {
            Text("Signing out will deactivate this device and stop monitoring. Anyone monitoring you may be alerted. Logging in again will create a new device.")
        }
    }

    private var headerCard: some View {
        Card {
            HStack(alignment: .center, spacing: 16) {
                AppBrandIcon()

                VStack(alignment: .leading, spacing: 4) {
                    Text("Virtue Initiative")
                        .font(.system(size: 26, weight: .semibold))
                        .foregroundStyle(VirtueBrand.text)
                    Link("virtueinitiative.org", destination: URL(string: "https://virtueinitiative.org")!)
                        .font(.subheadline)
                        .foregroundStyle(VirtueBrand.link)
                        .onHover { hovering in
                            if hovering {
                                NSCursor.pointingHand.push()
                            } else {
                                NSCursor.pop()
                            }
                        }
                    Text("Build \(coordinator.buildLabel)")
                        .font(.footnote)
                        .foregroundStyle(VirtueBrand.textMuted)
                }

                Spacer(minLength: 0)
            }
        }
    }

    private var statusCard: some View {
        Card {
            VStack(alignment: .leading, spacing: 10) {
                SectionLabel("Status")
                Text(primaryStatusTitle)
                    .font(.system(size: 22, weight: .semibold))
                    .foregroundStyle(VirtueBrand.text)
                Text(statusSubtitle)
                    .font(.body)
                    .foregroundStyle(VirtueBrand.textMuted)

                if let unexpectedStopMessage = coordinator.unexpectedStopMessage {
                    Text(unexpectedStopMessage)
                        .font(.subheadline)
                        .foregroundStyle(VirtueBrand.danger)
                }

                if let forceCaptureMessage = coordinator.forceCaptureMessage {
                    Text(forceCaptureMessage)
                        .font(.subheadline)
                        .foregroundStyle(VirtueBrand.textMuted)
                }

                VStack(alignment: .leading, spacing: 10) {
                    HStack(spacing: 10) {
                        Button("Status Details") {
                            showStatusSheet = true
                        }
                        .buttonStyle(VirtueButtonStyle())

                        Button(coordinator.isForceCapturing ? "Uploading…" : "Test Screenshot") {
                            coordinator.forceCapture()
                        }
                        .buttonStyle(VirtueButtonStyle())
                        // Off until Screen Recording is on, so it can't pull
                        // attention from the Restart Virtue step (issue #632).
                        .disabled(!coordinator.loggedIn || coordinator.needsScreenRecording || coordinator.isForceCapturing)
                        .overlay {
                            // A disabled view stops receiving the hover events that
                            // drive `.help()`, so host the tooltip on a plain
                            // (non-disabled) overlay instead of the button itself.
                            if !coordinator.loggedIn {
                                Color.clear
                                    .contentShape(Rectangle())
                                    .help("Sign in to use this feature")
                            } else if coordinator.needsScreenRecording {
                                Color.clear
                                    .contentShape(Rectangle())
                                    .help("Allow Screen Recording to use this feature")
                            }
                        }
                    }

                    HStack(spacing: 10) {
                        Button("Report a Bug") {
                            showReportBugSheet = true
                        }
                        .buttonStyle(VirtueButtonStyle())

                        Button("Stop Monitoring & Quit") {
                            showStopConfirmation = true
                        }
                        .buttonStyle(VirtueButtonStyle(prominent: true))
                        .disabled(!coordinator.loggedIn)
                    }
                }
                .padding(.top, 6)
            }
        }
    }

    // Short summary; the full list is on the linked help page. Keep both in step
    // with what the daemon actually does (core SPEC CORE-003, CORE-004).
    private var monitoringCard: some View {
        Card {
            VStack(alignment: .leading, spacing: 10) {
                SectionLabel("What Virtue Monitors")
                Text("Virtue takes a screenshot of your main display about every 5 minutes. Screenshots are blurred, have text blacked out, and can only be seen by you and your partners.")
                    .font(.body)
                    .foregroundStyle(VirtueBrand.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
                Link("Learn more", destination: URL(string: "https://virtueinitiative.org/help/what-virtue-monitors/mac")!)
                    .font(.subheadline)
                    .foregroundStyle(VirtueBrand.link)
            }
        }
    }

    private var accountCard: some View {
        Card {
            VStack(alignment: .leading, spacing: 10) {
                SectionLabel("Account")

                if coordinator.loggedIn {
                    Text("Signed in")
                        .font(.title3.weight(.semibold))
                        .foregroundStyle(VirtueBrand.text)
                    Text("Device: \(coordinator.deviceId)")
                        .foregroundStyle(VirtueBrand.textMuted)

                    Text("Add partners on the Virtue website. Open the Partners page and select \"Invite partner\".")
                        .foregroundStyle(VirtueBrand.textMuted)
                        .padding(.top, 6)
                    Button("Open Partners Page") {
                        openURL(partnersURL)
                    }
                    .buttonStyle(VirtueButtonStyle())

                    HStack(spacing: 10) {
                        Button(coordinator.isSigningOut ? "Signing Out…" : "Sign Out") {
                            showLogoutConfirmation = true
                        }
                        .buttonStyle(VirtueButtonStyle())
                        .disabled(coordinator.isSigningOut)
                    }
                    .padding(.top, 6)
                } else {
                    Text("Log in to start monitoring on this device.")
                        .foregroundStyle(VirtueBrand.textMuted)
                        .padding(.top, 4)

                    LoginFormView(
                        email: $coordinator.email,
                        password: $coordinator.password,
                        deviceName: $coordinator.deviceName,
                        deviceNamePlaceholder: NativeBridge.defaultDeviceName(),
                        isSigningIn: coordinator.isSigningIn,
                        loginError: coordinator.loginError,
                        onSubmit: coordinator.login
                    )
                    .padding(.top, 6)
                }
            }
        }
    }

    private var permissionCard: some View {
        Card {
            VStack(alignment: .leading, spacing: 10) {
                SectionLabel("Action Needed")
                Text("Allow Screen Recording")
                    .font(.title3.weight(.semibold))
                    .foregroundStyle(VirtueBrand.text)
                Text("Virtue can't take screenshots until you allow Screen Recording.")
                    .foregroundStyle(VirtueBrand.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
                VStack(alignment: .leading, spacing: 6) {
                    permissionStep(1, "Click Open System Settings.")
                    permissionStep(2, "Turn on Virtue in the list.")
                    permissionStep(3, "When macOS asks, click Quit & Reopen.")
                }
                .padding(.top, 2)
                HStack(spacing: 10) {
                    Button("Open System Settings") {
                        coordinator.openScreenRecordingSettings()
                    }
                    .buttonStyle(VirtueButtonStyle(prominent: true))
                    // macOS only reports the change to a freshly started app,
                    // so this covers anyone who clicked Later in step 3.
                    Button("Restart Virtue") {
                        coordinator.restartApp()
                    }
                    .buttonStyle(VirtueButtonStyle())
                }
                .padding(.top, 6)
                Text("If you turned it on and clicked Later, click Restart Virtue.")
                    .font(.footnote)
                    .foregroundStyle(VirtueBrand.textMuted)
            }
        }
    }

    private func permissionStep(_ number: Int, _ text: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text("\(number).")
                .fontWeight(.semibold)
                .foregroundStyle(VirtueBrand.text)
                .frame(minWidth: 16, alignment: .leading)
            Text(text)
                .foregroundStyle(VirtueBrand.text)
                .fixedSize(horizontal: false, vertical: true)
        }
    }

    private var primaryStatusTitle: String {
        if !coordinator.loggedIn {
            return "Signed out"
        }
        if coordinator.unexpectedStopMessage != nil {
            return "Monitoring stopped"
        }
        if coordinator.needsScreenRecording {
            return "Screen Recording off"
        }
        if coordinator.daemonStatus == .running {
            return "Monitoring active"
        }
        return "Starting…"
    }

    private var statusSubtitle: String {
        if !coordinator.loggedIn {
            return "Log in to register this device and start monitoring."
        }
        if coordinator.unexpectedStopMessage != nil {
            return "Relaunch the Virtue app to continue monitoring."
        }
        if coordinator.needsScreenRecording {
            return "Allow Screen Recording above so Virtue can take screenshots."
        }
        if coordinator.daemonStatus == .running {
            return "Virtue is taking screenshots on this Mac."
        }
        return "Waiting for the background service to start."
    }
}

private struct AppBrandIcon: View {
    var body: some View {
        Image(nsImage: NSApp.applicationIconImage)
            .resizable()
            .frame(width: 60, height: 60)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
    }
}
