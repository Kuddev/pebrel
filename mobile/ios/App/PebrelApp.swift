import SwiftUI
import PebrelCore

@main
struct PebrelApp: App {
    var body: some Scene {
        WindowGroup {
            NavigationStack {
                DemoTerminal()
                    .navigationTitle("atlas")
                    .navigationBarTitleDisplayMode(.inline)
            }
            .preferredColorScheme(.light)
        }
    }
}

struct DemoTerminal: UIViewRepresentable {
    func makeUIView(context: Context) -> TerminalCanvas {
        // Both resources are mandatory products of the validated asset build.
        let palettes = try! Palette.load(Data(contentsOf: Bundle.main.url(forResource: "themes", withExtension: "json")!))
        let grid = TerminalGrid()!
        grid.apply(palettes["MintLight"]!)
        let data = try! Data(contentsOf: Bundle.main.url(forResource: "Session", withExtension: "json")!)
        for line in try! JSONDecoder().decode([String].self, from: data) {
            _ = grid.feed(Data(line.utf8))
        }
        return TerminalCanvas(grid: grid)
    }
    func updateUIView(_ uiView: TerminalCanvas, context: Context) {}
}
