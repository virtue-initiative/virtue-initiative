import SwiftUI

@main
struct VirtueIOSApp: App {
    @StateObject private var coordinator = MonitoringCoordinator()

    var body: some Scene {
        WindowGroup {
            ContentView(coordinator: coordinator)
        }
    }
}

struct ContentView: View {
    @ObservedObject var coordinator: MonitoringCoordinator
    @State private var pulse = false

    private var captureActive: Bool {
        coordinator.safariCaptureHealth == "Active in Safari"
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                Text("Virtue iOS")
                    .font(.largeTitle.weight(.bold))
                Text("Build \(VirtueShared.buildLabel)")
                    .font(.footnote)
                    .foregroundStyle(.secondary)

                indicatorPanel

                VStack(alignment: .leading, spacing: 8) {
                    Text("Session")
                        .font(.headline)
                    Text("Logged in: \(coordinator.loggedIn ? "yes" : "no")")
                    Text("Device ID: \(coordinator.deviceId)")
                    Text("Monitoring: \(coordinator.monitorSummary)")
                    Text("Pending Requests: \(coordinator.pendingRequestCount)")
                    Text("API: \(coordinator.currentApiBaseUrl)")
                }

                VStack(alignment: .leading, spacing: 8) {
                    Text("Login")
                        .font(.headline)
                    TextField("Email", text: $coordinator.email)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .textFieldStyle(.roundedBorder)
                    SecureField("Password", text: $coordinator.password)
                        .textFieldStyle(.roundedBorder)
                    HStack {
                        Button("Sign In") {
                            coordinator.login()
                        }
                        .buttonStyle(.borderedProminent)

                        Button("Sign Out") {
                            coordinator.logout()
                        }
                        .buttonStyle(.bordered)
                    }
                }

                VStack(alignment: .leading, spacing: 8) {
                    Text("Safari Capture")
                        .font(.headline)
                    Text("Capture now comes only from the Safari extension. Keep Safari as the browsing surface and enable the Virtue extension for all websites.")
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                    Text("1. Open iOS Settings > Safari > Extensions.")
                        .font(.subheadline)
                    Text("2. Enable Virtue Safari Capture.")
                        .font(.subheadline)
                    Text("3. Allow access: All Websites.")
                        .font(.subheadline)
                    Text("4. Browse in Safari to produce screenshots.")
                        .font(.subheadline)
                }

                VStack(alignment: .leading, spacing: 8) {
                    Text("Runtime Overrides")
                        .font(.headline)
                    TextField("VIRTUE_BASE_API_URL", text: $coordinator.baseApiUrlOverride)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .textFieldStyle(.roundedBorder)
                    TextField(
                        "VIRTUE_CAPTURE_INTERVAL_SECONDS",
                        text: $coordinator.captureIntervalOverride
                    )
                    .keyboardType(.numberPad)
                    .textFieldStyle(.roundedBorder)
                    TextField(
                        "VIRTUE_BATCH_WINDOW_SECONDS",
                        text: $coordinator.batchWindowOverride
                    )
                    .keyboardType(.numberPad)
                    .textFieldStyle(.roundedBorder)
                    Button("Apply Overrides") {
                        coordinator.applyOverrides()
                    }
                    .buttonStyle(.bordered)
                }

                VStack(alignment: .leading, spacing: 4) {
                    Text("Status")
                        .font(.headline)
                    Text(coordinator.statusMessage)
                    Text("Core Last Loop: \(coordinator.lastCoreLoop)")
                        .foregroundStyle(.secondary)
                    Text("Core Last Screenshot: \(coordinator.lastCoreScreenshot)")
                        .foregroundStyle(.secondary)
                    Text("Core Last Batch: \(coordinator.lastCoreBatch)")
                        .foregroundStyle(.secondary)
                    Text("Capture Health: \(coordinator.safariCaptureHealth)")
                        .foregroundStyle(.secondary)
                    Text("Last Heartbeat: \(coordinator.safariLastHeartbeat)")
                        .foregroundStyle(.secondary)
                    Text("Last Frame: \(coordinator.safariLastFrame)")
                        .foregroundStyle(.secondary)
                    Text("Last Page: \(coordinator.safariLastPage)")
                        .foregroundStyle(.secondary)
                    Text("Last Capture Error: \(coordinator.safariLastError)")
                        .foregroundStyle(.secondary)
                    Text("Daemon: \(coordinator.safariDaemonStatus)")
                        .foregroundStyle(.secondary)
                }
            }
            .padding(20)
        }
        .onAppear {
            withAnimation(.easeInOut(duration: 0.85).repeatForever(autoreverses: true)) {
                pulse = true
            }
        }
    }

    private var indicatorPanel: some View {
        HStack(spacing: 10) {
            Circle()
                .fill(captureActive ? Color.red : Color.gray)
                .frame(width: 24, height: 24)
                .scaleEffect(captureActive && pulse ? 1.2 : 0.9)
                .shadow(color: captureActive ? .red.opacity(0.8) : .clear, radius: 8)

            VStack(alignment: .leading, spacing: 2) {
                Text("Safari Monitoring Indicator")
                    .font(.headline)
                Text(captureActive ? "ON (Safari extension heartbeat active)" : "OFF")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
        .padding(12)
        .background(
            RoundedRectangle(cornerRadius: 12)
                .fill(Color(.secondarySystemBackground))
        )
    }
}
