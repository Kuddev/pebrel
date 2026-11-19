import Foundation

public struct RGB: Decodable, Equatable {
    public let packed: UInt32

    public init(from decoder: Decoder) throws {
        var components = try decoder.unkeyedContainer()
        let red = try components.decode(UInt8.self)
        let green = try components.decode(UInt8.self)
        let blue = try components.decode(UInt8.self)
        packed = UInt32(red) << 16 | UInt32(green) << 8 | UInt32(blue)
    }
}

public struct Palette: Decodable {
    public let background, foreground, muted, accent, selected, line, red: RGB
    public let ansi: [RGB]

    public static func load(_ data: Data) throws -> [String: Palette] {
        let catalog = try JSONDecoder().decode([String: Palette].self, from: data)
        guard catalog["MintLight"] != nil, catalog.values.allSatisfy({ $0.ansi.count == 16 }) else {
            throw CocoaError(.fileReadCorruptFile)
        }
        return catalog
    }
}
