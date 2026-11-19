// swift-tools-version: 5.9
import PackageDescription

let package = Package(
    name: "PebrelCore",
    platforms: [.iOS(.v17), .macOS(.v13)],
    products: [.library(name: "PebrelCore", targets: ["PebrelCore"])],
    targets: [.target(name: "PebrelCore"), .testTarget(name: "PebrelCoreTests", dependencies: ["PebrelCore"])]
)
