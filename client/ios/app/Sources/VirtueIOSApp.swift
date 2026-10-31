import SwiftUI
import UIKit

@main
struct VirtueIOSApp: App {
    @StateObject private var coordinator = MonitoringCoordinator()

    var body: some Scene {
        WindowGroup {
            ContentView(coordinator: coordinator)
                .onOpenURL { coordinator.handleOpenURL($0) }
                .tint(VirtueBrand.accent)
                .preferredColorScheme(.light)
        }
    }
}

/// Home is the root. The menu and every screen under it are pushed.
enum Route: Hashable {
    case menu, setupSteps, statusDetails, whatVirtueMonitors, reportIssue
}

struct ContentView: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @State private var path: [Route] = []
    @State private var showPauseConfirmation = false
    @State private var showLogoutConfirmation = false
    @State private var showReportSent = false

    var body: some View {
        NavigationStack(path: $path) {
            HomeView(coordinator: coordinator, onPause: { showPauseConfirmation = true })
                .navigationTitle("Home")
                .toolbar(.hidden, for: .navigationBar)
                .navigationDestination(for: Route.self) { route in
                    switch route {
                    case .menu:
                        MenuView(coordinator: coordinator, onSignOut: { showLogoutConfirmation = true })
                    case .setupSteps:
                        SafariSetupGuide(coordinator: coordinator)
                    case .statusDetails:
                        StatusDetailsView(coordinator: coordinator)
                    case .whatVirtueMonitors:
                        WhatVirtueMonitorsView()
                    case .reportIssue:
                        ReportBugSheet(coordinator: coordinator) {
                            showReportSent = true
                        }
                    }
                }
        }
        // Pop back to Home only once sign-out has finished, so Home never
        // shows a stale active state with working buttons while it runs.
        .onChange(of: coordinator.loggedIn) { loggedIn in
            if !loggedIn { path = [] }
        }
        .alert("Report sent", isPresented: $showReportSent) {
            Button("OK") {}
        } message: {
            Text("Thanks, your report was sent to the Virtue Initiative team.")
        }
        .alert("Pause monitoring?", isPresented: $showPauseConfirmation) {
            Button("Cancel", role: .cancel) {}
            Button("Pause Monitoring", role: .destructive) {
                Haptics.tap()
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
            Text("Signing out will deactivate this device and stop monitoring. Anyone monitoring you may be alerted. Logging in again will create a new device.")
        }
    }
}

enum Haptics {
    static func tap() {
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
    }
}

// MARK: - Home

private struct HomeView: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    let onPause: () -> Void
    @ScaledMetric(relativeTo: .title3) private var titleSize: CGFloat = 19

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 28) {
                header
                focus
            }
            .padding(.horizontal, 20)
            .padding(.vertical, 12)
        }
        .scrollDismissesKeyboard(.interactively)
        .background(VirtueBrand.bg)
    }

    private var header: some View {
        HStack(spacing: 8) {
            Link(destination: URL(string: "https://virtueinitiative.org")!) {
                HStack(spacing: 8) {
                    VirtueLogoShape()
                        .fill(VirtueBrand.logo)
                        .frame(width: 28, height: 30)
                        .accessibilityHidden(true)
                    Text("Virtue Initiative")
                        .font(.system(size: titleSize, weight: .semibold))
                        .foregroundStyle(VirtueBrand.text)
                }
            }
            Spacer(minLength: 0)
            NavigationLink(value: Route.menu) {
                Image(systemName: "line.3.horizontal")
                    .font(.title2)
                    .foregroundStyle(VirtueBrand.text)
                    .frame(minWidth: 44, minHeight: 44)
                    .contentShape(Rectangle())
            }
            .accessibilityLabel("Menu")
        }
    }

    @ViewBuilder
    private var focus: some View {
        switch coordinator.monitoringState {
        case .signedOut:
            SignInForm(coordinator: coordinator)
        case .needsSetup, .needsPermission:
            SetupWizard(coordinator: coordinator)
        case .active, .paused:
            StatusFocus(coordinator: coordinator, onPause: onPause)
        }
    }
}

