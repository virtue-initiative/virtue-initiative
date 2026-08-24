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

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 18) {
                headerCard
                statusCard
                accountCard
                if let permissionPhase = coordinator.permissionPhase {
                    permissionCard(permissionPhase)
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
            Text("Logging out will alert people monitoring you and will recreate a new device on your next login. Continue?")
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
                        .foregroundStyle(VirtueBrand.accent)
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

                HStack(spacing: 10) {
                    Button("Status Details") {
                        showStatusSheet = true
                    }
                    .buttonStyle(VirtueButtonStyle())

                    Button(coordinator.isForceCapturing ? "Capturing…" : "Force Screenshot & Upload") {
                        coordinator.forceCapture()
                    }
                    .buttonStyle(VirtueButtonStyle())
                    .disabled(!coordinator.loggedIn || coordinator.isForceCapturing)

                    Button("Report a Bug") {
                        showReportBugSheet = true
                    }
                    .buttonStyle(VirtueButtonStyle())

                    Button("Stop Monitoring and Quit") {
                        showStopConfirmation = true
                    }
                    .buttonStyle(VirtueButtonStyle(prominent: true))
                    .disabled(!coordinator.loggedIn)
                }
                .padding(.top, 6)
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

                    HStack(spacing: 10) {
                        Button(coordinator.isSigningOut ? "Signing Out…" : "Logout") {
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
                        isSigningIn: coordinator.isSigningIn,
                        loginError: coordinator.loginError,
                        onSubmit: coordinator.login
                    )
                    .padding(.top, 6)
                }
            }
        }
    }

    private func permissionCard(_ phase: PermissionPhase) -> some View {
        Card {
            VStack(alignment: .leading, spacing: 10) {
                SectionLabel("Screen Recording")
                switch phase {
                case .needsRequest:
                    Text("Virtue needs Screen Recording permission to capture screenshots.")
                        .foregroundStyle(VirtueBrand.textMuted)
                    Button("Request Permissions") {
                        coordinator.requestPermissions()
                    }
                    .buttonStyle(VirtueButtonStyle(prominent: true))
                    .padding(.top, 6)
                case .needsRelaunch:
                    Text("Permission was granted in System Settings, but Virtue must relaunch its background service to use it.")
                        .foregroundStyle(VirtueBrand.textMuted)
                    Button(coordinator.isRelaunching ? "Restarting…" : "Relaunch to Accept Permissions") {
                        coordinator.relaunchToAcceptPermissions()
                    }
                    .buttonStyle(VirtueButtonStyle(prominent: true))
                    .disabled(coordinator.isRelaunching)
                    .padding(.top, 6)
                    if let relaunchError = coordinator.relaunchError {
                        Text("Relaunch failed: \(relaunchError)")
                            .font(.subheadline)
                            .foregroundStyle(VirtueBrand.danger)
                    }
                }
            }
        }
    }

    private var primaryStatusTitle: String {
        if !coordinator.loggedIn {
            return "Signed out"
        }
        if coordinator.unexpectedStopMessage != nil {
            return "Monitoring stopped"
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
        if coordinator.daemonStatus == .running {
            return "The background service is capturing activity on this device."
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
