// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Orbit",
    platforms: [.macOS(.v13)],
    products: [
        .library(name: "OrbitKit", targets: ["OrbitKit"]),
        .executable(name: "orbitctl", targets: ["OrbitCtl"]),
        .executable(
            name: "orbit-server",
            targets: ["OrbitServer"]
        ),
        // .executable(name: "orbit-old", targets: ["OrbitOld"]),
    ],
    dependencies: [
        .package(url: "https://github.com/apple/swift-argument-parser", from: "1.5.0"),
    ],
    targets: [
        .target(name: "OrbitKit"),
        .executableTarget(name: "OrbitCtl", dependencies: ["OrbitKit", .product(name: "ArgumentParser", package: "swift-argument-parser")]),
        .executableTarget(name: "OrbitServer", dependencies: ["OrbitKit"]),
        .testTarget(name: "OrbitKitTests", dependencies: ["OrbitKit"]),
    ]
)