private struct SignInForm: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @State private var isPasswordVisible = false
    @FocusState private var focus: Field?

    private enum Field { case email, password, deviceName }

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Text("Sign in")
                .font(.largeTitle.weight(.bold))
                .foregroundStyle(VirtueBrand.text)
                .accessibilityAddTraits(.isHeader)
            Text("Sign in to register this device and start monitoring.")
                .foregroundStyle(VirtueBrand.textMuted)

            VStack(spacing: 12) {
                TextField("Email", text: $coordinator.email)
                    .textContentType(.username)
                    .keyboardType(.emailAddress)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .submitLabel(.next)
                    .focused($focus, equals: .email)
                    .onSubmit { focus = .password }
                    .fieldStyle()

                // SwiftUI has no reveal affordance on SecureField, so swap
                // in a plain TextField while the eye toggle is on. Both
                // bind the same password, so toggling never loses input.
                HStack(spacing: 8) {
                    Group {
                        if isPasswordVisible {
                            TextField("Password", text: $coordinator.password)
                                .textInputAutocapitalization(.never)
                                .autocorrectionDisabled()
                        } else {
                            SecureField("Password", text: $coordinator.password)
                        }
                    }
                    .textContentType(.password)
                    .submitLabel(.next)
                    .focused($focus, equals: .password)
                    .onSubmit { focus = .deviceName }

                    Button {
                        isPasswordVisible.toggle()
                    } label: {
                        Image(systemName: isPasswordVisible ? "eye.slash" : "eye")
                            .foregroundStyle(VirtueBrand.textMuted)
                            .frame(minWidth: 44, minHeight: 44)
                            .contentShape(Rectangle())
                    }
                    .buttonStyle(.plain)
                    .accessibilityLabel(isPasswordVisible ? "Hide password" : "Show password")
                }
                .fieldStyle(trailingPadding: 0)

                TextField("Device name (default \"\(UIDevice.current.name)\")", text: $coordinator.deviceName)
                    .autocorrectionDisabled()
                    .submitLabel(.go)
                    .focused($focus, equals: .deviceName)
                    .onSubmit { signIn() }
                    .fieldStyle()
            }

            Button(coordinator.isSigningIn ? "Signing In…" : "Sign In") {
                signIn()
            }
            .buttonStyle(VirtueButtonStyle(prominent: true, fullWidth: true))
            .disabled(coordinator.isSigningIn)

            if let error = coordinator.loginError {
                Text(error)
                    .font(.subheadline)
                    .foregroundStyle(VirtueBrand.bad)
                    .fixedSize(horizontal: false, vertical: true)
            }

            Link(
                "Don't have an account? Sign up",
                destination: URL(string: "https://app.virtueinitiative.org/signup")!
            )
            .font(.subheadline)
            .foregroundStyle(VirtueBrand.link)
            .frame(maxWidth: .infinity, minHeight: 44)
        }
    }

    private func signIn() {
        focus = nil
        coordinator.login()
    }
}

private extension View {
    func fieldStyle(trailingPadding: CGFloat = 14) -> some View {
        self
            .padding(.leading, 14)
            .padding(.trailing, trailingPadding)
            .padding(.vertical, trailingPadding == 0 ? 0 : 12)
            .frame(minHeight: 48)
            .background(VirtueBrand.surface)
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .stroke(VirtueBrand.border, lineWidth: 1)
            )
    }
}

