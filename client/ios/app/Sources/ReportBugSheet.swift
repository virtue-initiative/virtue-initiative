import SwiftUI

/// "Report a Bug" form, ported from the Mac/Windows clients' report-issue
/// dialogs: a message box, an optional contact email (pre-filled when signed
/// in), and an opt-out "include logs" toggle with the same disclosure text.
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
        NavigationStack {
            Form {
                Section {
                    TextEditor(text: $message)
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
                } header: {
                    Text("What happened?")
                }

                Section {
                    TextField("Contact email (optional)", text: $contactEmail)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .keyboardType(.emailAddress)
                }

                Section {
                    Toggle("Include the last two days of diagnostic logs", isOn: $includeLogs)
                } footer: {
                    Text(
                        "Includes timestamps, monitoring status, and error messages. " +
                        "No screenshots or window titles are included. Known tokens are redacted automatically."
                    )
                }

                if let errorText {
                    Section {
                        Text(errorText)
                            .foregroundStyle(Color.red)
                    }
                }
            }
            .navigationTitle("Report a Bug")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        dismiss()
                    }
                    .disabled(isSending)
                }
                ToolbarItem(placement: .confirmationAction) {
                    Button(isSending ? "Sending…" : "Send") {
                        send()
                    }
                    .disabled(isSending || message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                }
            }
        }
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
