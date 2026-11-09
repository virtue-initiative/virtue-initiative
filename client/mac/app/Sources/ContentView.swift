import AppKit
import SwiftUI
import VirtueKit

/// What the window's content area shows. Everything but Home is reached from
/// the toolbar menu and has a back button (CLIENT-024).
enum Screen {
    case home, details, monitors, report

    var title: String {
        switch self {
        case .home: return "Virtue Initiative"
        case .details: return "Status Details"
        case .monitors: return "What Virtue Monitors"
        case .report: return "Report an Issue"
        }
    }
}

struct ContentView: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @ObservedObject var updateController: UpdateController
    @State private var screen: Screen = .home
    @State private var showStopConfirmation = false
    @State private var showLogoutConfirmation = false
    @State private var showReportSent = false

    var body: some View {
        content
            .frame(
                minWidth: 440, idealWidth: 560, maxWidth: .infinity, minHeight: 520,
                idealHeight: 640, maxHeight: .infinity
            )
            .background(VirtueBrand.bg)
            .navigationTitle(screen.title)
            .toolbar { toolbar }
            .overlay(alignment: .bottom) { toast }
            .background { shortcuts }
            .alert("Report sent", isPresented: $showReportSent) {
                Button("OK") {}
            } message: {
                Text("Thanks, your report was sent to the Virtue Initiative team.")
            }
            .alert("Stop monitoring and quit?", isPresented: $showStopConfirmation) {
                Button("Cancel", role: .cancel) {}
                Button("Stop Monitoring and Quit", role: .destructive) {
                    coordinator.stopMonitoringAndQuit()
                }
            } message: {
                Text(
                    "This will stop monitoring on this device and quit Virtue. People monitoring you may be alerted."
                )
            }
            .alert("Sign out?", isPresented: $showLogoutConfirmation) {
                Button("Cancel", role: .cancel) {}
                Button("Sign Out", role: .destructive) {
                    coordinator.logout()
                    screen = .home
                }
            } message: {
                Text(
                    "Signing out will deactivate this device and stop monitoring. Anyone monitoring you may be alerted. Logging in again will create a new device."
                )
            }
    }

    @ViewBuilder
    private var content: some View {
        switch screen {
        case .home:
            HomeView(coordinator: coordinator) { showStopConfirmation = true }
        case .details:
            StatusDetailsView(coordinator: coordinator)
        case .monitors:
            WhatVirtueMonitorsView()
        case .report:
            ReportIssueView(coordinator: coordinator) {
                screen = .home
                showReportSent = true
            }
        }
    }

    @ToolbarContentBuilder
    private var toolbar: some ToolbarContent {
        ToolbarItem(placement: .navigation) {
            if screen == .home {
                WebsiteLink()
            } else {
                HStack(spacing: 6) {
                    Button {
                        screen = .home
                    } label: {
                        Image(systemName: "chevron.backward")
                    }
                    .pointingHandCursor()
                    .keyboardShortcut("[", modifiers: .command)
                    .help("Back")
                    .accessibilityLabel("Back")
                    Text(screen.title)
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(VirtueBrand.text)
                }
            }
        }
        // With the title hidden nothing pushes the menu to the trailing edge,
        // so a Spacer does (it becomes a flexible toolbar space).
        ToolbarItemGroup(placement: .automatic) {
            Spacer()
            Menu {
                menuItems
            } label: {
                Image(systemName: "ellipsis")
            }
            .menuIndicator(.hidden)
            .help("Menu")
            .accessibilityLabel("Menu")
        }
    }

    @ViewBuilder
    private var menuItems: some View {
        if coordinator.loggedIn {
            Button(coordinator.isForceCapturing ? "Screenshot uploading…" : "Test Screenshot") {
                coordinator.forceCapture()
            }
            .disabled(coordinator.needsScreenRecording || coordinator.isForceCapturing)
            if coordinator.needsScreenRecording {
                Text("Allow Screen Recording to use this feature")
            }
            Divider()
        }
        Button("Status Details") { screen = .details }
        Button("What Virtue Monitors") { screen = .monitors }
        Button("Report an Issue") { screen = .report }
        if updateController.isEnabled || coordinator.loggedIn {
            Divider()
        }
        if updateController.isEnabled {
            Button("Check for Updates") {
                updateController.checkForUpdates()
            }
            .disabled(!updateController.canCheckForUpdates)
        }
        if coordinator.loggedIn {
            Button(coordinator.isSigningOut ? "Signing Out…" : "Sign Out", role: .destructive) {
                showLogoutConfirmation = true
            }
            .disabled(coordinator.isSigningOut)
        }
        Divider()
        Text("Build \(coordinator.buildLabel)")
        if let updateStatus = updateController.statusMessage {
            Text(updateStatus)
        }
    }

    /// Test Screenshot results (CLIENT-008), shown over whichever screen is open.
    @ViewBuilder
    private var toast: some View {
        if let message = coordinator.forceCaptureMessage {
            Text(message)
                .font(.callout)
                .foregroundStyle(.white)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 14)
                .padding(.vertical, 9)
                .background(
                    VirtueBrand.text.opacity(0.88),
                    in: RoundedRectangle(cornerRadius: 8, style: .continuous)
                )
                .padding(.horizontal, 24)
                .padding(.bottom, 20)
                .transition(.opacity)
                .accessibilityAddTraits(.updatesFrequently)
        }
    }

    /// An `LSUIElement` app has no main menu, so the standard Cmd+W and Cmd+Q
    /// need handling here. Cmd+Q asks the same question as Stop Monitoring.
    private var shortcuts: some View {
        ZStack {
            Button("Close Window") {
                NSApp.keyWindow?.performClose(nil)
            }
            .keyboardShortcut("w", modifiers: .command)
            Button("Quit") {
                if coordinator.loggedIn {
                    showStopConfirmation = true
                } else {
                    coordinator.stopMonitoringAndQuit()
                }
            }
            .keyboardShortcut("q", modifiers: .command)
        }
        .opacity(0)
        .frame(width: 0, height: 0)
        .accessibilityHidden(true)
    }
}