/// The active and paused home screen: a ring, the headline, one sentence and
/// the buttons that fit.
private struct StatusFocus: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    let onPause: () -> Void
    @Environment(\.openURL) private var openURL

    /// Partners are managed in the web app, not from this client.
    private let partnersURL = URL(string: "https://app.virtueinitiative.org/partners")!

    private var paused: Bool { coordinator.monitoringState == .paused }
    /// A missing count means unknown, not zero, so it never claims there are
    /// no partners.
    private var partnersTitle: String {
        guard let count = coordinator.coreStatus?.partnerCount else { return "Manage partners" }
        return count > 0 ? "Add more partners" : "Add partners"
    }

    var body: some View {
        VStack(spacing: 14) {
            StatusRing(
                kind: paused ? .warn : .good,
                size: 112,
                border: 6,
                systemImage: paused ? "pause.fill" : "checkmark"
            )
            .padding(.top, 16)
            Text(coordinator.monitoringState.headline)
                .font(.title.weight(.semibold))
                .foregroundStyle(VirtueBrand.text)
                .accessibilityAddTraits(.isHeader)
            Text(sentence)
                .foregroundStyle(VirtueBrand.textMuted)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)

            if let error = coordinator.actionError {
                Text(error)
                    .font(.subheadline)
                    .foregroundStyle(VirtueBrand.bad)
                    .multilineTextAlignment(.center)
                    .fixedSize(horizontal: false, vertical: true)
            }

            VStack(spacing: 10) {
                if paused {
                    Button("Resume Monitoring") {
                        Haptics.tap()
                        coordinator.toggleMonitoring()
                    }
                    .buttonStyle(VirtueButtonStyle(prominent: true, fullWidth: true))
                } else {
                    Button(partnersTitle) {
                        openURL(partnersURL)
                    }
                    .buttonStyle(VirtueButtonStyle(prominent: true, fullWidth: true))
                    Button("Pause Monitoring", action: onPause)
                        .buttonStyle(VirtueButtonStyle(fullWidth: true))
                    ExtensionCheckButton(coordinator: coordinator, style: .link)
                }
            }
            .frame(maxWidth: 300)
            .padding(.top, 8)
        }
        .frame(maxWidth: .infinity)
    }

    private var sentence: String {
        if paused {
            return "Monitoring is stopped on this device until you resume it."
        }
        var text = "Virtue is watching Safari."
        if let lastSeen = coordinator.safariSetup.lastMessageAt {
            text += " Safari last reported \(relativeTime(lastSeen))."
            if Date().timeIntervalSince(lastSeen) > 24 * 60 * 60 {
                text += " If you have used Safari since then, tap Check Extension."
            }
        }
        return text
    }
}

// MARK: - Menu and small screens

private struct MenuView: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    let onSignOut: () -> Void

    var body: some View {
        List {
            Section {
                NavigationLink("Setup Steps", value: Route.setupSteps)
                NavigationLink("Status Details", value: Route.statusDetails)
                NavigationLink("What Virtue Monitors", value: Route.whatVirtueMonitors)
            }
            .listRowBackground(VirtueBrand.surface)

            Section {
                NavigationLink("Report an Issue", value: Route.reportIssue)
                if coordinator.loggedIn {
                    Button(coordinator.isSigningOut ? "Signing Out…" : "Sign Out", role: .destructive, action: onSignOut)
                        .foregroundStyle(VirtueBrand.bad)
                        .disabled(coordinator.isSigningOut)
                }
            }
            .listRowBackground(VirtueBrand.surface)

            Section {
                Text("Build \(VirtueShared.buildLabel)")
                    .font(.footnote)
                    .foregroundStyle(VirtueBrand.textMuted)
                    .frame(maxWidth: .infinity)
                    .listRowBackground(Color.clear)
            }
        }
        .foregroundStyle(VirtueBrand.text)
        .scrollContentBackground(.hidden)
        .background(VirtueBrand.bg)
        .navigationTitle("Menu")
        .navigationBarTitleDisplayMode(.inline)
    }
}

