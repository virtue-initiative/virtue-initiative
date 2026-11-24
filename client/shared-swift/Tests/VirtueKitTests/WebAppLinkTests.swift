import XCTest

@testable import VirtueKit

// CLIENT-034: web app links use the self-hosted server's web app when the
// status names one, and the default web app otherwise.
final class WebAppLinkTests: XCTestCase {
    func testUsesTheDefaultWebAppWithoutOne() {
        XCTAssertEqual(
            WebAppLink.url("/partners", webAppUrl: nil).absoluteString,
            "https://app.virtueinitiative.org/partners")
        XCTAssertEqual(
            WebAppLink.url("/partners", webAppUrl: "").absoluteString,
            "https://app.virtueinitiative.org/partners")
    }

    func testUsesTheServersWebApp() {
        XCTAssertEqual(
            WebAppLink.url("/partners", webAppUrl: "https://virtue.example.org").absoluteString,
            "https://virtue.example.org/partners")
    }

    func testReadsTheWebAppFromStatus() {
        let json = """
            {"is_authenticated":true,"is_running":true,"pending_request_count":0,
             "web_app_url":"https://virtue.example.org"}
            """
        let status = CoreServiceStatus.decode(fromJson: json)
        XCTAssertEqual(status?.webAppUrl, "https://virtue.example.org")
        XCTAssertEqual(
            WebAppLink.url("/partners", status: status).absoluteString,
            "https://virtue.example.org/partners")
    }
}
