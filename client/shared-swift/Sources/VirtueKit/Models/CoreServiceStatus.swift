import Foundation

/// One entry in the daemon's recent-errors ring — `virtue_core::StatusError`.
public struct CoreStatusError: Decodable {
    public let atMs: Int64
    public let context: String
    public let message: String

    private enum CodingKeys: String, CodingKey {
        case atMs = "at_ms"
        case context
        case message
    }
}

/// Why the most recent capture attempt produced no screenshot —
/// `virtue_core::StatusSkipReason`. `unknown` covers a value written by a
/// newer core than this app was built against.
public enum CoreSkipReason: String, Decodable {
    case staticScreen = "static_screen"
    case lockedOrScreensaver = "locked_or_screensaver"
    case captureFailed = "capture_failed"
    case unknown

    public init(from decoder: Decoder) throws {
        let raw = try decoder.singleValueContainer().decode(String.self)
        self = CoreSkipReason(rawValue: raw) ?? .unknown
    }

    public var label: String {
        switch self {
        case .staticScreen: return "Screen unchanged since the last upload"
        case .lockedOrScreensaver: return "Screen locked or screensaver active"
        case .captureFailed: return "Capture failed"
        case .unknown: return "Unknown"
        }
    }
}

/// Mirrors `virtue_core::ServiceStatus`, the JSON payload returned by
/// `virtue_*_native_get_status_json()` on both platforms. This is the shared
/// status-page contract — see `client/core/SPEC.md` CORE-010.
///
/// Every field added after the original five is optional here, so an app
/// talking to an older core still decodes.
public struct CoreServiceStatus: Decodable {
    public let isAuthenticated: Bool
    public let isRunning: Bool
    public let accountEmail: String?
    public let deviceId: String?
    public let deviceName: String?
    public let partnerCount: Int?
    public let pendingHashCount: Int?
    public let pendingBatchCount: Int?
    public let pendingRequestCount: Int
    public let lastLoopAtMs: Int64?
    public let lastScreenshotAttemptAtMs: Int64?
    public let lastScreenshotAtMs: Int64?
    public let lastSkipReason: CoreSkipReason?
    public let lastBatchAtMs: Int64?
    public let recentErrors: [CoreStatusError]?
    public let apiBaseUrl: String?
    /// The self-hosted server's web app, when the device is signed in to one
    /// that named it (CORE-028).
    public let webAppUrl: String?
    public let hashBaseUrl: String?
    public let captureIntervalSeconds: Int64?
    public let batchWindowSeconds: Int64?

    private enum CodingKeys: String, CodingKey {
        case isAuthenticated = "is_authenticated"
        case isRunning = "is_running"
        case accountEmail = "account_email"
        case deviceId = "device_id"
        case deviceName = "device_name"
        case partnerCount = "partner_count"
        case pendingHashCount = "pending_hash_count"
        case pendingBatchCount = "pending_batch_count"
        case pendingRequestCount = "pending_request_count"
        case lastLoopAtMs = "last_loop_at_ms"
        case lastScreenshotAttemptAtMs = "last_screenshot_attempt_at_ms"
        case lastScreenshotAtMs = "last_screenshot_at_ms"
        case lastSkipReason = "last_skip_reason"
        case lastBatchAtMs = "last_batch_at_ms"
        case recentErrors = "recent_errors"
        case apiBaseUrl = "api_base_url"
        case webAppUrl = "web_app_url"
        case hashBaseUrl = "hash_base_url"
        case captureIntervalSeconds = "capture_interval_seconds"
        case batchWindowSeconds = "batch_window_seconds"
    }

    public static func decode(fromJson json: String) -> CoreServiceStatus? {
        guard let data = json.data(using: .utf8), !data.isEmpty else {
            return nil
        }
        return try? JSONDecoder().decode(CoreServiceStatus.self, from: data)
    }
}

/// Which server the sign-in form signs in to (CLIENT-034).
public enum ServerChoice: String, Hashable, CaseIterable {
    case virtueInitiative
    case selfHosted

    public var label: String {
        switch self {
        case .virtueInitiative: return "Virtue Initiative"
        case .selfHosted: return "Self-hosted"
        }
    }
}

/// Links into the web app (CLIENT-034): the default server's, or the web app
/// a self-hosted server named.
public enum WebAppLink {
    public static let defaultBase = "https://app.virtueinitiative.org"

    /// `path` (e.g. "/partners") on the web app for `status`.
    public static func url(_ path: String, status: CoreServiceStatus?) -> URL {
        url(path, webAppUrl: status?.webAppUrl)
    }

    public static func url(_ path: String, webAppUrl: String?) -> URL {
        let base = webAppUrl.flatMap { $0.isEmpty ? nil : $0 } ?? defaultBase
        return URL(string: base + path) ?? URL(string: defaultBase + path)!
    }
}