// Short summary; the full list is on the linked help page. Keep both in step
// with what the Safari extension and daemon actually do (core SPEC CORE-003, CORE-004).
private struct WhatVirtueMonitorsView: View {
    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                Text("Virtue only captures Safari. It takes a screenshot of the web page you are viewing about every 5 minutes, blurred and with text blacked out.")
                    .foregroundStyle(VirtueBrand.text)
                    .fixedSize(horizontal: false, vertical: true)
                Link("Learn more", destination: URL(string: "https://virtueinitiative.org/help/what-virtue-monitors/ios")!)
                    .font(.subheadline)
                    .foregroundStyle(VirtueBrand.link)
            }
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(20)
        }
        .background(VirtueBrand.bg)
        .navigationTitle("What Virtue Monitors")
        .navigationBarTitleDisplayMode(.inline)
    }
}

// MARK: - Status Details

/// The status page (see `client/core/SPEC.md` CORE-010) written for users,
/// with everything for developers under Advanced.
private struct StatusDetailsView: View {
    @ObservedObject var coordinator: MonitoringCoordinator

    private var status: CoreServiceStatus? { coordinator.coreStatus }
    private var setup: SafariSetupState { coordinator.safariSetup }

    var body: some View {
        List {
            summarySection
            accountSection
            safariSection
            screenshotsSection
            problemsSection
            advancedSection
        }
        .scrollContentBackground(.hidden)
        .background(VirtueBrand.bg)
        .navigationTitle("Status Details")
        .navigationBarTitleDisplayMode(.inline)
    }

    private var summarySection: some View {
        Section {
            HStack(alignment: .top, spacing: 12) {
                StatusDot(kind: coordinator.monitoringState.kind)
                    .padding(.top, 6)
                VStack(alignment: .leading, spacing: 4) {
                    Text(coordinator.monitoringState.headline)
                        .font(.headline)
                    Text(coordinator.monitoringState.summarySentence)
                        .font(.subheadline)
                        .foregroundStyle(VirtueBrand.textMuted)
                }
            }
            .accessibilityElement(children: .combine)
            .listRowBackground(VirtueBrand.surface)
        }
    }

    private var accountSection: some View {
        Section("Account") {
            if coordinator.loggedIn {
                DetailRow(
                    kind: .good,
                    label: "Signed in",
                    value: status?.accountEmail ?? coordinator.accountEmail ?? "Yes",
                    hint: "Device: \(status?.deviceName ?? UIDevice.current.name)"
                )
            } else {
                DetailRow(kind: .neutral, label: "Signed in", value: "No")
            }
            if let count = status?.partnerCount {
                DetailRow(
                    kind: count > 0 ? .good : .warn,
                    label: "Partners",
                    value: "\(count)",
                    hint: count == 0 ? "Nobody is receiving your reports yet." : nil
                )
            } else {
                DetailRow(kind: .neutral, label: "Partners", value: "Unknown")
            }
        }
    }

    private var safariSection: some View {
        Section("Safari") {
            if setup.extensionTurnedOn {
                DetailRow(
                    kind: .good,
                    label: "Safari extension",
                    value: "On",
                    hint: setup.lastMessageAt.map { "Last heard from Safari \(relativeTime($0))." }
                )
            } else {
                DetailRow(
                    kind: setup.reportedOff ? .bad : .warn,
                    label: "Safari extension",
                    value: "Off",
                    hint: "Turn it on in Settings."
                )
            }
            accessRow
        }
    }

    @ViewBuilder
    private var accessRow: some View {
        let progress = SetupProgress(setup: setup, loggedIn: coordinator.loggedIn)
        let missing = [
            setup.allSitesGranted == false ? "All websites" : nil,
            setup.privateAllowed == false ? "Private Browsing" : nil,
        ].compactMap { $0 }
        if !missing.isEmpty {
            DetailRow(
                kind: .bad,
                label: "Access",
                value: "Missing: \(missing.joined(separator: ", "))",
                valueColor: VirtueBrand.bad,
                hint: "Allow it in Settings so Virtue can see every page."
            )
        } else if progress.isDone(.allowAllWebsites) == true && setup.privateAllowed == true {
            DetailRow(kind: .good, label: "Access", value: "All websites and Private Browsing")
        } else {
            DetailRow(kind: .warn, label: "Access", value: "Not confirmed")
        }
    }

