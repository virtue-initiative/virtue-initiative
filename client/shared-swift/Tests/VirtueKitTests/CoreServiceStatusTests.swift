import XCTest

@testable import VirtueKit

// CORE-010, CLIENT-033: the announcement decodes from status, is absent when
// the core sends none, and an unknown level reads as info.
final class CoreServiceStatusTests: XCTestCase {
    private func decode(_ extra: String) -> CoreServiceStatus? {
        CoreServiceStatus.decode(
            fromJson:
                "{\"is_authenticated\":true,\"is_running\":true,\"pending_request_count\":0\(extra)}"
        )
    }

    func testDecodesTheAnnouncement() {
        XCTAssertNil(decode("")?.announcement)
        XCTAssertNil(decode(",\"announcement\":null")?.announcement)
        XCTAssertEqual(
            decode(
                ",\"announcement\":{\"message\":\"Update\",\"url\":\"https://x.org\",\"level\":\"urgent\"}"
            )?.announcement,
            CoreAnnouncement(message: "Update", url: "https://x.org", level: .urgent)
        )
        XCTAssertEqual(
            decode(",\"announcement\":{\"message\":\"Hi\",\"url\":null,\"level\":\"sparkly\"}")?
                .announcement?.level,
            .info
        )
    }
}
