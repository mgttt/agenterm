#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
XCFRAMEWORK="$TEMP/TinyWasiHost.xcframework"
TARGET_DIR=${CARGO_TARGET_DIR:-"$ROOT/target/tinyvm-ios-wasi-host-smoke"}
CARGO=${CARGO:-cargo}

command -v wat2wasm >/dev/null
wat2wasm "$CRATE/tests/ios/WasiHostFixture.wat" -o "$TEMP/WasiHostFixture.wasm"
CARGO="$CARGO" CARGO_TARGET_DIR="$TARGET_DIR" \
  "$CRATE/build-wasi-host-xcframework.sh" "$XCFRAMEWORK" >/dev/null

SLICE="$XCFRAMEWORK/ios-arm64_x86_64-simulator"
xcrun --sdk iphonesimulator clang \
  -target arm64-apple-ios14.0-simulator \
  -std=c11 -Wall -Wextra -Werror -fsyntax-only \
  -I "$SLICE/Headers" \
  "$CRATE/tests/ios/wasi_host_header_smoke.c"
xcrun --sdk iphonesimulator swiftc \
  -parse-as-library -warnings-as-errors -O \
  -target arm64-apple-ios14.0-simulator \
  -I "$SLICE/Headers" -L "$SLICE" -lagenterm_tinyvm \
  -Xlinker -fatal_warnings \
  "$CRATE/tests/ios/TinyWasiHostSmoke.swift" \
  -o "$TEMP/TinyWasiHostSmoke-arm64"
xcrun vtool -show-build "$TEMP/TinyWasiHostSmoke-arm64" | grep -q 'platform IOSSIMULATOR'
xcrun simctl spawn booted "$TEMP/TinyWasiHostSmoke-arm64" "$TEMP/WasiHostFixture.wasm"

echo "OK: optional iOS WASI host XCFramework, C header, Swift link and booted-container run"
