import AppKit
import SwiftUI
import VirtueKit

/// Status Details (CLIENT-007, CLIENT-024): a summary, the sections a user
/// cares about, and every CORE-010 field under Advanced.
struct StatusDetailsView: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @State private var showAdvanced = false

    private var status: CoreServiceStatus? { coordinator.coreStatus }
    private var state: HomeState { coordinator.homeState }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 26) {
                summary
                // Two columns of sections at their natural heights when they
                // fit, one column otherwise.
                ViewThatFits(in: .horizontal) {
                    HStack(alignment: .top, spacing: 24) {
                        VStack(spacing: 26) { thisMac; screenshots }
                            .frame(minWidth: 300, idealWidth: 300, maxWidth: .infinity, alignment: .top)
                        VStack(spacing: 26) { account; problems }
                            .frame(minWidth: 300, idealWidth: 300, maxWidth: .infinity, alignment: .top)
                    }
                    VStack(spacing: 26) { thisMac; screenshots; account; problems }
                }
                advanced
            }
            .frame(maxWidth: 840, alignment: .leading)
            .padding(28)
            .frame(maxWidth: .infinity)
        }
    }

    private var summary: some View {
        VStack(alignment: .leading, spacing: 6) {
            HStack(spacing: 10) {
                StatusDot(kind: state.kind)
                Text(state.headline)
                    .font(.system(size: 22, weight: .semibold))
                    .foregroundStyle(VirtueBrand.text)
            }
            Text(summarySentence)
                .font(.system(size: 14))
                .foregroundStyle(VirtueBrand.textMuted)
                .fixedSize(horizontal: false, vertical: true)
        }
        .accessibilityElement(children: .combine)
    }

    /// "Core initialization failed: …" and "Failed to start background
    /// service: …" are Stopped too, so show the actual cause (CLIENT-024).
    private var stopMessage: String? {
        coordinator.unexpectedStopMessage?.replacingOccurrences(of: "\n\n", with: " ")
    }

    /// Recent errors are kept until logout, so one from weeks ago shouldn't
    /// sit under "Everything is working." Advanced still lists them all.
    private func isRecent(_ error: CoreStatusError) -> Bool {
        let cutoffMs = Int64((Date().timeIntervalSince1970 - 24 * 60 * 60) * 1000)
        return error.atMs >= cutoffMs
    }

    private var summarySentence: String {
        switch state {
        case .needsScreenRecording: return "Allow Screen Recording to start monitoring."
        case .signedOut: return "Log in to start monitoring."
        case .stopped: return stopMessage ?? "The background service is not running."
        case .active: return "Everything is working. Screenshots are being taken and uploaded."
        case .starting:
            return coordinator.daemonStatus == .running
                ? "Waiting for the first screenshot."
                : "Waiting for the background service to start."
        }
    }

    private var thisMac: some View {
        DetailSection("This Mac") {
            if coordinator.needsScreenRecording {
                StatusRow(kind: .bad, label: "Screen Recording", value: "Off", hint: "Allow it so Virtue can take screenshots.")
            } else {
                StatusRow(kind: .good, label: "Screen Recording", value: "On")
            }
            RowDivider()
            if let stopMessage {
                StatusRow(kind: .bad, label: "Background service", value: "Stopped", hint: stopMessage)
            } else if coordinator.daemonStatus == .running {
                StatusRow(kind: .good, label: "Background service", value: "Running")
            } else {
                StatusRow(kind: .warn, label: "Background service", value: "Starting")
            }
        }
    }

    private var screenshots: some View {
        DetailSection("Screenshots") {
            if let last = status?.lastScreenshotAtMs {
                StatusRow(kind: .good, label: "Last screenshot", value: shortTime(last))
            } else if coordinator.loggedIn {
                StatusRow(kind: .warn, label: "Last screenshot", value: "None yet", hint: "The first one is taken within a few minutes.")
            } else {
                StatusRow(kind: .neutral, label: "Last screenshot", value: "None")
            }
            RowDivider()
            if let status {
                let waiting = (status.pendingHashCount ?? 0) + (status.pendingBatchCount ?? 0)
                StatusRow(kind: coordinator.loggedIn ? .good : .neutral, label: "Waiting to upload", value: "\(waiting)")
            } else {
                StatusRow(kind: .neutral, label: "Waiting to upload", value: "Unknown")
            }
        }
    }

    private var account: some View {
        DetailSection("Account") {
            if coordinator.loggedIn {
                StatusRow(kind: .good, label: "Logged in", value: status?.accountEmail ?? coordinator.accountEmail ?? "Yes")
                RowDivider()
                if let count = status?.partnerCount {
                    StatusRow(
                        kind: count > 0 ? .good : .warn,
                        label: "Partners",
                        value: "\(count)",
                        hint: count == 0 ? "Add a partner on the Virtue website so someone receives your reports." : nil
                    )
                } else {
                    StatusRow(kind: .neutral, label: "Partners", value: "Unknown")
                }
            } else {
                StatusRow(kind: .neutral, label: "Logged in", value: "No")
            }
        }
    }

    private var problems: some View {
        DetailSection("Problems") {
            if status == nil {
                StatusRow(kind: .neutral, label: "Recent problems", value: "Unknown")
            } else if let error = status?.recentErrors?.first(where: isRecent) {
                StatusRow(
                    kind: .warn,
                    label: "Recent problems",
                    value: error.message,
                    hint: "\(coordinator.formatStatusTimestamp(error.atMs)) · \(error.context)"
                )
            } else {
                StatusRow(kind: .good, label: "Recent problems", value: "None")
            }
        }
    }

    private var advanced: some View {
        VStack(alignment: .leading, spacing: 8) {
            Button {
                withAnimation(.easeInOut(duration: 0.15)) { showAdvanced.toggle() }
            } label: {
                HStack(spacing: 6) {
                    Text("Advanced")
                        .font(.system(size: 14, weight: .medium))
                    Image(systemName: "chevron.down")
                        .font(.system(size: 11, weight: .semibold))
                        .rotationEffect(.degrees(showAdvanced ? 180 : 0))
                }
                .foregroundStyle(VirtueBrand.textMuted)
                .padding(.vertical, 6)
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .pointingHandCursor()
            .accessibilityLabel(showAdvanced ? "Hide Advanced" : "Show Advanced")

            if showAdvanced {
                RowGroup {
                    ForEach(Array(advancedRows.enumerated()), id: \.offset) { index, row in
                        if index > 0 { RowDivider() }
                        StatusRow(kind: nil, label: row.label, value: row.value)
                    }
                    RowDivider()
                    HStack {
                        Button("Reveal Logs") {
                            NSWorkspace.shared.activateFileViewerSelecting([coordinator.logDirectory])
                        }
                        .buttonStyle(VirtueButtonStyle())
                        Spacer()
                    }
                    .padding(.horizontal, 16)
                    .padding(.vertical, 12)
                }
            }
        }
    }

    private var advancedRows: [(label: String, value: String)] {
        var rows: [(label: String, value: String)] = [
            ("Device name", status?.deviceName ?? "<none>"),
            ("Opens at login", coordinator.opensAtLogin ? "On" : "Off"),
            ("Last upload", coordinator.formatStatusTimestamp(status?.lastBatchAtMs)),
            ("Device ID", coordinator.deviceId),
            ("API URL", status?.apiBaseUrl ?? "<unknown>"),
            ("Hash base URL", status?.hashBaseUrl ?? "<default>"),
            ("Capture interval", status?.captureIntervalSeconds.map { "\($0)s" } ?? "<unknown>"),
            ("Batch window", status?.batchWindowSeconds.map { "\($0)s" } ?? "<unknown>"),
            ("Build", coordinator.buildLabel),
            ("Background service state", daemonStatusLabel),
            ("Waiting for hash", String(status?.pendingHashCount ?? 0)),
            ("Waiting in batch", String(status?.pendingBatchCount ?? 0)),
            ("Pending requests", String(coordinator.pendingRequestCount)),
            ("Last loop", coordinator.formatStatusTimestamp(status?.lastLoopAtMs)),
            ("Last attempt", coordinator.formatStatusTimestamp(status?.lastScreenshotAttemptAtMs)),
            ("Last screenshot", coordinator.formatStatusTimestamp(status?.lastScreenshotAtMs)),
            ("Last skip reason", status?.lastSkipReason?.label ?? "<none>"),
        ]
        let errors = status?.recentErrors ?? []
        if errors.isEmpty {
            rows.append(("Recent errors", "<none>"))
        } else {
            for error in errors {
                rows.append(("\(coordinator.formatStatusTimestamp(error.atMs)) · \(error.context)", error.message))
            }
        }
        if let message = coordinator.unexpectedStopMessage {
            rows.append(("Service message", message.replacingOccurrences(of: "\n\n", with: " ")))
        }
        rows.append(("Logs", coordinator.logDirectory.path))
        return rows
    }

    private var daemonStatusLabel: String {
        switch coordinator.daemonStatus {
        case .running: return "Running"
        case .stopped: return "Stopped"
        case .unreachable: return "Unreachable (busy)"
        }
    }
}

