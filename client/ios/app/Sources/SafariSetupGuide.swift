import SafariServices
import SwiftUI
import UIKit

/// The five setup steps. Each turns done from what the Safari extension
/// reports (`SafariSetupState`), so "signed in" is never mistaken for
/// "finished". The website's iOS install instructions
/// (landing/src/content/download-instructions/ios.md) show the same steps and
/// screenshots, so keep the two in step.
enum SetupStep: Int, CaseIterable, Identifiable {
    // Same order as the switches appear on the Settings page.
    case signIn, turnOnExtension, allowPrivate, allowAllWebsites, tryIt
    var id: Int { rawValue }

    var title: String {
        switch self {
        case .signIn: return "Sign in"
        case .turnOnExtension: return "Turn on the Safari extension"
        case .allowAllWebsites: return "Allow it on all websites"
        case .allowPrivate: return "Allow it in Private Browsing"
        case .tryIt: return "Check that it works"
        }
    }
}

/// Which steps are done, derived from the coordinator's facts.
struct SetupProgress {
    let setup: SafariSetupState
    let loggedIn: Bool

    /// `true` done, `false` not done, `nil` when Virtue can't tell.
    func isDone(_ step: SetupStep) -> Bool? {
        switch step {
        case .signIn: return loggedIn
        case .turnOnExtension: return setup.extensionTurnedOn
        // Safari does not always let the extension check this. Without an
        // answer, a captured page proves it; otherwise Virtue can't tell.
        case .allowAllWebsites: return setup.allSitesGranted ?? (setup.hasCaptured ? true : nil)
        case .allowPrivate: return setup.extensionTurnedOn ? setup.privateAllowed : false
        // Any message from the extension, the check page's "on" report
        // included, shows it works. A captured page is not required.
        case .tryIt: return setup.extensionTurnedOn
        }
    }

    var current: SetupStep? {
        SetupStep.allCases.first { isDone($0) == false }
    }

    var isComplete: Bool { current == nil }

    func statusLine(_ step: SetupStep) -> String? {
        let done = isDone(step)
        switch (step, done) {
        case (.turnOnExtension, false) where setup.reportedOff:
            return setup.reportedOffAt.map { "The extension was turned off when Virtue checked \(relativeTime($0))." }
        case (.turnOnExtension, true):
            return setup.lastMessageAt.map { "Last heard from Safari \(relativeTime($0))." }
        case (.tryIt, true):
            if let frame = setup.lastFrameAt { return "Last web page seen \(relativeTime(frame))." }
            return setup.lastMessageAt.map { "Last heard from Safari \(relativeTime($0))." }
        case (.allowPrivate, nil):
            return "Virtue can't check this setting. Make sure it's on."
        case (.allowAllWebsites, false) where setup.allSitesGranted == false:
            return "Safari reports this is no longer allowed."
        case (.allowPrivate, false) where setup.privateAllowed == false:
            return "Safari reports this is turned off."
        default:
            return nil
        }
    }
}

/// The Setup Steps screen: all five steps as an accordion. The first
/// unfinished step starts expanded.
struct SafariSetupGuide: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @State private var expandedStep: SetupStep?

    private var progress: SetupProgress {
        SetupProgress(setup: coordinator.safariSetup, loggedIn: coordinator.loggedIn)
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                Text(
                    progress.isComplete
                        ? "Every step is done. Virtue monitors Safari while you browse."
                        : "Virtue only works after the Safari extension is turned on."
                )
                .font(.subheadline)
                .foregroundStyle(VirtueBrand.textMuted)
                .fixedSize(horizontal: false, vertical: true)

                Card {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(SetupStep.allCases) { step in
                            stepRow(step)
                            if step != SetupStep.allCases.last {
                                Divider().overlay(VirtueBrand.border)
                            }
                        }
                    }
                }

                Link("See these steps on the website", destination: URL(string: "https://virtueinitiative.org/download#ios")!)
                    .font(.subheadline)
                    .foregroundStyle(VirtueBrand.link)
            }
            .padding(20)
        }
        .background(VirtueBrand.bg)
        .navigationTitle("Setup Steps")
        .navigationBarTitleDisplayMode(.inline)
    }

    private func stepRow(_ step: SetupStep) -> some View {
        let done = progress.isDone(step)
        let current = progress.current
        let expanded = (expandedStep ?? current) == step
        return VStack(alignment: .leading, spacing: 10) {
            Button {
                withAnimation(.easeInOut(duration: 0.15)) {
                    expandedStep = expanded ? nil : step
                }
            } label: {
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    StepBadge(number: step.rawValue + 1, done: done, current: step == current)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(step.title)
                            .font(.body.weight(step == current ? .semibold : .regular))
                            .foregroundStyle(VirtueBrand.text)
                            .multilineTextAlignment(.leading)
                        if let status = progress.statusLine(step) {
                            Text(status)
                                .font(.footnote)
                                .foregroundStyle(VirtueBrand.textMuted)
                                .multilineTextAlignment(.leading)
                        }
                    }
                    Spacer(minLength: 0)
                    Image(systemName: expanded ? "chevron.up" : "chevron.down")
                        .font(.footnote)
                        .foregroundStyle(VirtueBrand.textMuted)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityHint(expanded ? "Collapses the step" : "Expands the step")

            if expanded {
                SetupStepDetails(step: step, coordinator: coordinator)
                    .padding(.leading, 34)
            }
        }
        .padding(.vertical, 10)
    }
}

