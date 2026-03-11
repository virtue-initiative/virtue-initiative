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

    static let defaultBaseApiUrl = "http://10.7.7.4:8787"
    static let defaultCaptureIntervalSeconds = "15"
    static let defaultBatchWindowSeconds = "30"

    static let safariLastMessageAtKey = "VIRTUE_SAFARI_LAST_MESSAGE_AT"
    static let safariLastFrameAtKey = "VIRTUE_SAFARI_LAST_FRAME_AT"
    static let safariLastURLKey = "VIRTUE_SAFARI_LAST_URL"
    static let safariLastTitleKey = "VIRTUE_SAFARI_LAST_TITLE"
    static let safariLastErrorKey = "VIRTUE_SAFARI_LAST_ERROR"
    static let safariDaemonRunningKey = "VIRTUE_SAFARI_DAEMON_RUNNING"
    static let safariDaemonLastErrorKey = "VIRTUE_SAFARI_DAEMON_LAST_ERROR"

    static let safariHeartbeatStaleThresholdSeconds: TimeInterval = 10
    static let safariFrameFreshnessThresholdSeconds: TimeInterval = 20
}
