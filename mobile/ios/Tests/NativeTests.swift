import XCTest
@testable import Pebrel

final class NativeTests: XCTestCase {
    func testTerminalCellsKeepColorAndGraphemes() throws {
        let grid = try XCTUnwrap(TerminalGrid())
        XCTAssertNotNil(grid.feed(Data("\u{1b}[38;2;21;67;89m中e\u{301}\u{1b}[0m".utf8)))
        XCTAssertTrue(grid.snapshot().contains(0))
        let row = grid.rows[0]
        XCTAssertEqual(row.string(at: row.cells[0]), "中")
        XCTAssertEqual(row.cells[0].foreground, 0x154359)
        XCTAssertEqual(row.cells[0].width, 2)
        XCTAssertEqual(row.cells[1].width, 0)
        XCTAssertEqual(row.string(at: row.cells[2]), "e\u{301}")
    }

    func testCreateAndCloseSSHSession() {
        var id: Int64 = 0
        let empty = PebrelBytes(data: nil, len: 0)
        let error = pebrel_ssh_create("127.0.0.1", 1, "test", empty, "", empty, &id)
        defer { pebrel_string_free(error) }
        XCTAssertNil(error)
        XCTAssertGreaterThan(id, 0)
        XCTAssertNil(pebrel_ssh_close(id))
    }
}
