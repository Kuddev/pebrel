import XCTest
@testable import PebrelCore

final class PaletteTests: XCTestCase {
    func testChannelsAreUnsignedBytes() throws {
        XCTAssertEqual(try JSONDecoder().decode(RGB.self, from: Data("[18,171,254]".utf8)).packed, 0x12abfe)
        XCTAssertThrowsError(try JSONDecoder().decode(RGB.self, from: Data("[256,0,0]".utf8)))
        XCTAssertThrowsError(try Palette.load(Data("{}".utf8)))
    }
}
