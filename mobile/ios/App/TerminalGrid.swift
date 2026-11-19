import UIKit
import CoreText
import PebrelCore

extension UIColor {
    convenience init(rgb: UInt32) {
        self.init(red: CGFloat((rgb >> 16) & 255) / 255, green: CGFloat((rgb >> 8) & 255) / 255,
                  blue: CGFloat(rgb & 255) / 255, alpha: 1)
    }
}

struct GridRow {
    var cells: [PebrelCell]
    var text: [UInt8]

    func string(at cell: PebrelCell) -> String {
        String(decoding: text[cell.start ..< cell.start + cell.length], as: UTF8.self)
    }
}

final class TerminalGrid {
    let handle: OpaquePointer
    private(set) var frame = PebrelFrame()
    private(set) var rows: [GridRow] = []
    private var reply = [UInt8](repeating: 0, count: 65536)

    init?(columns: UInt16 = 48, height: UInt16 = 24, history: UInt16 = 1000) {
        guard let vt = pebrel_vt_new(columns, height, history) else { return nil }
        handle = vt
    }

    deinit { pebrel_vt_free(handle) }

    func apply(_ palette: Palette) {
        let ansi = palette.ansi.map(\.packed)
        _ = pebrel_vt_colors(handle, palette.foreground.packed, palette.background.packed, palette.accent.packed, ansi)
    }

    func feed(_ data: Data) -> Data? {
        let count = reply.count
        let n = data.withUnsafeBytes { bytes in
            pebrel_vt_feed(handle, bytes.bindMemory(to: UInt8.self).baseAddress, bytes.count, &reply, count)
        }
        return n >= 0 ? Data(reply.prefix(Int(n))) : nil
    }

    func snapshot() -> IndexSet {
        guard pebrel_vt_begin(handle, &frame) else { return [] }
        defer { pebrel_vt_end(handle) }
        let width = Int(frame.columns), height = Int(frame.rows)
        if rows.count != height || rows.first?.cells.count != width {
            rows = Array(repeating: GridRow(cells: Array(repeating: PebrelCell(), count: width),
                                            text: Array(repeating: 0, count: width * 4)), count: height)
        }
        var changed = IndexSet()
        for y in 0..<height {
            var dirty = false
            guard pebrel_vt_next_row(handle, &dirty) else { break }
            guard dirty else { continue }
            var row = rows[y]
            let capacity = row.text.count
            var count = pebrel_vt_row(handle, &row.cells, width, &row.text, capacity)
            if count > capacity {
                row.text = Array(repeating: 0, count: count)
                count = pebrel_vt_row(handle, &row.cells, width, &row.text, count)
            }
            guard count >= 0 else { continue }
            rows[y] = row
            changed.insert(y)
        }
        return changed
    }
}

final class TerminalCanvas: UIView {
    let grid: TerminalGrid
    var face = UIFont(name: "JetBrainsMono-Regular", size: 14) ?? .monospacedSystemFont(ofSize: 14, weight: .regular)
    var cellWidth: CGFloat { ("M" as NSString).size(withAttributes: [.font: face]).width.rounded(.up) }
    var cellHeight: CGFloat { ceil(face.lineHeight + 2) }
    let inset: CGFloat = 12
    private var dimensions: CGSize = .zero

    init(grid: TerminalGrid) {
        self.grid = grid
        super.init(frame: .zero)
        isOpaque = true
        contentMode = .redraw
        accessibilityIdentifier = "terminal.grid"
        isAccessibilityElement = true
        refresh()
    }

    required init?(coder: NSCoder) { return nil }

    override func layoutSubviews() {
        super.layoutSubviews()
        let size = CGSize(width: max(1, min(4096, floor((bounds.width - 2 * inset) / cellWidth))),
                          height: max(1, min(4096, floor((bounds.height - 2 * inset) / cellHeight))))
        guard bounds.width > inset * 2, bounds.height > inset * 2, size != dimensions else { return }
        dimensions = size
        _ = pebrel_vt_resize(grid.handle, UInt16(size.width), UInt16(size.height), UInt16(ceil(cellWidth)), UInt16(ceil(cellHeight)))
        refresh()
    }