/// The Home focus area while setup is unfinished or a permission is missing:
/// one step at a time, with Back and Next.
struct SetupWizard: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    /// `nil` follows the first unfinished step, so the card moves on by
    /// itself when Virtue sees a step get done.
    @State private var selected: SetupStep?

    private var progress: SetupProgress {
        SetupProgress(setup: coordinator.safariSetup, loggedIn: coordinator.loggedIn)
    }

    private var step: SetupStep { selected ?? progress.current ?? .tryIt }
    private var needsPermission: Bool { coordinator.monitoringState == .needsPermission }

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            HStack(spacing: 14) {
                StatusRing(
                    kind: needsPermission ? .bad : .warn,
                    size: 48,
                    border: 4,
                    label: needsPermission ? "!" : "\(step.rawValue + 1)"
                )
                VStack(alignment: .leading, spacing: 2) {
                    Text(needsPermission ? "Safari permission needed" : "Setup not finished")
                        .font(.title3.weight(.semibold))
                        .foregroundStyle(VirtueBrand.text)
                        .accessibilityAddTraits(.isHeader)
                    Text("Step \(step.rawValue + 1) of \(SetupStep.allCases.count)")
                        .font(.subheadline)
                        .foregroundStyle(VirtueBrand.textMuted)
                }
            }

            HStack(spacing: 6) {
                ForEach(SetupStep.allCases) { s in
                    Capsule()
                        .fill(segmentColor(s))
                        .frame(height: 5)
                }
            }
            .accessibilityHidden(true)

            Card {
                VStack(alignment: .leading, spacing: 12) {
                    Text(step.title)
                        .font(.headline)
                        .foregroundStyle(VirtueBrand.text)
                    if let status = progress.statusLine(step) {
                        Text(status)
                            .font(.footnote)
                            .foregroundStyle(VirtueBrand.textMuted)
                            .fixedSize(horizontal: false, vertical: true)
                    }
                    SetupStepDetails(step: step, coordinator: coordinator)
                }
            }

            HStack(spacing: 10) {
                Button("Back") { move(-1) }
                    .buttonStyle(VirtueButtonStyle(fullWidth: true))
                    .disabled(step == SetupStep.allCases.first)
                Button("Next") { move(1) }
                    .buttonStyle(VirtueButtonStyle(prominent: true, fullWidth: true))
                    .disabled(step == SetupStep.allCases.last)
            }
        }
    }

    private func segmentColor(_ s: SetupStep) -> Color {
        if progress.isDone(s) == true { return VirtueBrand.good }
        if s == step { return VirtueBrand.accent }
        return VirtueBrand.border
    }

    private func move(_ delta: Int) {
        guard let next = SetupStep(rawValue: step.rawValue + delta) else { return }
        withAnimation(.easeInOut(duration: 0.15)) {
            selected = next
        }
    }
}