/// The toolbar's logo and name. It sits where people grab to drag the window,
/// so the hover background and cursor say it's a link (CLIENT-024).
private struct WebsiteLink: View {
    @State private var isHovering = false

    var body: some View {
        Link(destination: URL(string: "https://virtueinitiative.org")!) {
            HStack(spacing: 8) {
                VirtueLogoShape()
                    .fill(VirtueBrand.logo)
                    .frame(width: 19, height: 20)
                Text("Virtue Initiative")
                    .font(.system(size: 15, weight: .semibold))
                    .foregroundStyle(VirtueBrand.text)
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(
                RoundedRectangle(cornerRadius: 6, style: .continuous)
                    .fill(isHovering ? VirtueBrand.border.opacity(0.6) : Color.clear)
            )
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .onHover { isHovering = $0 }
        .pointingHandCursor()
        .help("Open virtueinitiative.org")
        .accessibilityLabel("Virtue Initiative website")
    }
}

// MARK: - Home

/// The window's state, first match wins (CLIENT-006). Screen Recording comes
/// first because CLIENT-025 asks for it before sign-in.
enum HomeState {
    case needsScreenRecording, signedOut, stopped, starting, active

    var headline: String {
        switch self {
        case .needsScreenRecording: return "Screen Recording off"
        case .signedOut: return "Signed out"
        case .stopped: return "Monitoring stopped"
        case .starting: return "Starting…"
        case .active: return "Monitoring active"
        }
    }

    var kind: StatusKind {
        switch self {
        case .needsScreenRecording, .stopped: return .bad
        case .signedOut: return .neutral
        case .starting: return .warn
        case .active: return .good
        }
    }
}

extension MonitoringCoordinator {
    var homeState: HomeState {
        if needsScreenRecording { return .needsScreenRecording }
        if !loggedIn { return .signedOut }
        if unexpectedStopMessage != nil { return .stopped }
        if daemonStatus == .running, coreStatus?.lastScreenshotAtMs != nil { return .active }
        return .starting
    }
}

private struct HomeView: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    let onStop: () -> Void

    var body: some View {
        ScrollView {
            Group {
                switch coordinator.homeState {
                case .needsScreenRecording:
                    ScreenRecordingSteps(coordinator: coordinator)
                case .signedOut:
                    SignInForm(coordinator: coordinator)
                case .stopped, .starting, .active:
                    StatusFocus(coordinator: coordinator, onStop: onStop)
                }
            }
            .frame(maxWidth: 400)
            .padding(.horizontal, 24)
            .padding(.top, 32)
            .padding(.bottom, 24)
            .frame(maxWidth: .infinity)
        }
    }
}

private struct ScreenRecordingSteps: View {
    @ObservedObject var coordinator: MonitoringCoordinator

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text("Allow Screen Recording")
                .font(.system(size: 26, weight: .semibold))
                .foregroundStyle(VirtueBrand.text)
                .accessibilityAddTraits(.isHeader)
            Text("Virtue can't take screenshots until you allow Screen Recording.")
                .font(.system(size: 14))
                .foregroundStyle(VirtueBrand.textMuted)
                .fixedSize(horizontal: false, vertical: true)

