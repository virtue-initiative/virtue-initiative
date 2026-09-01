import Foundation

/// The JSON `virtue_{mac,ios}_native_begin_code_login` answers with (CORE-020).
/// Both native calls report failure in-band as `error`, so there is one shape to
/// decode either way.
public struct CodeLoginStart: Decodable, Sendable {
    public let userCode: String?
    public let expiresAtMs: Int64?
    public let intervalSeconds: Int?
    public let error: String?
}

private struct CodeLoginPollReport: Decodable {
    let status: String?
    let accountEmail: String?
    let error: String?
}

/// What one poll found (CORE-021).
public enum CodeLoginPoll: Sendable {
    case pending
    /// `accountEmail` is the approving account, when the native layer reported
    /// it. The device never learns the email from the user in this flow.
    case approved(accountEmail: String?)
    case expired
    /// The poll itself failed. Usually transient, so callers keep waiting
    /// rather than throwing away the code already on screen.
    case failed(String)
}

/// How long to wait between polls when the server didn't say.
public let defaultCodeLoginIntervalSeconds = 5

/// Decodes a `begin_code_login` payload into either the pairing or a message to
/// show. Lives here rather than in either app's `NativeBridge` so the two
/// platforms cannot drift on what a given payload means.
public func decodeCodeLoginStart(_ json: String?) -> Result<CodeLoginStart, String> {
    guard let json,
          let data = json.data(using: .utf8),
          let payload = try? JSONDecoder().decode(CodeLoginStart.self, from: data)
    else {
        return .failure("the native layer returned no result")
    }
    if let error = payload.error {
        return .failure(error)
    }
    guard payload.userCode != nil else {
        return .failure("the native layer returned no code")
    }
    return .success(payload)
}

public func decodeCodeLoginPoll(_ json: String?) -> CodeLoginPoll {
    guard let json,
          let data = json.data(using: .utf8),
          let payload = try? JSONDecoder().decode(CodeLoginPollReport.self, from: data)
    else {
        return .failed("the native layer returned no result")
    }
    if let error = payload.error {
        return .failed(error)
    }
    switch payload.status {
    case "pending": return .pending
    case "approved": return .approved(accountEmail: payload.accountEmail)
    case "expired": return .expired
    default: return .failed("unexpected poll status: \(payload.status ?? "<none>")")
    }
}
