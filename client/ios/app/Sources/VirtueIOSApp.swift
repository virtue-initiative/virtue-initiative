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
    @State private var showStatusSheet = false
    @State private var showOverridesSheet = false

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
        .sheet(isPresented: $showOverridesSheet) {
            OverridesSheet(coordinator: coordinator)
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
            Text("Logging out will alert people monitoring you and will recreate a new device on your next login. Continue?")
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

                        Button("Runtime Overrides") {
                            showOverridesSheet = true
                        }
                        .buttonStyle(VirtueButtonStyle())
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

                        SecureField("Password", text: $coordinator.password)
                            .textFieldStyle(.roundedBorder)

                        TextField("Device name", text: $coordinator.deviceName)
                            .autocorrectionDisabled()
                            .textFieldStyle(.roundedBorder)

                        HStack(spacing: 10) {
                            Button(coordinator.isSigningIn ? "Signing In…" : "Sign In") {
                                coordinator.login()
                            }
                            .buttonStyle(VirtueButtonStyle(prominent: true))
                            .disabled(coordinator.isSigningIn)

                            Button("Runtime Overrides") {
                                showOverridesSheet = true
                            }
                            .buttonStyle(VirtueButtonStyle())
                        }

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

private struct StatusSheet: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                Section("Service") {
                    DetailRow(label: "Summary", value: coordinator.monitorSummary)
                    DetailRow(label: "Status", value: coordinator.statusMessage)
                    DetailRow(label: "Pending requests", value: "\(coordinator.pendingRequestCount)")
                    DetailRow(label: "API", value: coordinator.currentApiBaseUrl)
                }

                Section("Core Lifecycle") {
                    DetailRow(label: "User session", value: coordinator.coreUserSession)
                    DetailRow(label: "Primary service", value: coordinator.corePrimaryService)
                    DetailRow(label: "Capture permission", value: coordinator.coreCapturePermission)
                    DetailRow(label: "Capture availability", value: coordinator.coreCaptureAvailability)
                }

                Section("Timing") {
                    DetailRow(label: "Last loop", value: coordinator.lastCoreLoop)
                    DetailRow(label: "Last screenshot", value: coordinator.lastCoreScreenshot)
                    DetailRow(label: "Last batch", value: coordinator.lastCoreBatch)
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
}

private struct OverridesSheet: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section("Runtime Overrides") {
                    TextField("VIRTUE_BASE_API_URL", text: $coordinator.baseApiUrlOverride)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .listRowBackground(VirtueBrand.surface)
                    TextField("VIRTUE_CAPTURE_INTERVAL_SECONDS", text: $coordinator.captureIntervalOverride)
                        .keyboardType(.numberPad)
                        .listRowBackground(VirtueBrand.surface)
                    TextField("VIRTUE_BATCH_WINDOW_SECONDS", text: $coordinator.batchWindowOverride)
                        .keyboardType(.numberPad)
                        .listRowBackground(VirtueBrand.surface)
                }

                Section {
                    Button("Apply Overrides") {
                        coordinator.applyOverrides()
                        dismiss()
                    }
                    .listRowBackground(VirtueBrand.surface)
                    .listRowInsets(EdgeInsets(top: 0, leading: 16, bottom: 0, trailing: 16))
                }
            }
            .scrollContentBackground(.hidden)
            .background(VirtueBrand.bg)
            .navigationTitle("Runtime Overrides")
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

private enum VirtueBrand {
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
