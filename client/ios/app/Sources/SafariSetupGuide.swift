import SwiftUI
import UIKit

/// The numbered setup checklist on the main screen. Each step turns green from
/// what the Safari extension reports (`SafariSetupState`), so "signed in" is
/// never mistaken for "finished". The first unfinished step is expanded with
/// its instructions, screenshots and buttons. The website's iOS install
/// instructions (landing/src/content/download-instructions/ios.md) show the
/// same steps and screenshots, so keep the two in step.
struct SafariSetupGuide: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @State private var expandedStep: Step?
    @Environment(\.openURL) private var openURL

    enum Step: Int, CaseIterable, Identifiable {
        // Same order as the switches appear on the Settings page.
        case signIn, turnOnExtension, allowPrivate, allowAllWebsites, tryIt
        var id: Int { rawValue }
    }

    /// `true` done, `false` not done, `nil` when Virtue can't tell.
    private func isDone(_ step: Step) -> Bool? {
        let setup = coordinator.safariSetup
        switch step {
        case .signIn: return coordinator.loggedIn
        case .turnOnExtension: return setup.extensionTurnedOn
        case .allowAllWebsites: return setup.allWebsitesAllowed
        case .allowPrivate: return setup.extensionTurnedOn ? setup.privateAllowed : false
        case .tryIt: return setup.hasCaptured
        }
    }

    private var currentStep: Step? {
        Step.allCases.first { isDone($0) == false }
    }

    var isComplete: Bool { currentStep == nil }

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text(isComplete ? "Safari setup" : "Finish setting up Virtue")
                .font(.title3.weight(.semibold))
                .foregroundStyle(VirtueBrand.text)
            Text(
                isComplete
                    ? "Every step is done. Virtue monitors Safari while you browse."
                    : "Virtue only works after the Safari extension is turned on. Follow each step. They turn green when Virtue sees they are done."
            )
            .font(.subheadline)
            .foregroundStyle(VirtueBrand.textMuted)
            .fixedSize(horizontal: false, vertical: true)

            VStack(alignment: .leading, spacing: 0) {
                ForEach(Step.allCases) { step in
                    stepRow(step)
                    if step != Step.allCases.last {
                        Divider().overlay(VirtueBrand.border)
                    }
                }
            }

            Link("See these steps on the website", destination: URL(string: "https://virtueinitiative.org/download#ios")!)
                .font(.subheadline)
                .foregroundStyle(VirtueBrand.link)
        }
    }

    private func stepRow(_ step: Step) -> some View {
        let done = isDone(step)
        let expanded = (expandedStep ?? currentStep) == step
        return VStack(alignment: .leading, spacing: 10) {
            Button {
                withAnimation(.easeInOut(duration: 0.15)) {
                    expandedStep = expanded ? nil : step
                }
            } label: {
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    StepBadge(number: step.rawValue + 1, done: done)
                    VStack(alignment: .leading, spacing: 2) {
                        Text(title(step))
                            .font(.body.weight(step == currentStep ? .semibold : .regular))
                            .foregroundStyle(VirtueBrand.text)
                            .multilineTextAlignment(.leading)
                        if let status = statusLine(step, done: done) {
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

            if expanded {
                details(step)
                    .padding(.leading, 34)
            }
        }
        .padding(.vertical, 10)
    }

    private func title(_ step: Step) -> String {
        switch step {
        case .signIn: return "Sign in"
        case .turnOnExtension: return "Turn on the Safari extension"
        case .allowAllWebsites: return "Allow it on all websites"
        case .allowPrivate: return "Allow it in Private Browsing"
        case .tryIt: return "Check that it works"
        }
    }

    private func statusLine(_ step: Step, done: Bool?) -> String? {
        let setup = coordinator.safariSetup
        switch (step, done) {
        case (.turnOnExtension, false) where setup.reportedOff:
            return setup.reportedOffAt.map { "The extension was turned off when Virtue checked \(relative($0))." }
        case (.turnOnExtension, true):
            return setup.lastMessageAt.map { "Last heard from Safari \(relative($0))." }
        case (.tryIt, true):
            return setup.lastFrameAt.map { "Last web page seen \(relative($0))." }
        case (.allowPrivate, nil):
            return "Virtue can't check this setting. Make sure it's on."
        case (.allowAllWebsites, false) where setup.allSitesGranted == false && setup.hasCaptured:
            return "Safari reports this is no longer allowed."
        case (.allowPrivate, false) where setup.privateAllowed == false:
            return "Safari reports this is turned off."
        default:
            return nil
        }
    }

    @ViewBuilder
    private func details(_ step: Step) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            switch step {
            case .signIn:
                Instruction("Enter your Virtue email and password in the Account section below, then tap Sign In.")
            case .turnOnExtension:
                Instruction(openSettingsText)
                openSettingsButton
                if #available(iOS 18.0, *) {
                    StepImage("ios_setup_apps_safari")
                }
                Instruction("Scroll down and tap Extensions.", image: "ios_setup_extensions")
                Instruction("Tap Virtue Safari Capture.", image: "ios_setup_extension_list")
                Instruction("Turn on Allow Extension.", image: "ios_setup_allow_extension")
                Instruction("Virtue sees this the next time you open Safari.")
                ScreenTimeNote()
            case .allowAllWebsites:
                Instruction("Open the Virtue Safari Capture page in Settings again (\(extensionPath)).")
                openSettingsButton
                Instruction("Under Permissions, tap All Websites.", image: "ios_setup_all_websites")
                Instruction("Tap Allow.", image: "ios_setup_all_websites_allow")
            case .allowPrivate:
                Instruction("Open the Virtue Safari Capture page in Settings again (\(extensionPath)).")
                openSettingsButton
                Instruction("Turn on Allow in Private Browsing.", image: "ios_setup_allow_private")
            case .tryIt:
                Instruction("Close Safari completely. Swipe up from the bottom of the screen and pause, then swipe Safari up and away.")
                Instruction("Tap Check Extension. Safari opens a page that shows whether the extension is on.")
                Instruction("Come back to Virtue. This step turns green once Safari has shown Virtue a web page.")
                ExtensionCheckButton(coordinator: coordinator)
            }
        }
    }

    /// `openSettingsURLString` opens Virtue's own page in Settings. iOS has no
    /// public URL for the Safari or extension settings (`App-prefs:` is
    /// private API and fails App Review), so the user has to back out first.
    /// The iOS 18.6 simulator showed the Apps list instead, but real devices
    /// land on Virtue's page.
    private var openSettingsText: String {
        if #available(iOS 18.0, *) {
            return "Tap Open Settings. It opens Virtue's own settings page, so tap the back button at the top left. Then tap Safari in the list of apps."
        }
        return "Tap Open Settings. It opens Virtue's own settings page, so tap the back button at the top left. Then scroll down and tap Safari."
    }

    private var extensionPath: String {
        if #available(iOS 18.0, *) {
            return "Apps, Safari, Extensions, Virtue Safari Capture"
        }
        return "Safari, Extensions, Virtue Safari Capture"
    }

    private var openSettingsButton: some View {
        Button("Open Settings") {
            // iOS has no public link to Safari's extension settings; this
            // gets the user to the app list that holds Safari.
            if let url = URL(string: UIApplication.openSettingsURLString) {
                openURL(url)
            }
        }
        .buttonStyle(VirtueButtonStyle(prominent: true))
    }

    private func relative(_ date: Date) -> String {
        relativeTime(date)
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
    @ObservedObject var coordinator: MonitoringCoordinator
    @Environment(\.openURL) private var openURL

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Button("Check Extension") {
                openURL(coordinator.startExtensionCheck())
            }
            .buttonStyle(VirtueButtonStyle(prominent: true))

            if let result {
                Label(result.text, systemImage: result.icon)
                    .font(.footnote)
                    .foregroundStyle(result.passed ? VirtueBrand.accent : VirtueBrand.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
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

    var body: some View {
        ZStack {
            if done == true {
                Image(systemName: "checkmark.circle.fill")
                    .font(.system(size: 22))
                    .foregroundStyle(VirtueBrand.accent)
            } else {
                Circle()
                    .stroke(done == nil ? VirtueBrand.ochre : VirtueBrand.border, lineWidth: 1.5)
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
