// swift-tools-version: 6.0

import PackageDescription

let package = Package(
    name: "TinyArcadeRuntimePackage",
    platforms: [.iOS(.v14)],
    products: [
        .library(name: "TinyArcadeRuntime", targets: ["TinyArcadeRuntime"]),
    ],
    targets: [
        .binaryTarget(
            name: "TinyArcade",
            path: "TinyArcade.xcframework"
        ),
        .target(
            name: "TinyArcadeRuntime",
            dependencies: ["TinyArcade"],
            path: "Sources/TinyArcadeRuntime"
        ),
    ]
)
