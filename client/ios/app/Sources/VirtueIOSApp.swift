import SwiftUI
import UIKit

@main
struct VirtueIOSApp: App {
    @StateObject private var coordinator = MonitoringCoordinator()

    var body: some Scene {
        WindowGroup {
            ContentView(coordinator: coordinator)
                .tint(VirtueBrand.accent)
                .preferredColorScheme(.light)
        }
    }
}

struct ContentView: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @State private var showPauseConfirmation = false
    @State private var showLogoutConfirmation = false
    @State private var isPasswordVisible = false
    @State private var showStatusSheet = false
    @State private var showReportBugSheet = false
    @State private var showReportBugConfirmation = false

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 18) {
                    headerCard
                    statusCard
                    accountCard
                    safariCard
                }
                .padding(20)
            }
            .background(VirtueBrand.bg)
            .navigationBarHidden(true)
        }
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
        .alert("Pause monitoring?", isPresented: $showPauseConfirmation) {
            Button("Cancel", role: .cancel) {}
            Button("Pause Monitoring", role: .destructive) {
                coordinator.toggleMonitoring()
            }
        } message: {
            Text("This will stop monitoring on this device. People monitoring you may be alerted.")
        }
        .alert("Sign out?", isPresented: $showLogoutConfirmation) {
            Button("Cancel", role: .cancel) {}
            Button("Sign Out", role: .destructive) {
                coordinator.logout()
            }
        } message: {
            Text("Signing out will delete this device and stop monitoring. Anyone monitoring you may be alerted. Logging in again will create a new device.")
        }
    }

    private var headerCard: some View {
        Card {
            HStack(alignment: .center, spacing: 16) {
                AppBrandIcon()

                VStack(alignment: .leading, spacing: 4) {
                    Text("Virtue Initiative")
                        .font(.system(size: 30, weight: .semibold))
                        .foregroundStyle(VirtueBrand.text)
                    Link("virtueinitiative.org", destination: URL(string: "https://virtueinitiative.org")!)
                        .font(.subheadline)
                        .foregroundStyle(VirtueBrand.accent)
                    Text("Build \(VirtueShared.buildLabel)")
                        .font(.footnote)
                        .foregroundStyle(VirtueBrand.textMuted)
                    Button("Report a Bug") {
                        showReportBugSheet = true
                    }
                    .font(.footnote)
                    .foregroundStyle(VirtueBrand.accent)
                    .padding(.top, 2)
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
                    .font(.system(size: 24, weight: .semibold))
                    .foregroundStyle(VirtueBrand.text)
                Text(statusSubtitle)
                    .font(.body)
                    .foregroundStyle(VirtueBrand.textMuted)

                HStack(spacing: 10) {
                    Button("Status Details") {
                        showStatusSheet = true
                    }
                    .buttonStyle(VirtueButtonStyle())

                    Button(coordinator.monitoringEnabled ? "Pause Monitoring" : "Resume Monitoring") {
                        if coordinator.monitoringEnabled {
                            showPauseConfirmation = true
                        } else {
                            coordinator.toggleMonitoring()
                        }
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
                    Text("Device: \(coordinator.deviceName)")
                        .foregroundStyle(VirtueBrand.textMuted)

                    HStack(spacing: 10) {
                        Button(coordinator.isSigningOut ? "Signing Out…" : "Sign Out") {
                            showLogoutConfirmation = true
                        }
                        .buttonStyle(VirtueButtonStyle())
                        .disabled(coordinator.isSigningOut)
                    }
                    .padding(.top, 6)
                } else {
                    Text("Sign in to start monitoring on this device.")
                        .foregroundStyle(VirtueBrand.textMuted)

                    VStack(alignment: .leading, spacing: 12) {
                        TextField("Email", text: $coordinator.email)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .textFieldStyle(.roundedBorder)

                        // SwiftUI has no reveal affordance on SecureField, so swap
                        // in a plain TextField while the eye toggle is on. Both
                        // bind the same password, so toggling never loses input.
                        HStack(spacing: 8) {
                            if isPasswordVisible {
                                TextField("Password", text: $coordinator.password)
                                    .textInputAutocapitalization(.never)
                                    .autocorrectionDisabled()
                                    .textFieldStyle(.roundedBorder)
                            } else {
                                SecureField("Password", text: $coordinator.password)
                                    .textFieldStyle(.roundedBorder)
                            }
                            Button {
                                isPasswordVisible.toggle()
                            } label: {
                                Image(systemName: isPasswordVisible ? "eye.slash" : "eye")
                                    .foregroundStyle(VirtueBrand.textMuted)
                            }
                            .buttonStyle(.plain)
                            .accessibilityLabel(isPasswordVisible ? "Hide password" : "Show password")
                        }

                        TextField("Device name", text: $coordinator.deviceName)
                            .autocorrectionDisabled()
                            .textFieldStyle(.roundedBorder)

                        HStack(spacing: 10) {
                            Button(coordinator.isSigningIn ? "Signing In…" : "Sign In") {
                                coordinator.login()
                            }
                            .buttonStyle(VirtueButtonStyle(prominent: true))
                            .disabled(coordinator.isSigningIn)
                        }

                        Link(
                            "Don't have an account? Sign up",
                            destination: URL(string: "https://app.virtueinitiative.org/signup")!
                        )
                        .font(.subheadline)
                        .foregroundStyle(VirtueBrand.accent)

                        if let error = coordinator.loginError {
                            Text(error)
                                .font(.subheadline)
                                .foregroundStyle(Color.red)
                        }
                    }
                    .padding(.top, 6)
                }
            }
        }
    }

    private var safariCard: some View {
        Card {
            VStack(alignment: .leading, spacing: 10) {
                SectionLabel("Safari")
                Text("Safari extension capture")
                    .font(.title3.weight(.semibold))
                    .foregroundStyle(VirtueBrand.text)
                VStack(alignment: .leading, spacing: 6) {
                    Text("Permission state: \(coordinator.safariPermissionSummary)")
                    Text("Daemon state: \(coordinator.safariDaemonStatus)")
                }
                .font(.subheadline)
                .foregroundStyle(VirtueBrand.textMuted)

                VStack(alignment: .leading, spacing: 6) {
                    Text("1. Open Settings > Safari > Extensions.")
                    Text("2. Enable Virtue Safari Capture.")
                    Text("3. Allow access on All Websites.")
                    Text("4. Virtue will produce screenshots while browsing.")
                }
                .font(.subheadline)
                .foregroundStyle(VirtueBrand.text)
            }
        }
    }

    private var primaryStatusTitle: String {
        if !coordinator.loggedIn {
            return "Signed out"
        }
        if coordinator.monitorSummary == "paused" {
            return "Monitoring paused"
        }
        if coordinator.monitorSummary == "active" {
            return "Monitoring active"
        }
        if coordinator.monitorSummary == "waiting for Safari" {
            return "Waiting for Safari"
        }
        return "Monitoring idle"
    }

    private var statusSubtitle: String {
        if !coordinator.loggedIn {
            return "Sign in to register this device and start monitoring."
        }
        if coordinator.monitorSummary == "paused" {
            return "Monitoring is stopped on this device until you resume it."
        }
        if coordinator.monitorSummary == "active" {
            return "The Safari extension is sending fresh capture activity."
        }
        if coordinator.monitorSummary == "waiting for Safari" {
            return "Monitoring is enabled, but Safari needs to be active on a capturable page."
        }
        return "Monitoring is enabled, but the service is currently idle."
    }
}

/// The status page (see `client/core/SPEC.md` CORE-010): the same sections, in
/// the same order, as every other platform's status screen, followed by the
/// iOS-only Safari extension section.
private struct StatusSheet: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section("Account") {
                    DetailRow(label: "Summary", value: coordinator.monitorSummary)
                    DetailRow(label: "Status", value: coordinator.statusMessage)
                    DetailRow(label: "Email", value: status?.accountEmail ?? coordinator.accountEmail ?? "<none>")
                    DetailRow(label: "Device name", value: status?.deviceName ?? "<none>")
                    DetailRow(label: "Partners", value: status?.partnerCount.map(String.init) ?? "<unknown>")
                }

                Section("Queues") {
                    DetailRow(label: "Waiting for hash", value: "\(status?.pendingHashCount ?? 0)")
                    DetailRow(label: "Waiting in batch", value: "\(status?.pendingBatchCount ?? 0)")
                    DetailRow(label: "Pending requests", value: "\(coordinator.pendingRequestCount)")
                    DetailRow(label: "Last batch upload", value: coordinator.lastCoreBatch)
                }

                Section("Capture") {
                    DetailRow(label: "Last loop", value: coordinator.lastCoreLoop)
                    DetailRow(label: "Last attempt", value: coordinator.lastCoreScreenshotAttempt)
                    DetailRow(label: "Last screenshot", value: coordinator.lastCoreScreenshot)
                    DetailRow(label: "Last skip reason", value: status?.lastSkipReason?.label ?? "<none>")
                }

                Section("Recent errors") {
                    if let errors = status?.recentErrors, !errors.isEmpty {
                        ForEach(Array(errors.prefix(5).enumerated()), id: \.offset) { _, error in
                            DetailRow(
                                label: "\(coordinator.formatStatusTimestamp(error.atMs)) · \(error.context)",
                                value: error.message
                            )
                        }
                    } else {
                        DetailRow(label: "Errors", value: "None")
                    }
                }

                Section("Advanced") {
                    DetailRow(label: "Device ID", value: coordinator.deviceId)
                    DetailRow(label: "API URL", value: status?.apiBaseUrl ?? coordinator.currentApiBaseUrl)
                    DetailRow(label: "Hash base URL", value: status?.hashBaseUrl ?? "<default>")
                    DetailRow(
                        label: "Capture interval",
                        value: status?.captureIntervalSeconds.map { "\($0)s" }
                            ?? "\(VirtueShared.defaultCaptureIntervalSeconds)s"
                    )
                    DetailRow(
                        label: "Batch window",
                        value: status?.batchWindowSeconds.map { "\($0)s" }
                            ?? "\(VirtueShared.defaultBatchWindowSeconds)s"
                    )
                    DetailRow(label: "Build", value: VirtueShared.buildLabel)
                }

                Section("Safari Extension") {
                    DetailRow(label: "Capture health", value: coordinator.safariCaptureHealth)
                    DetailRow(label: "Permission state", value: coordinator.safariPermissionSummary)
                    DetailRow(label: "Daemon", value: coordinator.safariDaemonStatus)
                    DetailRow(label: "Last heartbeat", value: coordinator.safariLastHeartbeat)
                    DetailRow(label: "Last frame", value: coordinator.safariLastFrame)
                    DetailRow(label: "Last page", value: coordinator.safariLastPage)
                    DetailRow(label: "Last error", value: coordinator.safariLastError)
                }
            }
            .scrollContentBackground(.hidden)
            .background(VirtueBrand.bg)
            .navigationTitle("Status Details")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        dismiss()
                    }
                }
            }
        }
    }

    private var status: CoreServiceStatus? { coordinator.coreStatus }
}

