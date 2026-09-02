import Foundation

// Wire shapes for the passwordless sign-in flow (CORE-020/CORE-021), for iOS.
//
// This duplicates `VirtueKit`'s `CodeLogin.swift`, which the Mac app uses. The
// iOS target does not link VirtueKit — it carries its own copies of `Card`,
// `SectionLabel`, `DetailRow` and `VirtueBrand` in `VirtueIOSApp.swift` for the
// same reason. Linking VirtueKit here and deleting both sets of duplicates is
// worth doing, but it is a project-graph change that wants a Mac to verify, so
// it is deliberately left out of this change.
//
// The matching view lives in `VirtueIOSApp.swift` beside the other private view
// helpers, because `VirtueButtonStyle` is file-private there.
//
// Both copies decode the exact same JSON, produced by one `#[no_mangle]`
// function per platform; if you change one, change the other.

/// The JSON `virtue_ios_native_begin_code_login` answers with. It reports
/// failure in-band as `error`, so there is one shape to decode either way.
struct CodeLoginStart: Decodable {
    let userCode: String?
    let expiresAtMs: Int64?
    let intervalSeconds: Int?
    let error: String?
}

private struct CodeLoginPollReport: Decodable {
    let status: String?
    let accountEmail: String?
    let error: String?
}

/// What one poll found (CORE-021).
enum CodeLoginPoll {
    case pending
    /// `accountEmail` is the approving account, when the native layer reported
    /// it. The device never learns the email from the user in this flow.
    case approved(accountEmail: String?)
    case expired
    /// CORE-021: the server could not be reached. The pairing is untouched, so
    /// callers keep waiting rather than throwing away the code already on
    /// screen.
    case unavailable
    /// The poll itself failed. Usually transient, so callers keep waiting too.
    case failed(String)
}

/// How long to wait between polls when the server didn't say.
let defaultCodeLoginIntervalSeconds = 5

func decodeCodeLoginStart(_ json: String?) -> Result<CodeLoginStart, String> {
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

func decodeCodeLoginPoll(_ json: String?) -> CodeLoginPoll {
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
    case "unavailable": return .unavailable
    default: return .failed("unexpected poll status: \(payload.status ?? "<none>")")
    }
}