            // The window moves on by itself once the permission is on, so
            // there is no restart step (CLIENT-025).
            Card {
                VStack(alignment: .leading, spacing: 10) {
                    Text("Turn on Virtue")
                        .font(.system(size: 15, weight: .semibold))
                    Text("Open Privacy & Security, then Screen Recording. Turn on **Virtue**.")
                    Button("Open System Settings") {
                        coordinator.openScreenRecordingSettings()
                    }
                    .buttonStyle(VirtueButtonStyle(prominent: true))
                    Image("ScreenRecordingSettings")
                        .resizable()
                        .scaledToFit()
                        .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
                        .overlay(
                            RoundedRectangle(cornerRadius: 6, style: .continuous)
                                .stroke(VirtueBrand.border, lineWidth: 1)
                        )
                        .accessibilityLabel(
                            "Privacy & Security settings, Screen Recording, with Virtue turned on")
                    Text("If macOS asks to quit and reopen Virtue, either choice works.")
                        .font(.system(size: 12))
                        .foregroundStyle(VirtueBrand.textMuted)
                }
                .foregroundStyle(VirtueBrand.text)
                .fixedSize(horizontal: false, vertical: true)
            }
            .padding(.top, 10)
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct SignInForm: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @State private var isPasswordVisible = false
    @FocusState private var focused: Field?

    private enum Field { case email, password, deviceName }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Log in")
                .font(.system(size: 26, weight: .semibold))
                .foregroundStyle(VirtueBrand.text)
                .accessibilityAddTraits(.isHeader)
            Text("Log in to start monitoring on this device.")
                .font(.system(size: 14))
                .foregroundStyle(VirtueBrand.textMuted)
                .padding(.bottom, 8)