    private var screenshotsSection: some View {
        let pending = (status?.pendingHashCount ?? 0) + (status?.pendingBatchCount ?? 0)
        return Section("Screenshots") {
            if let ms = status?.lastScreenshotAtMs {
                DetailRow(
                    kind: .good,
                    label: "Last screenshot",
                    value: coordinator.formatStatusTimestamp(ms),
                    hint: coordinator.safariLastPageHost.map { "Last page: \($0)" }
                )
            } else if status == nil {
                DetailRow(kind: .neutral, label: "Last screenshot", value: "Unknown")
            } else {
                DetailRow(
                    kind: .warn,
                    label: "Last screenshot",
                    value: "None yet",
                    hint: "Open a web page in Safari to take the first one."
                )
            }
            if status == nil {
                DetailRow(kind: .neutral, label: "Waiting to upload", value: "Unknown")
            } else {
                DetailRow(
                    kind: pending == 0 ? .good : .warn,
                    label: "Waiting to upload",
                    value: pending == 0 ? "None" : "\(pending) screenshot\(pending == 1 ? "" : "s")",
                    hint: status?.lastBatchAtMs.map { "Last upload: \(coordinator.formatStatusTimestamp($0))" }
                )
            }
        }
    }

    private var problemsSection: some View {
        Section("Problems") {
            if status == nil {
                DetailRow(kind: .neutral, label: "Recent problems", value: "Unknown")
            } else if let error = status?.recentErrors?.first {
                DetailRow(
                    kind: .bad,
                    label: "Recent problems",
                    value: error.message,
                    valueColor: VirtueBrand.bad,
                    hint: "\(coordinator.formatStatusTimestamp(error.atMs)) · \(error.context)"
                )
            } else {
                DetailRow(kind: .good, label: "Recent problems", value: "None")
            }
        }
    }

    private var advancedSection: some View {
        Section {
            DisclosureGroup("Advanced") {
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
                DetailRow(label: "Waiting for hash", value: "\(status?.pendingHashCount ?? 0)")
                DetailRow(label: "Waiting in batch", value: "\(status?.pendingBatchCount ?? 0)")
                DetailRow(label: "Pending requests", value: "\(coordinator.pendingRequestCount)")
                DetailRow(label: "Last loop", value: coordinator.lastCoreLoop)
                DetailRow(label: "Last attempt", value: coordinator.lastCoreScreenshotAttempt)
                DetailRow(label: "Last skip reason", value: status?.lastSkipReason?.label ?? "<none>")
                DetailRow(label: "Capture health", value: coordinator.safariCaptureHealth)
                DetailRow(label: "Permission state", value: coordinator.safariPermissionSummary)
                DetailRow(label: "Extension status", value: coordinator.safariDaemonStatus)
                DetailRow(label: "Last heartbeat", value: coordinator.safariLastHeartbeat)
                DetailRow(label: "Last frame", value: coordinator.safariLastFrame)
                DetailRow(label: "Last page", value: coordinator.safariLastPage)
                DetailRow(label: "Last error", value: coordinator.safariLastError)
                DetailRow(label: "Status", value: coordinator.statusMessage)
                if let errors = status?.recentErrors, !errors.isEmpty {
                    ForEach(Array(errors.enumerated()), id: \.offset) { _, error in
                        DetailRow(
                            label: "Recent error",
                            value: error.message,
                            hint: "\(coordinator.formatStatusTimestamp(error.atMs)) · \(error.context)"
                        )
                    }
                } else {
                    DetailRow(label: "Recent errors", value: "<none>")
                }

                Button(
                    coordinator.forceCaptureRequested
                        ? "Waiting for Safari…"
                        : "Capture Next Safari Page"
                ) {
                    coordinator.requestForceCapture()
                }
                .disabled(coordinator.forceCaptureRequested || !coordinator.loggedIn)

                Text("Capture Next Safari Page takes a screenshot the next time you view a page in Safari. It uploads that screenshot right away, along with anything else waiting to upload.")
                    .font(.footnote)
                    .foregroundStyle(VirtueBrand.textMuted)
            }
            .listRowBackground(VirtueBrand.surface)
        }
    }
}

