import XCTest

@testable import VirtueKit

// CLIENT-007: timestamps show local time and a relative age that reads Ns, Nm,
// Nh Mm or Nd Nh ago, and absent values show <none>.
final class StatusTimestampTests: XCTestCase {
    private let second: Int64 = 1000
    private let minute: Int64 = 60 * 1000
    private let hour: Int64 = 60 * 60 * 1000
    private let day: Int64 = 24 * 60 * 60 * 1000

    func testFormatsRelativeAges() {
        let cases: [(Int64, String)] = [
            (-1, "in the future"),
            (0, "0s ago"),
            (59 * second + 999, "59s ago"),
            (minute, "1m ago"),
            (59 * minute + 59 * second, "59m ago"),
            (hour, "1h ago"),
            (4 * hour + 54 * minute, "4h 54m ago"),
            (23 * hour + 59 * minute + 59 * second, "23h 59m ago"),
            (day, "1d ago"),
            (2 * day + 3 * hour + 30 * minute, "2d 3h ago"),
            (40 * day + 10 * hour, "40d 10h ago"),
        ]
        let nowMs: Int64 = 1_800_000_000_000
        for (deltaMs, expected) in cases {
            XCTAssertEqual(
                StatusTimestamp.age(ofMs: nowMs - deltaMs, nowMs: nowMs), expected,
                "\(deltaMs) ms")
        }
    }

    func testFullTimestampHasLocalTimeAndAge() {
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        let then = Date(timeIntervalSince1970: 1_800_000_000 - 5 * 60)
        let formatter = DateFormatter()
        formatter.locale = Locale(identifier: "en_US_POSIX")
        formatter.dateFormat = "yyyy-MM-dd HH:mm:ss"
        XCTAssertEqual(
            StatusTimestamp.full(Int64(then.timeIntervalSince1970) * 1000, now: now),
            "\(formatter.string(from: then)) (5m ago)")
    }

    func testShortTimestampEndsWithTheAge() {
        let now = Date(timeIntervalSince1970: 1_800_000_000)
        let thenMs = Int64(1_800_000_000 - 4 * 3600 - 54 * 60) * 1000
        XCTAssertTrue(StatusTimestamp.short(thenMs, now: now).hasSuffix(" (4h 54m ago)"))
    }

    func testAbsentTimestampShowsNone() {
        XCTAssertEqual(StatusTimestamp.full(nil), "<none>")
    }
}
