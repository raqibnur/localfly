// swift-tools-version: 6.0
import PackageDescription

let package = Package(
    name: "LocalflyCore",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "LocalflyCore", targets: ["LocalflyCore"]),
    ],
    targets: [
        .target(name: "LocalflyCore"),
        .testTarget(name: "LocalflyCoreTests", dependencies: ["LocalflyCore"]),
    ]
)
