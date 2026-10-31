// swift-tools-version:6.0
import PackageDescription

let package = Package(
    name: "{{name_pascal}}",
    products: [
        .executable(name: "{{name_kebab}}", targets: ["{{name_pascal}}"]),
    ],
    targets: [
        .target(name: "{{name_pascal}}Core"),
        .executableTarget(name: "{{name_pascal}}", dependencies: ["{{name_pascal}}Core"]),
        .testTarget(name: "{{name_pascal}}CoreTests", dependencies: ["{{name_pascal}}Core"]),
    ]
)
