import SwiftUI

/// Dumb login form: no business logic, just bindings and callbacks. Each
/// platform's coordinator owns the actual login/validation flow.
public struct LoginFormView: View {
    @Binding private var email: String
    @Binding private var password: String
    @Binding private var deviceName: String
    private let isSigningIn: Bool
    private let loginError: String?
    private let onSubmit: () -> Void

    public init(
        email: Binding<String>,
        password: Binding<String>,
        deviceName: Binding<String>,
        isSigningIn: Bool,
        loginError: String?,
        onSubmit: @escaping () -> Void
    ) {
        self._email = email
        self._password = password
        self._deviceName = deviceName
        self.isSigningIn = isSigningIn
        self.loginError = loginError
        self.onSubmit = onSubmit
    }

    public var body: some View {
        VStack(alignment: .leading, spacing: VirtueSpacing.s3) {
            TextField("Email", text: $email)
                .textFieldStyle(.roundedBorder)
            SecureField("Password", text: $password)
                .textFieldStyle(.roundedBorder)
            TextField("Device name", text: $deviceName)
                .textFieldStyle(.roundedBorder)

            Button(isSigningIn ? "Signing In…" : "Sign In", action: onSubmit)
                .buttonStyle(VirtueButtonStyle(prominent: true))
                .disabled(isSigningIn)

            if let loginError {
                Text(loginError)
                    .font(.subheadline)
                    .foregroundStyle(VirtueBrand.danger)
            }
        }
    }
}
