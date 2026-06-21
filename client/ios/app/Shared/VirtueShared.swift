import Foundation

enum VirtueShared {
    static let appGroupID = "group.org.virtueinitiative.virtueios"
    static let buildLabel: String = {
        if let buildLabel = Bundle.main.object(forInfoDictionaryKey: "VirtueBuildLabel") as? String {
            return buildLabel
        }
        if let marketingVersion =
            Bundle.main.object(forInfoDictionaryKey: "CFBundleShortVersionString") as? String
        {
            return marketingVersion
        }
        return "0.0.0"
    }()

    static let baseApiUrlKey = "VIRTUE_BASE_API_URL"
    static let captureIntervalKey = "VIRTUE_CAPTURE_INTERVAL_SECONDS"
    static let batchWindowKey = "VIRTUE_BATCH_WINDOW_SECONDS"
    static let monitoringEnabledKey = "VIRTUE_MONITORING_ENABLED"
    static let safariCaptureStateCodeKey = "VIRTUE_SAFARI_CAPTURE_STATE_CODE"
    static let safariPauseStopIssuedKey = "VIRTUE_SAFARI_PAUSE_STOP_ISSUED"

    static let defaultBaseApiUrl = "https://api.virtueinitiative.org"
    static let defaultCaptureIntervalSeconds = "15"
    static let defaultBatchWindowSeconds = "30"
    static let defaultMonitoringEnabled = true

    static let safariLastMessageAtKey = "VIRTUE_SAFARI_LAST_MESSAGE_AT"
    static let safariLastFrameAtKey = "VIRTUE_SAFARI_LAST_FRAME_AT"
    static let safariLastURLKey = "VIRTUE_SAFARI_LAST_URL"
    static let safariLastTitleKey = "VIRTUE_SAFARI_LAST_TITLE"
    static let safariLastErrorKey = "VIRTUE_SAFARI_LAST_ERROR"
    static let safariDaemonRunningKey = "VIRTUE_SAFARI_DAEMON_RUNNING"
    static let safariDaemonLastErrorKey = "VIRTUE_SAFARI_DAEMON_LAST_ERROR"

    static let safariHeartbeatStaleThresholdSeconds: TimeInterval = 10
    static let safariFrameFreshnessThresholdSeconds: TimeInterval = 20

    static let captureStateReady = 0
    static let captureStatePermissionMissing = 1
    static let captureStateSessionUnavailable = 2
    static let captureStateUnknown = 3

    static let brandAccentHex = "#1e3a2e"
}
