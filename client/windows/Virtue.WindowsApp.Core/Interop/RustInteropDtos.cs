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

public sealed record RuntimeConfigPayload(
    string ApiBaseUrl,
    int CaptureIntervalSeconds,
    int BatchWindowSeconds,
    string ConfigPath,
    string BuildLabel);

public sealed record RuntimeConfigUpdate(
    string? ApiBaseUrl,
    int? CaptureIntervalSeconds,
    int? BatchWindowSeconds);

internal sealed record LoginRequest(
    string Email,
    string Password,
    string? DeviceName);