/// A step's instructions, images and buttons.
struct SetupStepDetails: View {
    let step: SetupStep
    @ObservedObject var coordinator: MonitoringCoordinator
    @Environment(\.openURL) private var openURL

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            switch step {
            case .signIn:
                Instruction("Enter your Virtue email and password on the home screen, then tap Sign In.")
            case .turnOnExtension:
                Instruction(openSettingsText)
                openSettingsButton
                if !hasDirectExtensionLink {
                    if #available(iOS 18.0, *) {
                        StepImage("ios_setup_apps_safari")
                    }
                    Instruction("Scroll down and tap Extensions.", image: "ios_setup_extensions")
                    Instruction("Tap Virtue Safari Capture.", image: "ios_setup_extension_list")
                } else {
                    Instruction("If Safari shows a list of extensions, tap Virtue Safari Capture.")
                }
                Instruction("Turn on Allow Extension.", image: "ios_setup_allow_extension")
                Instruction("Virtue sees this the next time you open Safari.")
                ScreenTimeNote()
            case .allowAllWebsites:
                Instruction(reopenText)
                openSettingsButton
                Instruction("Under Permissions, tap All Websites.", image: "ios_setup_all_websites")
                Instruction("Tap Allow.", image: "ios_setup_all_websites_allow")
            case .allowPrivate:
                Instruction(reopenText)
                openSettingsButton
                Instruction("Turn on Allow in Private Browsing.", image: "ios_setup_allow_private")
            case .tryIt:
                Instruction("Close Safari completely. Swipe up from the bottom of the screen and pause, then swipe Safari up and away.")
                Instruction("Tap Check Extension. Safari opens a page that shows whether the extension is on.")
                Instruction("Come back to Virtue. This step turns green once the check page reports that the extension is on.")
                ExtensionCheckButton(coordinator: coordinator, style: .prominent)
            }
        }
    }

    /// `openSettingsURLString` opens Virtue's own page in Settings. iOS has no
    /// public URL for the Safari or extension settings (`App-prefs:` is
    /// private API and fails App Review), so the user has to back out first.
    /// The iOS 18.6 simulator showed the Apps list instead, but real devices
    /// land on Virtue's page.
    private var openSettingsText: String {
        if hasDirectExtensionLink {
            return "Tap Open Settings. It opens Safari's settings for the Virtue extension."
        }
        if #available(iOS 18.0, *) {
            return "Tap Open Settings. It opens Virtue's own settings page, so tap the back button at the top left. Then tap Safari in the list of apps."
        }
        return "Tap Open Settings. It opens Virtue's own settings page, so tap the back button at the top left. Then scroll down and tap Safari."
    }

    private var reopenText: String {
        hasDirectExtensionLink
            ? "Tap Open Settings to go back to the Virtue Safari Capture settings."
            : "Open the Virtue Safari Capture page in Settings again (\(extensionPath))."
    }

    /// iOS 26.2 added a link straight to an extension's settings. It was
    /// not available to test, so `openSettings` falls back to the app's
    /// own Settings page if it throws.
    private var hasDirectExtensionLink: Bool {
        #if compiler(>=6.2.3)
        if #available(iOS 26.2, *) { return true }
        #endif
        return false
    }

    private var extensionPath: String {
        if #available(iOS 18.0, *) {
            return "Apps, Safari, Extensions, Virtue Safari Capture"
        }
        return "Safari, Extensions, Virtue Safari Capture"
    }

    private var openSettingsButton: some View {
        Button("Open Settings") {
            Task { await openSettings() }
        }
        .buttonStyle(VirtueButtonStyle(prominent: true))
    }

    private func openSettings() async {
        #if compiler(>=6.2.3)
        if #available(iOS 26.2, *), let appID = Bundle.main.bundleIdentifier {
            do {
                // The extension's bundle id is the app's plus ".broadcast"
                // (project.yml).
                try await SFSafariSettings.openExtensionsSettings(forIdentifiers: ["\(appID).broadcast"])
                return
            } catch {}
        }
        #endif
        // Before iOS 26.2 there is no public link to Safari's extension
        // settings; this gets the user to the app list that holds Safari.
        if let url = URL(string: UIApplication.openSettingsURLString) {
            openURL(url)
        }
    }
}

enum VirtueSetupLinks {
    /// Any page would do for the app's check. The extension also fills in
    /// this one (content.js), so it shows the result inside Safari too.
    static let checkPage = URL(string: "https://virtueinitiative.org/check-extension")!
}

/// Opens the check page in Safari and shows what Virtue heard once the user
/// comes back. See `MonitoringCoordinator.startExtensionCheck`.
struct ExtensionCheckButton: View {
    enum Style { case prominent, link }

