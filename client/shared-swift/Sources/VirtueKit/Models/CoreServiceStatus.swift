import Foundation

/// Mirrors `virtue_core::ServiceStatus`, the JSON payload returned by
/// `virtue_*_native_get_status_json()` on both platforms.
public struct CoreServiceStatus: Decodable {
    public let isAuthenticated: Bool
    public let isRunning: Bool
    public let deviceId: String?
    public let lastLoopAtMs: Int64?
    public let pendingRequestCount: Int

    private enum CodingKeys: String, CodingKey {
        case isAuthenticated = "is_authenticated"
        case isRunning = "is_running"
        case deviceId = "device_id"
        case lastLoopAtMs = "last_loop_at_ms"
        case pendingRequestCount = "pending_request_count"
    }

    public static func decode(fromJson json: String) -> CoreServiceStatus? {
        guard let data = json.data(using: .utf8), !data.isEmpty else {
            return nil
        }
        return try? JSONDecoder().decode(CoreServiceStatus.self, from: data)
    }
}
