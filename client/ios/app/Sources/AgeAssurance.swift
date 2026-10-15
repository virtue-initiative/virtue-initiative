import DeclaredAgeRange
import PermissionKit
import StoreKit
import SwiftUI
import UIKit

/// Age assurance where the law requires it (Texas SB 2420 today; Utah and
/// Louisiana in 2027). Apple confirms the person's age and gets a parent's
/// consent before a minor can download the app. Virtue's part is to check the
/// age range Apple shares, turn away anyone under the Terms of Use minimum,
/// and get fresh consent when a significant change ships. Apple stops the app
/// from launching when a parent withdraws consent, and the API signs out the
/// device (API-057).
///
/// The age range is never stored or sent anywhere, since the law only allows
/// it to be used for this check.
@MainActor
final class AgeAssurance: ObservableObject {
    enum Gate: Equatable {
        case checking
        case allowed
        case underMinimumAge
        case ageNotShared
        case awaitingParentalApproval
        case updateNotAcknowledged
    }

    /// A release that needs fresh consent from minors, and acknowledgment from
    /// adults where the law asks for it. SB 2420 counts a change as significant
    /// when it changes what personal data Virtue collects or shares, changes the
    /// age rating, adds purchases or ads, or materially changes how the app
    /// works. Add an entry in the release that ships such a change. Never
    /// reuse or remove an `id`.
    struct SignificantChange: Equatable {
        let id: String
        /// Shown to parents and guardians, so say plainly what changed.
        let description: String
    }

    static let significantChanges: [SignificantChange] = []

    /// The Terms of Use minimum age.
    static let minimumAge = 13
    private static let adultAge = 18

    private static let handledChangesKey = "virtue.ageAssurance.handledChanges"
    private static let baselineKey = "virtue.ageAssurance.baselineRecorded"
    private static let ageRatingCodeKey = "virtue.ageAssurance.ageRatingCode"

    @Published private(set) var gate: Gate
    private var isChecking = false
    /// Lives as long as the app; `AgeAssurance` is a single app-wide object.
    private var approvalListener: Task<Void, Never>?

    init() {
        if #available(iOS 26.2, *) {
            gate = .checking
            approvalListener = Task { [weak self] in
                for await response in AskCenter.shared.responses(for: SignificantAppUpdateTopic.self) {
                    guard response.choice.answer == .approval else { continue }
                    await self?.parentApproved()
                }
            }
        } else {
            // Apple's age assurance needs iOS 26.2. Accounts on older versions
            // aren't covered by it.
            gate = .allowed
        }
        recordBaselineIfNeeded()
    }

    /// Runs on launch, and again on foreground while the gate is closed so a
    /// change in Settings or a parent's answer takes effect.
    func check() async {
        guard #available(iOS 26.2, *), !isChecking else { return }
        isChecking = true
        defer { isChecking = false }

        do {
            guard try await AgeRangeService.shared.isEligibleForAgeFeatures else {
                gate = .allowed
                return
            }
        } catch {
            // The age service isn't offered here or is unreachable. Let the
            // person in and check again on the next launch.
            gate = .allowed
            return
        }

        guard let viewController = UIApplication.shared.virtueTopViewController else {
            gate = .allowed
            return
        }

        let response: AgeRangeService.Response
        do {
            // Texas's categories. The system substitutes the gates the law of
            // the person's region requires.
            response = try await AgeRangeService.shared.requestAgeRange(
                ageGates: Self.minimumAge, 16, Self.adultAge,
                in: viewController
            )
        } catch {
            gate = .allowed
            return
        }

        switch response {
        case .declinedSharing:
            gate = .ageNotShared
        case .sharing(let range):
            guard let lowerBound = range.lowerBound, lowerBound >= Self.minimumAge else {
                gate = .underMinimumAge
                return
            }
            await handleSignificantChanges(isMinor: lowerBound < Self.adultAge)
        @unknown default:
            gate = .allowed
        }
    }

    // MARK: Significant changes

    @available(iOS 26.2, *)
    private func handleSignificantChanges(isMinor: Bool) async {
        let pending = await pendingChanges()
        guard !pending.isEmpty else {
            gate = .allowed
            return
        }

        let needsParentalConsent: Bool
        let needsAdultAcknowledgment: Bool
        if #available(iOS 26.4, *) {
            let required = (try? await AgeRangeService.shared.requiredRegulatoryFeatures) ?? []
            needsParentalConsent = required.contains(.significantAppChangeRequiresParentalConsent)
            needsAdultAcknowledgment = required.contains(.significantAppChangeRequiresAdultNotification)
        } else {
            // Before 26.4 the system can't say which features apply, so treat
            // every eligible minor as needing consent.
            needsParentalConsent = true
            needsAdultAcknowledgment = false
        }

        let description = pending.map(\.description).joined(separator: " ")

        if isMinor && needsParentalConsent {
            gate = .awaitingParentalApproval
            await askParent(description: description)
            return
        }

        if !isMinor && needsAdultAcknowledgment {
            if #available(iOS 26.4, *) {
                guard let scene = UIApplication.shared.virtueActiveWindowScene else {
                    gate = .updateNotAcknowledged
                    return
                }
                do {
                    try await AgeRangeService.shared.showSignificantUpdateAcknowledgment(
                        in: scene,
                        updateDescription: description
                    )
                    markHandled(pending)
                    gate = .allowed
                } catch {
                    gate = .updateNotAcknowledged
                }
                return
            }
        }

        markHandled(pending)
        gate = .allowed
    }

    @available(iOS 26.2, *)
    private func askParent(description: String) async {
        guard let viewController = UIApplication.shared.virtueTopViewController else { return }
        let question = PermissionQuestion(
            significantAppUpdateTopic: SignificantAppUpdateTopic(description: description)
        )
        // The gate stays closed until `responses(for:)` delivers an approval.
        // If sending fails, "Ask Again" retries.
        try? await AskCenter.shared.ask(question, in: viewController)
    }

    private func parentApproved() async {
        if #available(iOS 26.2, *) {
            markHandled(await pendingChanges())
        }
        if gate == .awaitingParentalApproval {
            gate = .allowed
        }
    }

    /// Listed changes this install hasn't handled, plus an age rating change
    /// since the last launch.
    @available(iOS 26.2, *)
    private func pendingChanges() async -> [SignificantChange] {
        let handled = Set(UserDefaults.standard.stringArray(forKey: Self.handledChangesKey) ?? [])
        var pending = Self.significantChanges.filter { !handled.contains($0.id) }

        if let code = await AppStore.ageRatingCode {
            let defaults = UserDefaults.standard
            let lastCode = defaults.object(forKey: Self.ageRatingCodeKey) as? Int
            if lastCode == nil {
                defaults.set(code, forKey: Self.ageRatingCodeKey)
            } else if lastCode != code {
                let change = SignificantChange(
                    id: "age-rating-\(code)",
                    description: "Virtue's App Store age rating changed."
                )
                if !handled.contains(change.id) {
                    pending.append(change)
                }
            }
        }
        return pending
    }

    private func markHandled(_ changes: [SignificantChange]) {
        let defaults = UserDefaults.standard
        var handled = Set(defaults.stringArray(forKey: Self.handledChangesKey) ?? [])
        for change in changes {
            handled.insert(change.id)
            if change.id.hasPrefix("age-rating-"), let code = Int(change.id.dropFirst("age-rating-".count)) {
                defaults.set(code, forKey: Self.ageRatingCodeKey)
            }
        }
        defaults.set(Array(handled).sorted(), forKey: Self.handledChangesKey)
    }

    /// A fresh install already has every listed change, so only changes added
    /// after this install's first launch are pending for it.
    private func recordBaselineIfNeeded() {
        let defaults = UserDefaults.standard
        guard !defaults.bool(forKey: Self.baselineKey) else { return }
        markHandled(Self.significantChanges)
        defaults.set(true, forKey: Self.baselineKey)
    }
}