/// A bold title over a rounded group of rows.
private struct DetailSection<Rows: View>: View {
    let title: String
    let rows: Rows

    init(_ title: String, @ViewBuilder rows: () -> Rows) {
        self.title = title
        self.rows = rows()
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title)
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(VirtueBrand.text)
                .accessibilityAddTraits(.isHeader)
            RowGroup {
                rows
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct RowGroup<Content: View>: View {
    @ViewBuilder let content: Content

    var body: some View {
        VStack(spacing: 0) { content }
            .background(VirtueBrand.surface)
            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
            .overlay(
                RoundedRectangle(cornerRadius: 10, style: .continuous)
                    .stroke(VirtueBrand.border, lineWidth: 1)
            )
    }
}

private struct RowDivider: View {
    var body: some View {
        Rectangle()
            .fill(VirtueBrand.border)
            .frame(height: 1)
            .padding(.leading, 16)
    }
}

/// Dot, muted label, right-aligned value, and a hint underneath only when the
/// user must act. Warn values use the link color, bad values red.
private struct StatusRow: View {
    let kind: StatusKind?
    let label: String
    let value: String
    var hint: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 5) {
            // Value beside the label when it fits on one line, otherwise
            // underneath, so long emails, URLs and errors wrap at word
            // boundaries instead of being squeezed into a narrow column.
            ViewThatFits(in: .horizontal) {
                HStack(alignment: .firstTextBaseline, spacing: 10) {
                    labelView
                    Spacer(minLength: 12)
                    valueView.fixedSize()
                }
                VStack(alignment: .leading, spacing: 4) {
                    labelView
                    valueView
                        .padding(.leading, indent)
                        .fixedSize(horizontal: false, vertical: true)
                }
            }
            if let hint {
                Text(hint)
                    .font(.system(size: 12))
                    .foregroundStyle(VirtueBrand.textMuted)
                    .padding(.leading, indent)
                    .fixedSize(horizontal: false, vertical: true)
            }
        }
        .font(.system(size: 13.5))
        .padding(.horizontal, 16)
        .padding(.vertical, 14)
        .frame(maxWidth: .infinity, alignment: .leading)
        .accessibilityElement(children: .combine)
    }

    private var indent: CGFloat { kind == nil ? 0 : 19 }

    private var labelView: some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            if let kind {
                StatusDot(kind: kind)
                    .alignmentGuide(.firstTextBaseline) { $0[VerticalAlignment.center] + 4 }
            }
            Text(label)
                .foregroundStyle(VirtueBrand.textMuted)
        }
    }

    private var valueView: some View {
        Text(value)
            .fontWeight(.medium)
            .foregroundStyle(valueColor)
            .textSelection(.enabled)
    }

    private var valueColor: Color {
        switch kind {
        case .warn: return VirtueBrand.link
        case .bad: return VirtueBrand.bad
        default: return VirtueBrand.text
        }
    }
}
