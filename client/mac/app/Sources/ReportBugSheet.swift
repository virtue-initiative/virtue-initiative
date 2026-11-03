import SwiftUI
import VirtueKit

/// "Report an Issue" (CLIENT-016): a message, an optional contact email
/// (prefilled when signed in), and an opt-out checkbox to attach logs.
struct ReportIssueView: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    let onSent: () -> Void

    @State private var message: String = ""
    @State private var contactEmail: String = ""
    @State private var includeLogs = true
    @State private var isSending = false
    @State private var errorText: String?

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                TextEditor(text: $message)
                    .font(.system(size: 14))
                    .scrollContentBackground(.hidden)
                    .padding(6)
                    .frame(height: 160)
                    .background(VirtueBrand.surface, in: RoundedRectangle(cornerRadius: 7, style: .continuous))
                    .overlay(alignment: .topLeading) {
                        if message.isEmpty {
                            Text("Describe what happened.")
                                .font(.system(size: 14))
                                .foregroundStyle(VirtueBrand.textMuted)
                                .padding(.top, 6)
                                .padding(.leading, 11)
                                .allowsHitTesting(false)
                        }
                    }
                    .overlay(
                        RoundedRectangle(cornerRadius: 7, style: .continuous)
                            .stroke(VirtueBrand.border)
                    )
                    .accessibilityLabel("Describe what happened")

                TextField("Contact email (optional)", text: $contactEmail)
                    .textFieldStyle(.plain)
                    .font(.system(size: 14))
                    .padding(.horizontal, 10)
                    .padding(.vertical, 8)
                    .background(VirtueBrand.surface, in: RoundedRectangle(cornerRadius: 7, style: .continuous))
                    .overlay(
                        RoundedRectangle(cornerRadius: 7, style: .continuous)
                            .stroke(VirtueBrand.border)
                    )

                VStack(alignment: .leading, spacing: 4) {
                    Toggle("Attach logs from the last two days", isOn: $includeLogs)
                        .toggleStyle(AccentCheckboxStyle())
                    Text(
                        "Includes timestamps, monitoring status, and error messages. " +
                        "No screenshots or window titles are included. Known tokens are redacted automatically."
                    )
                    .font(.system(size: 12))
                    .foregroundStyle(VirtueBrand.textMuted)
                    .padding(.leading, 20)
                    .fixedSize(horizontal: false, vertical: true)
                }

                if let errorText {
                    Text(errorText)
                        .font(.system(size: 13))
                        .foregroundStyle(VirtueBrand.bad)
                        .fixedSize(horizontal: false, vertical: true)
                }

                Button(isSending ? "Sending…" : "Send Report") {
                    send()
                }
                .buttonStyle(VirtueButtonStyle(prominent: true, large: true))
                .disabled(isSending)
            }
            .frame(maxWidth: 400)
            .padding(24)
            .frame(maxWidth: .infinity)
        }
        .onAppear {
            if contactEmail.isEmpty {
                contactEmail = coordinator.accountEmail ?? ""
            }
        }
    }

    /// The system checkbox ignores `.tint` and stays system blue, so draw one
    /// in the accent color.
    private struct AccentCheckboxStyle: ToggleStyle {
        func makeBody(configuration: Configuration) -> some View {
            Button {
                configuration.isOn.toggle()
            } label: {
                HStack(spacing: 6) {
                    RoundedRectangle(cornerRadius: 3.5, style: .continuous)
                        .fill(configuration.isOn ? VirtueBrand.accent : VirtueBrand.surface)
                        .overlay(
                            RoundedRectangle(cornerRadius: 3.5, style: .continuous)
                                .stroke(configuration.isOn ? Color.clear : VirtueBrand.border, lineWidth: 1)
                        )
                        .overlay {
                            if configuration.isOn {
                                Image(systemName: "checkmark")
                                    .font(.system(size: 9, weight: .bold))
                                    .foregroundStyle(.white)
                            }
                        }
                        .frame(width: 14, height: 14)
                    configuration.label
                        .foregroundStyle(VirtueBrand.text)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .accessibilityValue(configuration.isOn ? "On" : "Off")
        }
    }

    private func send() {
        let trimmedMessage = message.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedMessage.isEmpty else {
            errorText = "Please describe the issue."
            return
        }

        let trimmedEmail = contactEmail.trimmingCharacters(in: .whitespacesAndNewlines)
        errorText = nil
        isSending = true
        coordinator.submitBugReport(
            message: trimmedMessage,
            contactEmail: trimmedEmail.isEmpty ? nil : trimmedEmail,
            includeLogs: includeLogs
        ) { error in
            isSending = false
            if let error {
                errorText = error
                return
            }
            onSent()
        }
    }
}