            TextField("Email", text: $coordinator.email)
                .textContentType(.username)
                .focused($focused, equals: .email)
                .modifier(InputStyle(isFocused: focused == .email))
            ZStack(alignment: .trailing) {
                Group {
                    // SwiftUI has no reveal on SecureField, so swap in a plain
                    // TextField. Both bind the same text.
                    if isPasswordVisible {
                        TextField("Password", text: $coordinator.password)
                    } else {
                        SecureField("Password", text: $coordinator.password)
                    }
                }
                .textContentType(.password)
                .focused($focused, equals: .password)
                .padding(.trailing, 26)
                .modifier(InputStyle(isFocused: focused == .password))
                Button {
                    isPasswordVisible.toggle()
                    focused = .password
                } label: {
                    Image(systemName: isPasswordVisible ? "eye.slash" : "eye")
                        .foregroundStyle(VirtueBrand.textMuted)
                        .frame(width: 24, height: 24)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .pointingHandCursor()
                .padding(.trailing, 8)
                .help(isPasswordVisible ? "Hide password" : "Show password")
                .accessibilityLabel(isPasswordVisible ? "Hide password" : "Show password")
            }
            TextField(
                "Device name (default \"\(NativeBridge.defaultDeviceName())\")",
                text: $coordinator.deviceName
            )
            .focused($focused, equals: .deviceName)
            .modifier(InputStyle(isFocused: focused == .deviceName))

            Button(coordinator.isSigningIn ? "Logging In…" : "Log In", action: coordinator.login)
                .buttonStyle(VirtueButtonStyle(prominent: true, large: true))
                .keyboardShortcut(.defaultAction)
                .disabled(coordinator.isSigningIn)
                .padding(.top, 4)

            Link(
                "Don't have an account? Sign up",
                destination: URL(string: "https://app.virtueinitiative.org/signup")!
            )
            .font(.system(size: 13))
            .foregroundStyle(VirtueBrand.link)
            .pointingHandCursor()
            .frame(maxWidth: .infinity)

            // A daemon that failed to start would fail the login too, so say
            // why up front (CLIENT-024).
            ForEach(
                [coordinator.loginError, coordinator.unexpectedStopMessage].compactMap { $0 },
                id: \.self
            ) { message in
                Text(message.replacingOccurrences(of: "\n\n", with: " "))
                    .font(.system(size: 13))
                    .foregroundStyle(VirtueBrand.bad)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .onSubmit(coordinator.login)
        .onAppear {
            if coordinator.email.isEmpty { focused = .email }
        }
    }
}

/// A larger field than `.roundedBorder`, on the surface color, with an accent
/// ring while focused (the plain style draws no focus ring of its own).
private struct InputStyle: ViewModifier {
    let isFocused: Bool

    func body(content: Content) -> some View {
        content
            .textFieldStyle(.plain)
            .font(.system(size: 14))
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .background(
                VirtueBrand.surface, in: RoundedRectangle(cornerRadius: 7, style: .continuous)
            )
            .overlay(
                RoundedRectangle(cornerRadius: 7, style: .continuous)
                    .stroke(
                        isFocused ? VirtueBrand.accent : VirtueBrand.border,
                        lineWidth: isFocused ? 2 : 1)
            )
    }
}

private struct StatusFocus: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    let onStop: () -> Void
    @Environment(\.openURL) private var openURL

    /// Partners are managed in the web app, not from this client.
    private let partnersURL = URL(string: "https://app.virtueinitiative.org/partners")!

    private var state: HomeState { coordinator.homeState }

    /// A missing count means unknown, not zero, so it never claims there are
    /// no partners.
    private var partnersTitle: String {
        guard let count = coordinator.coreStatus?.partnerCount else { return "Manage partners" }
        return count > 0 ? "Add more partners" : "Add partners"
    }

    var body: some View {
        VStack(spacing: 14) {
            StatusRing(kind: state.kind, systemImage: ringSymbol)
            Text(state.headline)
                .font(.system(size: 26, weight: .semibold))
                .foregroundStyle(VirtueBrand.text)
                .padding(.top, 6)
                .accessibilityAddTraits(.isHeader)
            Text(sentence)
                .font(.system(size: 14))
                .foregroundStyle(VirtueBrand.textMuted)
                .multilineTextAlignment(.center)
                .fixedSize(horizontal: false, vertical: true)

            VStack(spacing: 10) {
                if state == .stopped {
                    Button("Relaunch Virtue") {
                        coordinator.restartApp()
                    }
                    .buttonStyle(VirtueButtonStyle(prominent: true, large: true))
                } else {
                    Button(partnersTitle) {
                        openURL(partnersURL)
                    }
                    .buttonStyle(VirtueButtonStyle(prominent: true, large: true))
                    Button("Stop Monitoring & Quit", action: onStop)
                        .buttonStyle(VirtueButtonStyle(large: true))
                }
            }
            .frame(maxWidth: 300)
            .padding(.top, 10)
        }
        .frame(maxWidth: .infinity)
    }

