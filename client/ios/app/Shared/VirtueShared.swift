import Foundation

@_silgen_name("virtue_ios_default_api_base_url")
private func virtue_ios_default_api_base_url() -> UnsafePointer<CChar>?

@_silgen_name("virtue_ios_default_capture_interval_seconds")
private func virtue_ios_default_capture_interval_seconds() -> UInt64

@_silgen_name("virtue_ios_default_batch_window_seconds")
private func virtue_ios_default_batch_window_seconds() -> UInt64

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

    static let monitoringEnabledKey = "VIRTUE_MONITORING_ENABLED"
    static let accountEmailKey = "VIRTUE_ACCOUNT_EMAIL"
    static let safariCaptureStateCodeKey = "VIRTUE_SAFARI_CAPTURE_STATE_CODE"
    static let safariPauseStopIssuedKey = "VIRTUE_SAFARI_PAUSE_STOP_ISSUED"

    static let defaultBaseApiUrl: String = {
        guard let ptr = virtue_ios_default_api_base_url() else {
            return "https://api.virtueinitiative.org"
        }
        return String(cString: ptr)
    }()
    static let defaultCaptureIntervalSeconds = String(virtue_ios_default_capture_interval_seconds())
    static let defaultBatchWindowSeconds = String(virtue_ios_default_batch_window_seconds())
    static let defaultMonitoringEnabled = true

    static let safariLastMessageAtKey = "VIRTUE_SAFARI_LAST_MESSAGE_AT"
    static let safariLastFrameAtKey = "VIRTUE_SAFARI_LAST_FRAME_AT"
    static let safariLastURLKey = "VIRTUE_SAFARI_LAST_URL"
    static let safariLastTitleKey = "VIRTUE_SAFARI_LAST_TITLE"
    /// Set by the app when the check page reports the extension off.
    static let safariReportedOffAtKey = "VIRTUE_SAFARI_REPORTED_OFF_AT"
    static let safariLastErrorKey = "VIRTUE_SAFARI_LAST_ERROR"
    static let safariDaemonRunningKey = "VIRTUE_SAFARI_DAEMON_RUNNING"
    static let safariDaemonLastErrorKey = "VIRTUE_SAFARI_DAEMON_LAST_ERROR"
    /// Bools reported by the extension on every message, when Safari lets it
    /// check (see `readPermissionState` in background.js). Absent = unknown.
    static let safariAllSitesGrantedKey = "VIRTUE_SAFARI_ALL_SITES_GRANTED"
    static let safariPrivateAllowedKey = "VIRTUE_SAFARI_PRIVATE_ALLOWED"
    /// Set by the app's "Capture Next Safari Page" button; the extension
    /// clears it once it has scheduled the forced tick.
    static let safariForceCaptureRequestedAtKey = "VIRTUE_SAFARI_FORCE_CAPTURE_REQUESTED_AT"

    static let safariHeartbeatStaleThresholdSeconds: TimeInterval = 10
    static let safariFrameFreshnessThresholdSeconds: TimeInterval = 20

    static let captureStateReady = 0
    static let captureStatePermissionMissing = 1
    static let captureStateSessionUnavailable = 2
    static let captureStateUnknown = 3

    static let brandAccentHex = "#1e3a2e"
}
