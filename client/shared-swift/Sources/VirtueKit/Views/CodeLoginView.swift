import SwiftUI

/// Passwordless sign-in (CORE-020/CORE-021): the device shows a short code and
/// the user types it into an already-signed-in web session.
///
/// Dumb view, like `LoginFormView`: no business logic, just bindings and
/// callbacks. Each platform's coordinator owns the pairing and the polling.
public struct CodeLoginView: View {
    @Binding private var deviceName: String
    private let userCode: String?
    private let isRequestingCode: Bool
    private let errorText: String?
    private let onGetCode: () -> Void
    private let onUsePassword: () -> Void

    public init(
        deviceName: Binding<String>,
        userCode: String?,
        isRequestingCode: Bool,
        errorText: String?,
        onGetCode: @escaping () -> Void,
        onUsePassword: @escaping () -> Void
    ) {
        self._deviceName = deviceName
        self.userCode = userCode
        self.isRequestingCode = isRequestingCode
        self.errorText = errorText
        self.onGetCode = onGetCode
        self.onUsePassword = onUsePassword
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: VirtueSpacing.s3) {
            Text("Get a code here, then enter it on the Virtue website while signed in.")
                .font(.subheadline)
                .foregroundStyle(VirtueBrand.textMuted)

            TextField("Device name", text: $deviceName)
                .textFieldStyle(.roundedBorder)
                .autocorrectionDisabled()

            Button(buttonTitle, action: onGetCode)
                .buttonStyle(VirtueButtonStyle(prominent: true))
                .disabled(isRequestingCode)

            if let userCode {
                // Monospaced digits keep the six glyphs on an even rhythm, so a
                // code read off this screen and typed into another is easy to
                // track character by character.
                Text(userCode)
                    .font(.system(size: 34, weight: .semibold, design: .monospaced))
                    .kerning(4)
                    .foregroundStyle(VirtueBrand.text)
                    .frame(maxWidth: .infinity, alignment: .center)
                    .textSelection(.enabled)
                    .accessibilityLabel(spelledOut(userCode))

                Text("Waiting for you to approve this code on the website.")
                    .font(.subheadline)
                    .foregroundStyle(VirtueBrand.textMuted)
                    .frame(maxWidth: .infinity, alignment: .center)
            }

            Link(
                "Open the Devices page",
                destination: URL(string: "https://app.virtueinitiative.org/devices?add")!
            )
            .font(.subheadline)
            .foregroundStyle(VirtueBrand.link)

            Button("Use a password instead", action: onUsePassword)
                .buttonStyle(.plain)
                .font(.subheadline)
                .foregroundStyle(VirtueBrand.link)

            if let errorText {
                Text(errorText)
                    .font(.subheadline)
                    .foregroundStyle(VirtueBrand.danger)
            }
        }
    }

    private var buttonTitle: String {
        if isRequestingCode {
            return "Getting Code…"
        }
        return userCode == nil ? "Get Code" : "Get a New Code"
    }

    /// VoiceOver reads `K7R-M3X` as a word; spacing the characters out makes it
    /// read them one at a time, which is the only useful way to hear a code.
    private func spelledOut(_ code: String) -> String {
        code.map(String.init).joined(separator: " ")
    }
}
