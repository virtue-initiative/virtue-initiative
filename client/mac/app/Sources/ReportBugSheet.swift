import SwiftUI
import VirtueKit

/// "Report a Bug" form, ported from the Windows client's `ShowReportBugDialogAsync`:
/// a message box, an optional contact email (pre-filled when signed in), and an
/// opt-out "include logs" checkbox with the same disclosure text.
struct ReportBugSheet: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    let onSent: () -> Void
    @Environment(\.dismiss) private var dismiss

    @State private var message: String = ""
    @State private var contactEmail: String = ""
    @State private var includeLogs = true
    @State private var isSending = false
    @State private var errorText: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text("Report a Bug")
                    .font(.headline)
                Spacer()
                Button("Cancel") {
                    dismiss()
                }
                .disabled(isSending)
            }
            .padding()

            VStack(alignment: .leading, spacing: VirtueSpacing.s3) {
                TextEditor(text: $message)
                    .padding(.top, 10)
                    .padding(.leading, 1)
                    .frame(height: 120)
                    .overlay(alignment: .topLeading) {
                        if message.isEmpty {
                            Text("Describe the issue")
                                .foregroundStyle(VirtueBrand.textMuted)
                                .padding(.top, 8)
                                .padding(.leading, 5)
                                .allowsHitTesting(false)
                        }
                    }
                    .padding(4)
                    .overlay(
                        RoundedRectangle(cornerRadius: VirtueRadius.button)
                            .stroke(VirtueBrand.border)
                    )

                TextField("Contact email (optional)", text: $contactEmail)
                    .textFieldStyle(.roundedBorder)

                Toggle("Include the last two days of diagnostic logs", isOn: $includeLogs)

                Text(
                    "Includes timestamps, monitoring status, and error messages. " +
                    "No screenshots or window titles are included. Known tokens are redacted automatically."
                )
                .font(.footnote)
                .foregroundStyle(VirtueBrand.textMuted)

                if let errorText {
                    Text(errorText)
                        .font(.subheadline)
                        .foregroundStyle(VirtueBrand.danger)
                }

                Button(isSending ? "Sending…" : "Send Report") {
                    send()
                }
                .buttonStyle(VirtueButtonStyle(prominent: true))
                .disabled(isSending || message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
            .padding([.horizontal, .bottom])
        }
        .frame(width: 420)
        .onAppear {
            if contactEmail.isEmpty {
                contactEmail = coordinator.accountEmail ?? ""
            }
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
            dismiss()
            onSent()
        }
    }
}
