namespace Virtue.WindowsApp.Core.Interop;

public sealed record SessionStatusPayload(
    bool LoggedIn,
    string? DeviceId,
    string? Email,
    string BuildLabel);

public sealed record MonitorStatusPayload(
    string State,
    bool LoggedIn,
    int PendingRequestCount,
    long? LastScreenshotAtMs,
    string? LastError);

internal sealed record LoginRequest(
    string Email,
    string Password,
    string? DeviceName);

internal sealed record ReportIssueRequest(
    string Message,
    string? ContactEmail,
    bool IncludeLogs);
