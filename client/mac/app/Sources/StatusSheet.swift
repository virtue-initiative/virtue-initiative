import SwiftUI
import VirtueKit

struct StatusSheet: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Status Details")
                    .font(.headline)
                Spacer()
                Button("Done") {
                    dismiss()
                }
            }
            .padding()

            StatusDetailList(sections: [
                StatusDetailSection(title: "Service", rows: [
                    (label: "Logged in", value: coordinator.loggedIn ? "true" : "false"),
                    (label: "Daemon status", value: daemonStatusLabel),
                    (label: "Pending requests", value: "\(coordinator.pendingRequestCount)"),
                    (label: "Device ID", value: coordinator.deviceId),
                ]),
                StatusDetailSection(title: "Timing", rows: [
                    (label: "Last loop", value: coordinator.lastLoopAt)
                ]),
            ])
        }
        .frame(width: 380, height: 300)
    }

    private var daemonStatusLabel: String {
        switch coordinator.daemonStatus {
        case .running: return "Running"
        case .stopped: return "Stopped"
        case .unreachable: return "Unreachable (busy)"
        }
    }
}