/// One row: optional status dot, muted label, value, and an optional hint.
/// Stacks the label above the value at accessibility text sizes.
private struct DetailRow: View {
    var kind: StatusKind?
    let label: String
    let value: String
    var valueColor: Color = VirtueBrand.text
    var hint: String?
    @Environment(\.dynamicTypeSize) private var typeSize

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            if typeSize.isAccessibilitySize {
                VStack(alignment: .leading, spacing: 2) { labelView; valueView }
            } else {
                HStack(alignment: .firstTextBaseline, spacing: 12) {
                    labelView
                    Spacer(minLength: 8)
                    valueView.multilineTextAlignment(.trailing)
                }
            }
            if let hint {
                Text(hint)
                    .font(.footnote)
                    .foregroundStyle(VirtueBrand.textMuted)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
        .padding(.vertical, 2)
        .accessibilityElement(children: .combine)
        .listRowBackground(VirtueBrand.surface)
    }

    private var labelView: some View {
        HStack(spacing: 8) {
            if let kind { StatusDot(kind: kind) }
            Text(label)
                .font(.subheadline)
                .foregroundStyle(VirtueBrand.textMuted)
        }
    }

    private var valueView: some View {
        Text(value)
            .font(.subheadline)
            .foregroundStyle(valueColor)
            .textSelection(.enabled)
    }
}

// MARK: - Status vocabulary

enum StatusKind {
    case good, warn, bad, neutral

    var color: Color {
        switch self {
        case .good: return VirtueBrand.good
        case .warn: return VirtueBrand.warn
        case .bad: return VirtueBrand.bad
        case .neutral: return VirtueBrand.textMuted.opacity(0.5)
        }
    }

    var accessibilityLabel: String {
        switch self {
        case .good: return "Working"
        case .warn: return "Not yet"
        case .bad: return "Needs attention"
        case .neutral: return "Not applicable"
        }
    }
}

extension MonitoringState {
    var headline: String {
        switch self {
        case .signedOut: return "Signed out"
        case .paused: return "Monitoring paused"
        case .needsSetup: return "Setup not finished"
        case .needsPermission: return "Safari permission needed"
        case .active: return "Monitoring active"
        }
    }

    var kind: StatusKind {
        switch self {
        case .signedOut: return .neutral
        case .paused, .needsSetup: return .warn
        case .needsPermission: return .bad
        case .active: return .good
        }
    }

    var summarySentence: String {
        switch self {
        case .signedOut:
            return "Sign in to register this device and start monitoring."
        case .paused:
            return "Monitoring is stopped on this device until you resume it."
        case .needsSetup:
            return "Virtue is not monitoring yet. Finish the Safari setup steps."
        case .needsPermission:
            return "The Safari extension is missing a permission. Allow it in Settings so Virtue can see every page."
        case .active:
            return "The Safari extension is on. Virtue takes screenshots as you browse in Safari."
        }
    }
}

struct StatusDot: View {
    let kind: StatusKind

    var body: some View {
        Circle()
            .fill(kind.color)
            .frame(width: 10, height: 10)
            .accessibilityElement()
            .accessibilityLabel(kind.accessibilityLabel)
    }
}

/// A circle in the state color with a symbol or short text centered inside.
struct StatusRing: View {
    let kind: StatusKind
    let size: CGFloat
    let border: CGFloat
    var label: String?
    var systemImage: String?

    var body: some View {
        ZStack {
            Circle()
                .fill(VirtueBrand.surface)
            Circle()
                .strokeBorder(kind.color, lineWidth: border)
            if let systemImage {
                Image(systemName: systemImage)
                    .font(.system(size: size * 0.42, weight: .bold))
                    .foregroundStyle(kind.color)
            } else if let label {
                Text(label)
                    .font(.system(size: size * 0.42, weight: .semibold))
                    .foregroundStyle(kind.color)
            }
        }
        .frame(width: size, height: size)
        .accessibilityHidden(true)
    }
}

