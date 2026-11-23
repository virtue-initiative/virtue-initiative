import Foundation

/// How Status Details writes a timestamp (CLIENT-007): local time, then a
/// relative age in parentheses. The age wording is the same on every
/// platform.
public enum StatusTimestamp {
    /// `Ns ago`, `Nm ago`, `Nh Mm ago` or `Nd Nh ago`, rounded down, without
    /// a second unit of zero, or `in the future`.
    public static func age(ofMs timestampMs: Int64, nowMs: Int64) -> String {
        let deltaMs = nowMs - timestampMs
        guard deltaMs >= 0 else { return "in the future" }
        let secs = deltaMs / 1000
        let major: String
        let minor: Int64
        let minorUnit: String
        switch secs {
        case ..<60: return "\(secs)s ago"
        case ..<3600: return "\(secs / 60)m ago"
        case ..<86_400:
            major = "\(secs / 3600)h"
            minor = secs % 3600 / 60
            minorUnit = "m"
        default:
            major = "\(secs / 86_400)d"
            minor = secs % 86_400 / 3600
            minorUnit = "h"
        }
        return minor == 0 ? "\(major) ago" : "\(major) \(minor)\(minorUnit) ago"
    }

    /// `2026-11-11 19:32:05 (5m ago)`, or `<none>` for an absent value.
    public static func full(_ timestampMs: Int64?, now: Date = Date()) -> String {
        guard let timestampMs else { return "<none>" }
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd HH:mm:ss"
        return "\(formatter.string(from: date(timestampMs))) (\(age(timestampMs, now)))"
    }

    /// For a summary row: `7:32 PM (5m ago)`, with the date in front when it
    /// isn't today.
    public static func short(_ timestampMs: Int64, now: Date = Date()) -> String {
        let date = date(timestampMs)
        let formatter = DateFormatter()
        formatter.dateStyle = Calendar.current.isDate(date, inSameDayAs: now) ? .none : .medium
        formatter.timeStyle = .short
        return "\(formatter.string(from: date)) (\(age(timestampMs, now)))"
    }

    private static func date(_ timestampMs: Int64) -> Date {
        Date(timeIntervalSince1970: TimeInterval(timestampMs) / 1000)
    }

    private static func age(_ timestampMs: Int64, _ now: Date) -> String {
        age(ofMs: timestampMs, nowMs: Int64((now.timeIntervalSince1970 * 1000).rounded(.down)))
    }
}