private struct VirtueButtonStyle: ButtonStyle {
    var prominent: Bool = false

    func makeBody(configuration: Configuration) -> some View {
        configuration.label
            .padding(.horizontal, 14)
            .padding(.vertical, 7)
            .background(
                prominent
                    ? (configuration.isPressed ? VirtueBrand.accent.opacity(0.85) : VirtueBrand.accent)
                    : (configuration.isPressed ? VirtueBrand.border : VirtueBrand.bgSubtle)
            )
            .foregroundStyle(prominent ? Color.white : VirtueBrand.accent)
            .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .stroke(prominent ? Color.clear : VirtueBrand.border, lineWidth: 1)
            )
            .animation(.easeInOut(duration: 0.1), value: configuration.isPressed)
    }
}

private struct Card<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            content
        }
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(VirtueBrand.surface)
        .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .stroke(VirtueBrand.border, lineWidth: 1)
        )
    }
}

private struct SectionLabel: View {
    let text: String

    init(_ text: String) {
        self.text = text
    }

    var body: some View {
        Text(text.uppercased())
            .font(.caption.weight(.medium))
            .foregroundStyle(VirtueBrand.ochre)
    }
}

private struct DetailRow: View {
    let label: String
    let value: String

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label)
                .font(.caption)
                .foregroundStyle(VirtueBrand.textMuted)
            Text(value)
                .font(.body)
                .foregroundStyle(VirtueBrand.text)
        }
        .padding(.vertical, 2)
        .listRowBackground(VirtueBrand.surface)
    }
}

