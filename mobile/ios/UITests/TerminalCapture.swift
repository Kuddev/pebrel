import XCTest

final class TerminalCapture: XCTestCase {
    func testTerminal() throws {
        let app = XCUIApplication()
        app.launchArguments = ["-capture", "-AppleLanguages", "(en)"]
        app.launch()
        let grid = app.descendants(matching: .any)["terminal.grid"].firstMatch
        XCTAssertTrue(grid.waitForExistence(timeout: 15))
        XCTAssertTrue((grid.value as? String)?.contains("Cargo.toml") == true)
        let shot = XCTAttachment(screenshot: app.screenshot())
        shot.name = "terminal-\(ProcessInfo.processInfo.environment["SIMULATOR_DEVICE_NAME"] ?? "iPhone")-MintLight-default-en"
        shot.lifetime = .keepAlways
        add(shot)
    }
}
