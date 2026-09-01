namespace Virtue.WindowsApp.Core.Interop;

public sealed record SessionStatusPayload(
    bool LoggedIn,
    string? DeviceId,
    string? Email,
    string BuildLabel);

public sealed record StatusErrorPayload(
    long AtMs,
    string Context,
    string Message);

/// <summary>
/// The shared status-page payload (client/core/SPEC.md CORE-010) plus the
/// Windows-only monitor state, last error, and log directory. The new members
/// are optional so the existing positional constructions (tests, defaults)
/// keep working; the Rust side always sends every key.
/// </summary>
public sealed record MonitorStatusPayload(
    string State,
    bool LoggedIn,
    int PendingRequestCount,
    long? LastScreenshotAtMs,
    string? LastError,
    string? AccountEmail = null,
    string? DeviceId = null,
    string? DeviceName = null,
    int? PartnerCount = null,
    int PendingHashCount = 0,
    int PendingBatchCount = 0,
    long? LastLoopAtMs = null,
    long? LastScreenshotAttemptAtMs = null,
    string? LastSkipReason = null,
    long? LastBatchAtMs = null,
    IReadOnlyList<StatusErrorPayload>? RecentErrors = null,
    string? ApiBaseUrl = null,
    string? HashBaseUrl = null,
    long? CaptureIntervalSeconds = null,
    long? BatchWindowSeconds = null,
    string? LogDirectory = null);

/// <summary>
/// What a "Test Screenshot" run actually did, once its batch has had a chance
/// to reach the server. <c>Outcome</c> is the stable code
/// (<c>uploaded</c>/<c>not_captured</c>/<c>pending</c>); <c>Message</c> is the
/// wording every platform shows, from <c>virtue_core::force_capture</c>.
/// </summary>
public sealed record ForceCapturePayload(
    string Outcome,
    string Message);

/// <summary>
/// CORE-020: the code to show the user, and how to pace the polling.
/// </summary>
public sealed record BeginCodeLoginPayload(
    string UserCode,
    long ExpiresAtMs,
    int IntervalSeconds);

/// <summary>
/// CORE-021: <c>Status</c> is <c>pending</c>, <c>approved</c>, or
/// <c>expired</c>. <c>DeviceId</c> is set only when approved.
/// </summary>
public sealed record PollCodeLoginPayload(
    string Status,
    string? DeviceId);

internal sealed record BeginCodeLoginRequest(
    string? DeviceName);

internal sealed record LoginRequest(
    string Email,
    string Password,
    string? DeviceName);

internal sealed record ReportIssueRequest(
    string Message,
    string? ContactEmail,
    bool IncludeLogs);