    func refresh() {
        let previous = Int(grid.frame.cursor_y)
        var changed = grid.snapshot()
        changed.insert(previous)
        changed.insert(Int(grid.frame.cursor_y))
        backgroundColor = UIColor(rgb: grid.frame.background)
        for y in changed { setNeedsDisplay(CGRect(x: 0, y: inset + CGFloat(y) * cellHeight, width: bounds.width, height: cellHeight)) }
        if UIAccessibility.isVoiceOverRunning || ProcessInfo.processInfo.arguments.contains("-capture") {
            accessibilityValue = grid.rows.map { row in row.cells.map { row.string(at: $0) }.joined()
                .trimmingCharacters(in: .whitespaces) }.joined(separator: "\n")
        }
    }

    override func draw(_ rect: CGRect) {
        guard let ctx = UIGraphicsGetCurrentContext() else { return }
        UIColor(rgb: grid.frame.background).setFill()
        ctx.fill(rect)
        let first = max(0, Int(floor((rect.minY - inset) / cellHeight)))
        let end = min(grid.rows.count, Int(ceil((rect.maxY - inset) / cellHeight)))
        guard first < end else { return }
        for y in first..<end {
            let row = grid.rows[y]
            for (x, cell) in row.cells.enumerated() where cell.width > 0 {
                let box = CGRect(x: inset + CGFloat(x) * cellWidth, y: inset + CGFloat(y) * cellHeight,
                                 width: CGFloat(cell.width) * cellWidth, height: cellHeight)
                UIColor(rgb: cell.background).setFill()
                ctx.fill(box)
                if cell.flags & 32 != 0 { continue }
                let string = row.string(at: cell)
                if string == " " && cell.flags & 12 == 0 { continue }
                var traits: UIFontDescriptor.SymbolicTraits = []
                if cell.flags & 1 != 0 { traits.insert(.traitBold) }
                if cell.flags & 2 != 0 { traits.insert(.traitItalic) }
                let font = traits.isEmpty ? face : face.fontDescriptor.withSymbolicTraits(traits).map {
                    UIFont(descriptor: $0, size: face.pointSize)
                } ?? face
                let color = UIColor(rgb: cell.foreground).withAlphaComponent(cell.flags & 16 != 0 ? 0.65 : 1)
                let line = CTLineCreateWithAttributedString(NSAttributedString(string: string,
                    attributes: [.font: font, .foregroundColor: color]))
                ctx.saveGState()
                ctx.translateBy(x: box.minX, y: box.minY + face.ascender + 1)
                ctx.scaleBy(x: 1, y: -1)
                ctx.textMatrix = .identity
                ctx.textPosition = .zero
                CTLineDraw(line, ctx)
                ctx.restoreGState()
                color.setFill()
                if cell.flags & 4 != 0 { ctx.fill(CGRect(x: box.minX, y: box.maxY - 2, width: box.width, height: 1)) }
                if cell.flags & 8 != 0 { ctx.fill(CGRect(x: box.minX, y: box.midY, width: box.width, height: 1)) }
            }
        }
        if grid.frame.cursor_visible {
            let box = CGRect(x: inset + CGFloat(grid.frame.cursor_x) * cellWidth, y: inset + CGFloat(grid.frame.cursor_y) * cellHeight,
                             width: cellWidth, height: cellHeight)
            let color = UIColor(rgb: grid.frame.cursor_color)
            color.setFill()
            switch grid.frame.cursor_style {
            case 0: ctx.fill(CGRect(x: box.minX, y: box.minY, width: 2, height: box.height))
            case 2: ctx.fill(CGRect(x: box.minX, y: box.maxY-2, width: box.width, height: 2))
            case 3: color.setStroke(); ctx.stroke(box.insetBy(dx: 0.5, dy: 0.5))
            default: color.withAlphaComponent(0.55).setFill(); ctx.fill(box)
            }
        }
    }
}
