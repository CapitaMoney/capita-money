// swift-tools-version: 5.9
// SPDX-License-Identifier: AGPL-3.0-only
// The iPhone shell's native parts (docs/adr/0024-hardware-keys-checked-phones.md).
// The Capacitor CLI links this package into CapApp-SPM (`npm run ios`).
import PackageDescription

let package = Package(
    name: "CapitaIos",
    platforms: [.iOS(.v15)],
    products: [
        .library(name: "CapitaIos", targets: ["CapitaIos"])
    ],
    dependencies: [
        .package(url: "https://github.com/ionic-team/capacitor-swift-pm.git", exact: "8.6.2"),
        // MIT; BAC, PACE, chip authentication, active authentication with our challenge
        .package(url: "https://github.com/AndyQ/NFCPassportReader.git", exact: "3.0.0")
    ],
    targets: [
        .target(
            name: "CapitaIos",
            dependencies: [
                .product(name: "Capacitor", package: "capacitor-swift-pm"),
                .product(name: "NFCPassportReader", package: "NFCPassportReader")
            ],
            path: "ios/Sources/CapitaIos"
        )
    ]
)
