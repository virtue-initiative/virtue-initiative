// swift-tools-version:5.7
import PackageDescription

let package = Package(
    name: "VirtueKit",
    platforms: [
        .macOS(.v13),
        .iOS(.v16),
    ],
    products: [
        .library(name: "VirtueKit", targets: ["VirtueKit"])
    ],
    targets: [
        .target(name: "VirtueKit")
    ]
)
