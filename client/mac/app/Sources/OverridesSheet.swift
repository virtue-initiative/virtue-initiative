import SwiftUI
import VirtueKit

/// Lets a developer/tester point the bundled daemon at a different API base
/// URL or shorten the capture/batch cadence. Written to `config.json`, which
/// the daemon's `ConfigModule` hot-reloads on its next `Ping` — no relaunch
/// needed. Mirrors iOS's `OverridesSheet`.
struct OverridesSheet: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(alignment: .leading, spacing: VirtueSpacing.s4) {
            HStack {
                Text("Runtime Overrides")
                    .font(.headline)
                Spacer()
                Button("Done") {
                    dismiss()
                }
            }

            VStack(alignment: .leading, spacing: VirtueSpacing.s3) {
                labeledField("VIRTUE_BASE_API_URL", text: $coordinator.baseApiUrlOverride)
                labeledField("VIRTUE_CAPTURE_INTERVAL_SECONDS", text: $coordinator.captureIntervalOverride)
                labeledField("VIRTUE_BATCH_WINDOW_SECONDS", text: $coordinator.batchWindowOverride)
            }

            Button("Apply Overrides") {
                coordinator.applyOverrides()
            }
            .buttonStyle(VirtueButtonStyle(prominent: true))

            if let overridesMessage = coordinator.overridesMessage {
                Text(overridesMessage)
                    .font(.subheadline)
                    .foregroundStyle(VirtueBrand.textMuted)
            }

            Spacer(minLength: 0)
        }
        .padding(VirtueSpacing.s5)
        .frame(width: 420, height: 300)
        .background(VirtueBrand.bg)
    }

    private func labeledField(_ label: String, text: Binding<String>) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(label)
                .font(.caption)
                .foregroundStyle(VirtueBrand.textMuted)
            TextField("", text: text)
                .textFieldStyle(.roundedBorder)
        }
    }
}
