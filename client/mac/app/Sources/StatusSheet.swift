import AppKit
import SwiftUI
import VirtueKit

/// The status page (see `client/core/SPEC.md` CORE-010): the same sections, in
/// the same order, as every other platform's status screen, plus the macOS
/// screen-recording permission and daemon reachability rows.
struct StatusSheet: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        VStack(spacing: 0) {
            HStack {
                Text("Status Details")
                    .font(.headline)
                Spacer()
                Button("Reveal Logs") {
                    NSWorkspace.shared.activateFileViewerSelecting([coordinator.logDirectory])
                }
                Button("Done") {
                    dismiss()
                }
            }
            .padding()

            StatusDetailList(sections: sections)
        }
        .frame(width: 460, height: 520)
    }

    private var sections: [StatusDetailSection] {
        let status = coordinator.coreStatus
        return [
            StatusDetailSection(title: "Account", rows: [
                (label: "Signed in", value: coordinator.loggedIn ? "Yes" : "No"),
                (label: "Email", value: status?.accountEmail ?? coordinator.accountEmail ?? "<none>"),
                (label: "Device name", value: status?.deviceName ?? "<none>"),
                (label: "Partners", value: status?.partnerCount.map(String.init) ?? "<unknown>"),
            ]),
            StatusDetailSection(title: "Queues", rows: [
                (label: "Waiting for hash", value: String(status?.pendingHashCount ?? 0)),
                (label: "Waiting in batch", value: String(status?.pendingBatchCount ?? 0)),
                (label: "Last batch upload", value: coordinator.formatStatusTimestamp(status?.lastBatchAtMs)),
            ]),
            StatusDetailSection(title: "Capture", rows: [
                (label: "Daemon status", value: daemonStatusLabel),
                (label: "Screen recording", value: permissionLabel),
                (label: "Last loop", value: coordinator.formatStatusTimestamp(status?.lastLoopAtMs)),
                (label: "Last attempt", value: coordinator.formatStatusTimestamp(status?.lastScreenshotAttemptAtMs)),
                (label: "Last screenshot", value: coordinator.formatStatusTimestamp(status?.lastScreenshotAtMs)),
                (label: "Last skip reason", value: status?.lastSkipReason?.label ?? "<none>"),
            ]),
            StatusDetailSection(title: "Recent errors", rows: errorRows),
            StatusDetailSection(title: "Advanced", rows: [
                (label: "Device ID", value: coordinator.deviceId),
                (label: "API URL", value: status?.apiBaseUrl ?? "<unknown>"),
                (label: "Hash base URL", value: status?.hashBaseUrl ?? "<default>"),
                (label: "Capture interval", value: status.map { "\($0.captureIntervalSeconds ?? 0)s" } ?? "<unknown>"),
                (label: "Batch window", value: status.map { "\($0.batchWindowSeconds ?? 0)s" } ?? "<unknown>"),
                (label: "Build", value: coordinator.buildLabel),
                (label: "Logs", value: coordinator.logDirectory.path),
            ]),
        ]
    }

    private var errorRows: [(label: String, value: String)] {
        let errors = coordinator.coreStatus?.recentErrors ?? []
        guard !errors.isEmpty else {
            return [(label: "Errors", value: "None")]
        }
        return errors.prefix(5).map { error in
            (
                label: "\(coordinator.formatStatusTimestamp(error.atMs)) · \(error.context)",
                value: error.message
            )
        }
    }

    private var permissionLabel: String {
        switch coordinator.permissionPhase {
        case .needsRequest: return "Not granted"
        case .needsRelaunch: return "Granted — relaunch required"
        case nil: return "Granted"
        }
    }

    private var daemonStatusLabel: String {
        switch coordinator.daemonStatus {
        case .running: return "Running"
        case .stopped: return "Stopped"
        case .unreachable: return "Unreachable (busy)"
        }
    }
}