// MARK: - Shared styling

/// The V from landing/public/logo.svg, cropped to the V itself (viewBox
/// `3 6 54 58` of the 60 unit drawing).
struct VirtueLogoShape: Shape {
    func path(in rect: CGRect) -> Path {
        let originX = 21.459, originY = 201.684, width = 54.0, height = 58.0
        func p(_ x: Double, _ y: Double) -> CGPoint {
            CGPoint(
                x: rect.minX + (x - originX) / width * rect.width,
                y: rect.minY + (y - originY) / height * rect.height
            )
        }
        var path = Path()
        path.move(to: p(22.334, 203.140))
        path.addLine(to: p(41.952, 249.639))
        path.addLine(to: p(48.160, 234.879))
        path.addLine(to: p(36.141, 203.138))
        path.closeSubpath()

        path.move(to: p(64.839, 203.161))
        path.addLine(to: p(43.551, 252.456))
        path.addCurve(to: p(55.923, 247.815), control1: p(43.551, 252.456), control2: p(50.473, 261.429))
        path.addCurve(to: p(74.555, 203.156), control1: p(62.757, 230.379), control2: p(69.101, 220.585))
        path.closeSubpath()
        return path
    }
}

struct VirtueButtonStyle: ButtonStyle {
    var prominent: Bool = false
    var fullWidth: Bool = false

    func makeBody(configuration: Configuration) -> some View {
        StyledButton(configuration: configuration, prominent: prominent, fullWidth: fullWidth)
    }

    private struct StyledButton: View {
        let configuration: Configuration
        let prominent: Bool
        let fullWidth: Bool
        @Environment(\.isEnabled) private var isEnabled

        var body: some View {
            configuration.label
                .font(.body.weight(.semibold))
                .multilineTextAlignment(.center)
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .frame(maxWidth: fullWidth ? .infinity : nil, minHeight: 44)
                .background(
                    prominent
                        ? (configuration.isPressed ? VirtueBrand.accent.opacity(0.85) : VirtueBrand.accent)
                        : (configuration.isPressed ? VirtueBrand.border : VirtueBrand.bgSubtle)
                )
                .foregroundStyle(prominent ? Color.white : VirtueBrand.accent)
                .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .stroke(prominent ? Color.clear : VirtueBrand.border, lineWidth: 1)
                )
                .opacity(isEnabled ? 1 : 0.4)
                .animation(.easeInOut(duration: 0.1), value: configuration.isPressed)
        }
    }
}

struct Card<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            content
        }
        .padding(20)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(VirtueBrand.surface)
        .clipShape(RoundedRectangle(cornerRadius: 12, style: .continuous))
        .overlay(
            RoundedRectangle(cornerRadius: 12, style: .continuous)
                .stroke(VirtueBrand.border, lineWidth: 1)
        )
    }
}

enum VirtueBrand {
    // Forest green — matches --accent / --forest in shared-web/tokens.css
    static let accent = Color(
        red: 30.0 / 255.0,
        green: 58.0 / 255.0,
        blue: 46.0 / 255.0
    )
    // Link color — matches --link in shared-web/tokens.css
    static let link = Color(red: 179.0 / 255.0, green: 67.0 / 255.0, blue: 0.0 / 255.0)
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
    // Status colors from the client redesign: working, needs action, and
    // starting or paused.
    static let good = Color(red: 47.0 / 255.0, green: 125.0 / 255.0, blue: 79.0 / 255.0)
    static let bad = Color(red: 192.0 / 255.0, green: 57.0 / 255.0, blue: 43.0 / 255.0)
    static let warn = ochre
    // The logo's own dark green, from landing/public/logo.svg.
    static let logo = Color(red: 22.0 / 255.0, green: 48.0 / 255.0, blue: 38.0 / 255.0)
}