    @ObservedObject var coordinator: MonitoringCoordinator
    var style: Style = .prominent
    @Environment(\.openURL) private var openURL

    var body: some View {
        VStack(alignment: style == .link ? .center : .leading, spacing: 8) {
            Button("Check Extension") {
                openURL(coordinator.startExtensionCheck())
            }
            .modifier(CheckButtonStyle(style: style))

            if let result {
                Label(result.text, systemImage: result.icon)
                    .font(.footnote)
                    .foregroundStyle(result.passed ? VirtueBrand.accent : VirtueBrand.textMuted)
                    .multilineTextAlignment(style == .link ? .center : .leading)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
    }

    private struct CheckButtonStyle: ViewModifier {
        let style: Style

        func body(content: Content) -> some View {
            switch style {
            case .prominent:
                content.buttonStyle(VirtueButtonStyle(prominent: true))
            case .link:
                content
                    .font(.subheadline)
                    .foregroundStyle(VirtueBrand.link)
                    .frame(minHeight: 44)
            }
        }
    }

    private var result: (text: String, icon: String, passed: Bool)? {
        switch coordinator.extensionCheck {
        case .waiting(_, true):
            return ("No answer yet. On the check page, tap Back to Virtue to see the result.", "hourglass", false)
        case .passed:
            return ("The extension is on.", "checkmark.circle.fill", true)
        case .failed:
            return ("The extension is turned off. Turn it on with the Turn on the Safari extension step.", "exclamationmark.triangle", false)
        case .waiting(_, false), nil:
            return nil
        }
    }
}

private struct StepBadge: View {
    let number: Int
    let done: Bool?
    var current = false

    var body: some View {
        ZStack {
            if done == true {
                Image(systemName: "checkmark.circle.fill")
                    .font(.system(size: 22))
                    .foregroundStyle(VirtueBrand.good)
            } else {
                Circle()
                    .stroke(
                        current ? VirtueBrand.accent : (done == nil ? VirtueBrand.ochre : VirtueBrand.border),
                        lineWidth: current ? 2 : 1.5
                    )
                    .frame(width: 22, height: 22)
                Text("\(number)")
                    .font(.caption.weight(.semibold))
                    .foregroundStyle(VirtueBrand.text)
            }
        }
        .frame(width: 24, height: 24)
        .alignmentGuide(.firstTextBaseline) { $0[VerticalAlignment.center] + 5 }
        .accessibilityLabel(done == true ? "Step \(number), done" : "Step \(number)")
    }
}

private struct Instruction: View {
    let text: String
    let image: String?

    init(_ text: String, image: String? = nil) {
        self.text = text
        self.image = image
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Text(text)
                .font(.subheadline)
                .foregroundStyle(VirtueBrand.text)
                .fixedSize(horizontal: false, vertical: true)
            if let image {
                StepImage(image)
            }
        }
    }
}

/// A cropped Settings screenshot with the thing to tap outlined. Made on an
/// iOS 18 simulator; the same images are on the website's install page.
private struct StepImage: View {
    let name: String

    init(_ name: String) {
        self.name = name
    }

    var body: some View {
        if let uiImage = UIImage(named: name) {
            Image(uiImage: uiImage)
                .resizable()
                .scaledToFit()
                .frame(maxWidth: 260, alignment: .leading)
                .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 8, style: .continuous)
                        .stroke(VirtueBrand.border, lineWidth: 1)
                )
                .accessibilityHidden(true)
        }
    }
}

private struct ScreenTimeNote: View {
    var body: some View {
        HStack(alignment: .top, spacing: 8) {
            Image(systemName: "hourglass")
                .foregroundStyle(VirtueBrand.ochre)
            Text("If Allow Extension is grayed out or won't stay on, Screen Time restrictions are blocking it. Whoever knows your Screen Time passcode can open Settings, tap Screen Time, and turn off Content & Privacy Restrictions. Turn on the extension, then turn the restrictions back on.")
                .font(.footnote)
                .foregroundStyle(VirtueBrand.textMuted)
                .fixedSize(horizontal: false, vertical: true)
        }
        .padding(10)
        .background(VirtueBrand.bgSubtle)
        .clipShape(RoundedRectangle(cornerRadius: 6, style: .continuous))
    }
}