/// Shown instead of the app while `AgeAssurance` keeps the gate closed.
struct AgeGateView: View {
    @ObservedObject var ageAssurance: AgeAssurance

    var body: some View {
        VStack(alignment: .leading, spacing: 16) {
            if ageAssurance.gate == .checking {
                ProgressView("Checking your age range.")
                    .frame(maxWidth: .infinity)
            } else {
                Text(title)
                    .font(.title2.weight(.semibold))
                    .foregroundStyle(VirtueBrand.text)
                Text(message)
                    .foregroundStyle(VirtueBrand.textMuted)
                    .fixedSize(horizontal: false, vertical: true)
                if let retryLabel {
                    Button(retryLabel) {
                        Task { await ageAssurance.check() }
                    }
                    .buttonStyle(VirtueButtonStyle(prominent: true))
                }
            }
        }
        .padding(24)
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .center)
        .background(VirtueBrand.bg)
    }

    private var title: String {
        switch ageAssurance.gate {
        case .underMinimumAge: return "Virtue is for people 13 and older."
        case .ageNotShared: return "Share your age range to continue."
        case .awaitingParentalApproval: return "Waiting for a parent or guardian."
        case .updateNotAcknowledged: return "Review what changed."
        case .checking, .allowed: return ""
        }
    }

    private var message: String {
        switch ageAssurance.gate {
        case .underMinimumAge:
            return "Your Apple Account shows you are under 13, so you can't use Virtue on this device."
        case .ageNotShared:
            return "The law in your region requires Virtue to check your age range. Tap Try Again and share your age range when asked."
        case .awaitingParentalApproval:
            return "Virtue has changed since you last used it. The law in your region requires a parent or guardian to approve the change. Ask them to approve the request on their device."
        case .updateNotAcknowledged:
            return "Virtue has changed since you last used it. The law in your region requires you to review the change before you continue."
        case .checking, .allowed:
            return ""
        }
    }

    private var retryLabel: String? {
        switch ageAssurance.gate {
        case .ageNotShared, .underMinimumAge: return "Try Again"
        case .awaitingParentalApproval: return "Ask Again"
        case .updateNotAcknowledged: return "Review Changes"
        case .checking, .allowed: return nil
        }
    }
}

extension UIApplication {
    var virtueActiveWindowScene: UIWindowScene? {
        let scenes = connectedScenes.compactMap { $0 as? UIWindowScene }
        return scenes.first { $0.activationState == .foregroundActive } ?? scenes.first
    }

    /// The view controller system sheets should be presented from.
    var virtueTopViewController: UIViewController? {
        let scene = virtueActiveWindowScene
        var controller = (scene?.keyWindow ?? scene?.windows.first)?.rootViewController
        while let presented = controller?.presentedViewController {
            controller = presented
        }
        return controller
    }
}
