import SwiftUI

/// "Report an Issue" screen, pushed from the menu. Ported from the Mac/Windows clients' report-issue
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
                TextField("Email", text: $contactEmail)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .keyboardType(.emailAddress)
                    .textContentType(.emailAddress)
            } header: {
                Text("Contact email (optional)")
            }

            Section {
                Toggle("Attach logs from the last two days", isOn: $includeLogs)
            } footer: {
                Text(
                    "Includes timestamps, monitoring status, and error messages. " +
                    "No screenshots or window titles are included. Known tokens are redacted automatically."
                )
            }

            if let errorText {
                Section {
                    Text(errorText)
                        .foregroundStyle(VirtueBrand.bad)
                }
            }

            Section {
                Button(isSending ? "Sending…" : "Send Report") {
                    send()
                }
                .buttonStyle(VirtueButtonStyle(prominent: true, fullWidth: true))
                .disabled(isSending || message.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets())
            }
        }
        .scrollContentBackground(.hidden)
        .background(VirtueBrand.bg)
        .navigationTitle("Report an Issue")
        .navigationBarTitleDisplayMode(.inline)
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
