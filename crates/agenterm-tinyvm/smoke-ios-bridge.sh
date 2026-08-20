#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
XCFRAMEWORK="$TEMP/TinyArcade.xcframework"
TARGET_DIR=${CARGO_TARGET_DIR:-"$ROOT/target/tinyarcade-ios-smoke"}
CARGO=${CARGO:-cargo}

CARGO="$CARGO" CARGO_TARGET_DIR="$TARGET_DIR" \
  "$CRATE/build-xcframework.sh" "$XCFRAMEWORK"

SLICE="$XCFRAMEWORK/ios-arm64_x86_64-simulator"
xcrun --sdk iphonesimulator clang \
  -target arm64-apple-ios14.0-simulator \
  -std=c11 -Wall -Wextra -Werror -fsyntax-only \
  -I "$SLICE/Headers" \
  "$CRATE/tests/ios/header_smoke.c"
xcrun --sdk iphonesimulator swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -target arm64-apple-ios14.0-simulator \
  -I "$SLICE/Headers" \
  -L "$SLICE" \
  -lagenterm_tinyvm \
  -Xlinker -fatal_warnings \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift" \
  "$CRATE/tests/ios/TinyArcadeSmoke.swift" \
  -o "$TEMP/TinyArcadeSmoke-arm64"
xcrun --sdk iphonesimulator swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -target x86_64-apple-ios14.0-simulator \
  -I "$SLICE/Headers" \
  -L "$SLICE" \
  -lagenterm_tinyvm \
  -Xlinker -fatal_warnings \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift" \
  "$CRATE/tests/ios/TinyArcadeSmoke.swift" \
  -o "$TEMP/TinyArcadeSmoke-x86_64"

xcrun vtool -show-build "$TEMP/TinyArcadeSmoke-arm64" | grep -q 'platform IOSSIMULATOR'
xcrun vtool -show-build "$TEMP/TinyArcadeSmoke-x86_64" | grep -q 'platform IOSSIMULATOR'
ARM64_LINKED_BYTES=$(stat -f%z "$TEMP/TinyArcadeSmoke-arm64")
X86_64_LINKED_BYTES=$(stat -f%z "$TEMP/TinyArcadeSmoke-x86_64")
MAX_LINKED_BYTES=1310720
test "$ARM64_LINKED_BYTES" -le "$MAX_LINKED_BYTES"
test "$X86_64_LINKED_BYTES" -le "$MAX_LINKED_BYTES"
test -f "$XCFRAMEWORK/ios-arm64/libagenterm_tinyvm.a"
test -f "$XCFRAMEWORK/ios-arm64_x86_64-simulator/libagenterm_tinyvm.a"
test -f "$XCFRAMEWORK/ios-arm64/Headers/tinyarcade.h"
test -f "$XCFRAMEWORK/ios-arm64_x86_64-simulator/Headers/module.modulemap"

PACKAGE="$TEMP/TinyArcadeRuntimePackage"
CARGO="$CARGO" CARGO_TARGET_DIR="$TARGET_DIR" \
  "$CRATE/build-swift-package.sh" "$PACKAGE" >/dev/null
swift package --package-path "$PACKAGE" dump-package >/dev/null
(
  cd "$PACKAGE"
  xcodebuild -quiet -scheme TinyArcadeRuntimePackage \
    -destination 'generic/platform=iOS Simulator' \
    -derivedDataPath "$TEMP/swift-package-simulator" \
    CODE_SIGNING_ALLOWED=NO build
  xcodebuild -quiet -scheme TinyArcadeRuntimePackage \
    -destination 'generic/platform=iOS' \
    -derivedDataPath "$TEMP/swift-package-device" \
    CODE_SIGNING_ALLOWED=NO build
)

if [ "${TINYARCADE_RUN_BOOTED_SIMULATOR:-0}" = 1 ]; then
  DEPTH_CARTRIDGE="$TEMP/depth-well-0.1.0.wasm"
  PADDLE_CARTRIDGE="$TEMP/paddle-guard-0.1.0.wasm"
  "$CRATE/build-depth-well-cartridge.sh" "$DEPTH_CARTRIDGE" >/dev/null
  "$CRATE/build-paddle-guard-cartridge.sh" "$PADDLE_CARTRIDGE" >/dev/null
  xcrun simctl spawn booted "$TEMP/TinyArcadeSmoke-arm64" \
    "$DEPTH_CARTRIDGE" "$PADDLE_CARTRIDGE"
fi

echo "OK: iOS device + universal simulator XCFramework and Swift package; links arm64=${ARM64_LINKED_BYTES} x86_64=${X86_64_LINKED_BYTES} bytes"
