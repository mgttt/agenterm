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

SLICE="$XCFRAMEWORK/ios-arm64-simulator"
xcrun --sdk iphonesimulator clang \
  -target arm64-apple-ios14.0-simulator \
  -std=c11 -Wall -Wextra -Werror -fsyntax-only \
  -I "$SLICE/Headers" \
  "$CRATE/tests/ios/header_smoke.c"
xcrun --sdk iphonesimulator swiftc \
  -parse-as-library \
  -O \
  -target arm64-apple-ios14.0-simulator \
  -I "$SLICE/Headers" \
  -L "$SLICE" \
  -lagenterm_tinyvm \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift" \
  "$CRATE/tests/ios/TinyArcadeSmoke.swift" \
  -o "$TEMP/TinyArcadeSmoke"

xcrun vtool -show-build "$TEMP/TinyArcadeSmoke" | grep -q 'platform IOSSIMULATOR'
LINKED_BYTES=$(stat -f%z "$TEMP/TinyArcadeSmoke")
test "$LINKED_BYTES" -le 1048576
test -f "$XCFRAMEWORK/ios-arm64/libagenterm_tinyvm.a"
test -f "$XCFRAMEWORK/ios-arm64-simulator/libagenterm_tinyvm.a"
test -f "$XCFRAMEWORK/ios-arm64/Headers/tinyarcade.h"
test -f "$XCFRAMEWORK/ios-arm64-simulator/Headers/module.modulemap"

echo "OK: iOS device + simulator XCFramework; Swift simulator link ${LINKED_BYTES} bytes"