    private var ringSymbol: String {
        switch state {
        case .active: return "checkmark"
        case .stopped: return "exclamationmark"
        default: return "ellipsis"
        }
    }

    private var sentence: String {
        switch state {
        case .stopped:
            return (coordinator.unexpectedStopMessage ?? "")
                .replacingOccurrences(of: "\n\n", with: " ")
        case .active:
            let last =
                coordinator.coreStatus?.lastScreenshotAtMs.map {
                    " Last screenshot at \(shortTime($0))."
                } ?? ""
            return "Virtue is capturing your main display.\(last)"
        default:
            return "Virtue is getting ready. The first screenshot is taken within a few minutes."
        }
    }
}

// MARK: - What Virtue Monitors

// Short summary; the full list is on the linked help page. Keep both in step
// with what the daemon actually does (core SPEC CORE-003, CORE-004).
private struct WhatVirtueMonitorsView: View {
    var body: some View {
        ScrollView {
            Card {
                VStack(alignment: .leading, spacing: 12) {
                    Text(
                        "Virtue takes a screenshot of your main display about every 5 minutes. Screenshots are blurred, have text blacked out, and can only be seen by you and your partners."
                    )
                    .font(.system(size: 14))
                    .foregroundStyle(VirtueBrand.text)
                    .fixedSize(horizontal: false, vertical: true)
                    Link(
                        "Learn more",
                        destination: URL(
                            string: "https://virtueinitiative.org/help/what-virtue-monitors/mac")!
                    )
                    .font(.system(size: 14))
                    .foregroundStyle(VirtueBrand.link)
                    .pointingHandCursor()
                }
            }
            .frame(maxWidth: 400)
            .padding(24)
            .frame(maxWidth: .infinity)
        }
    }
}

// MARK: - Shared pieces

/// Local time of day, with the date too when it isn't today.
func shortTime(_ timestampMs: Int64) -> String {
    let date = Date(timeIntervalSince1970: TimeInterval(timestampMs) / 1000)
    let formatter = DateFormatter()
    formatter.dateStyle = Calendar.current.isDateInToday(date) ? .none : .medium
    formatter.timeStyle = .short
    return formatter.string(from: date)
}

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

struct StatusDot: View {
    let kind: StatusKind

    var body: some View {
        Circle()
            .fill(kind.color)
            .frame(width: 9, height: 9)
            .accessibilityElement()
            .accessibilityLabel(kind.accessibilityLabel)
    }
}

/// The 112 pt home ring: the state color around the surface, with a symbol.
private struct StatusRing: View {
    let kind: StatusKind
    let systemImage: String

    var body: some View {
        ZStack {
            Circle().fill(VirtueBrand.surface)
            Circle().strokeBorder(kind.color, lineWidth: 6)
            Image(systemName: systemImage)
                .font(.system(size: 46, weight: .bold))
                .foregroundStyle(kind.color)
        }
        .frame(width: 112, height: 112)
        .accessibilityHidden(true)
    }
}

/// The V from landing/public/logo.svg, cropped to the V itself (viewBox
/// `3 6 54 58` of the 60 unit drawing).
struct VirtueLogoShape: Shape {
    func path(in rect: CGRect) -> Path {
        let originX = 21.459
        let originY = 201.684
        let width = 54.0
        let height = 58.0
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
        path.addCurve(
            to: p(55.923, 247.815), control1: p(43.551, 252.456), control2: p(50.473, 261.429))
        path.addCurve(
            to: p(74.555, 203.156), control1: p(62.757, 230.379), control2: p(69.101, 220.585))
        path.closeSubpath()
        return path
    }
}