private struct AppBrandIcon: View {
    var body: some View {
        if let image = resolvePrimaryAppIcon() {
            Image(uiImage: image)
                .resizable()
                .frame(width: 60, height: 60)
                .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        } else {
            RoundedRectangle(cornerRadius: 8, style: .continuous)
                .fill(VirtueBrand.accent)
                .frame(width: 60, height: 60)
                .overlay(
                    Text("VI")
                        .font(.headline.weight(.bold))
                        .foregroundStyle(.white)
                )
        }
    }

    private func resolvePrimaryAppIcon() -> UIImage? {
        let iconDictionaryKeys = ["CFBundleIcons", "CFBundleIcons~ipad"]
        for dictionaryKey in iconDictionaryKeys {
            guard
                let icons = Bundle.main.object(forInfoDictionaryKey: dictionaryKey) as? [String: Any],
                let primaryIcon = icons["CFBundlePrimaryIcon"] as? [String: Any],
                let iconFiles = primaryIcon["CFBundleIconFiles"] as? [String]
            else {
                continue
            }

            for iconName in iconFiles.reversed() {
                if let image = UIImage(named: iconName) {
                    return image
                }
            }
        }
        return nil
    }
}

enum VirtueBrand {
    // Forest green — matches --accent / --forest in shared-web/tokens.css
    static let accent = Color(
        red: 30.0 / 255.0,
        green: 58.0 / 255.0,
        blue: 46.0 / 255.0
    )
    // Warm ochre — matches --ochre in shared-web/tokens.css
    static let ochre = Color(
        red: 166.0 / 255.0,
        green: 127.0 / 255.0,
        blue: 61.0 / 255.0
    )
    // Page background — matches --bg (#f4efe3) in shared-web/tokens.css
    static let bg = Color(red: 244.0 / 255.0, green: 239.0 / 255.0, blue: 227.0 / 255.0)
    // Card surface — matches --surface (#fbf7ea) in shared-web/tokens.css
    static let surface = Color(red: 251.0 / 255.0, green: 247.0 / 255.0, blue: 234.0 / 255.0)
    // Subtle background — matches --bg-subtle (#ebe4ce) in shared-web/tokens.css
    static let bgSubtle = Color(red: 235.0 / 255.0, green: 228.0 / 255.0, blue: 206.0 / 255.0)
    // Border — matches --border (#d9d1bc) in shared-web/tokens.css
    static let border = Color(red: 217.0 / 255.0, green: 209.0 / 255.0, blue: 188.0 / 255.0)
    // Primary text — matches --text (#1b1a16) in shared-web/tokens.css
    static let text = Color(red: 27.0 / 255.0, green: 26.0 / 255.0, blue: 22.0 / 255.0)
    // Muted text — matches --text-muted (#6a6655) in shared-web/tokens.css
    static let textMuted = Color(red: 106.0 / 255.0, green: 102.0 / 255.0, blue: 85.0 / 255.0)
}
