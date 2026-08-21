#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
XCFRAMEWORK="$TEMP/TinyArcade.xcframework"
TARGET_DIR=${CARGO_TARGET_DIR:-"$ROOT/target/tinyarcade-ios-smoke"}
CARGO=${CARGO:-cargo}

CARGO="$CARGO" CARGO_TARGET_DIR="$TARGET_DIR" \
  "$CRATE/build-xcframework.sh" "$XCFRAMEWORK"

SLICE="$XCFRAMEWORK/ios-arm64_x86_64-simulator"
if grep -R -q 'tinyvm_wasi_host_v1_' "$XCFRAMEWORK"/*/Headers; then
  echo "optional WASI host header leaked into default TinyArcade XCFramework" >&2
  exit 1
fi
if LC_ALL=C grep -a -q 'tinyvm_wasi_host_v1_' \
    "$XCFRAMEWORK/ios-arm64/libagenterm_tinyvm.a"; then
  echo "optional WASI host symbol leaked into default TinyArcade XCFramework" >&2
  exit 1
fi
xcrun --sdk iphonesimulator clang \
  -target arm64-apple-ios14.0-simulator \
  -std=c11 -Wall -Wextra -Werror -fsyntax-only \
  -I "$SLICE/Headers" \
  "$CRATE/tests/ios/header_smoke.c"
xcrun --sdk iphonesimulator swiftc \
  -parse-as-library \
  -D TINYARCADE_EXTERNAL_CARTRIDGES \
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
  -D TINYARCADE_EXTERNAL_CARTRIDGES \
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
xcrun --sdk iphonesimulator swiftc \
  -parse-as-library \
  -D TINYARCADE_EXTERNAL_CARTRIDGES \
  -warnings-as-errors \
  -O \
  -target arm64-apple-ios14.0-simulator \
  -I "$SLICE/Headers" \
  -L "$SLICE" \
  -lagenterm_tinyvm \
  -Xlinker -fatal_warnings \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift" \
  "$CRATE/tests/ios/TinyArcadeHostProfileCatalogSmoke.swift" \
  -o "$TEMP/TinyArcadeHostProfileCatalogSmoke-arm64"
xcrun --sdk iphonesimulator swiftc \
  -parse-as-library \
  -D TINYARCADE_EXTERNAL_CARTRIDGES \
  -warnings-as-errors \
  -O \
  -target arm64-apple-ios14.0-simulator \
  -I "$SLICE/Headers" \
  -L "$SLICE" \
  -lagenterm_tinyvm \
  -framework CryptoKit \
  -Xlinker -fatal_warnings \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift" \
  "$CRATE/tests/ios/TinyArcadeReviewedFlowSmoke.swift" \
  -o "$TEMP/TinyArcadeReviewedFlowSmoke-arm64"
xcrun --sdk iphonesimulator swiftc \
  -parse-as-library \
  -D TINYARCADE_EXTERNAL_CARTRIDGES \
  -warnings-as-errors \
  -O \
  -target arm64-apple-ios14.0-simulator \
  -I "$SLICE/Headers" \
  -L "$SLICE" \
  -lagenterm_tinyvm \
  -Xlinker -fatal_warnings \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift" \
  "$CRATE/tests/ios/TinyArcadeSnapshotStoreSmoke.swift" \
  -o "$TEMP/TinyArcadeSnapshotStoreSmoke-arm64"
xcrun --sdk iphonesimulator swiftc \
  -parse-as-library \
  -D TINYARCADE_EXTERNAL_CARTRIDGES \
  -warnings-as-errors \
  -O \
  -target arm64-apple-ios14.0-simulator \
  -I "$SLICE/Headers" \
  -L "$SLICE" \
  -lagenterm_tinyvm \
  -Xlinker -fatal_warnings \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift" \
  "$CRATE/tests/ios/TinyArcadeReplaySmoke.swift" \
  -o "$TEMP/TinyArcadeReplaySmoke-arm64"
xcrun --sdk iphonesimulator swiftc \
  -parse-as-library \
  -D TINYARCADE_EXTERNAL_CARTRIDGES \
  -warnings-as-errors \
  -O \
  -target arm64-apple-ios14.0-simulator \
  -I "$SLICE/Headers" \
  -L "$SLICE" \
  -lagenterm_tinyvm \
  -Xlinker -fatal_warnings \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift" \
  "$CRATE/tests/ios/TinyArcadePrivateLibrarySmoke.swift" \
  -o "$TEMP/TinyArcadePrivateLibrarySmoke-arm64"
xcrun --sdk iphonesimulator swiftc \
  -parse-as-library \
  -D TINYARCADE_EXTERNAL_CARTRIDGES \
  -warnings-as-errors \
  -O \
  -target arm64-apple-ios14.0-simulator \
  -I "$SLICE/Headers" \
  -L "$SLICE" \
  -lagenterm_tinyvm \
  -Xlinker -fatal_warnings \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift" \
  "$CRATE/tests/ios/TinyArcadeGameSessionSmoke.swift" \
  -o "$TEMP/TinyArcadeGameSessionSmoke-arm64"
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
  "$CRATE/tests/ios/TinyArcadeCompletionSmoke.swift" \
  -o "$TEMP/TinyArcadeCompletionSmoke-arm64"

xcrun vtool -show-build "$TEMP/TinyArcadeSmoke-arm64" | grep -q 'platform IOSSIMULATOR'
xcrun vtool -show-build "$TEMP/TinyArcadeSmoke-x86_64" | grep -q 'platform IOSSIMULATOR'
ARM64_LINKED_BYTES=$(stat -f%z "$TEMP/TinyArcadeSmoke-arm64")
X86_64_LINKED_BYTES=$(stat -f%z "$TEMP/TinyArcadeSmoke-x86_64")
HOST_PROFILE_CATALOG_LINKED_BYTES=$(stat -f%z "$TEMP/TinyArcadeHostProfileCatalogSmoke-arm64")
REPLAY_LINKED_BYTES=$(stat -f%z "$TEMP/TinyArcadeReplaySmoke-arm64")
PRIVATE_LIBRARY_LINKED_BYTES=$(stat -f%z "$TEMP/TinyArcadePrivateLibrarySmoke-arm64")
GAME_SESSION_LINKED_BYTES=$(stat -f%z "$TEMP/TinyArcadeGameSessionSmoke-arm64")
COMPLETION_LINKED_BYTES=$(stat -f%z "$TEMP/TinyArcadeCompletionSmoke-arm64")
# Imported-memory store identity adds the guarded shared path while defined
# memories retain their direct fast path. Keep one explicit 16 KiB product step
# for that standard capability; later increments stay within this ceiling.
# One fixed 16 KiB graduation step funds store-owned cross-instance funcref
# continuations; keep every arm64 consumer under the same explicit ceiling.
# ABI v1.10 adds the app-facing completion owner, process-lifetime domain
# allocator and late-delivery guards. Fund that complete boundary with two
# explicit 16 KiB product steps rather than hiding it in an unbounded ceiling.
MAX_ARM64_LINKED_BYTES=1638400
# x86_64 is a simulator-only compatibility slice. Keep its separate ceiling
# honest instead of weakening the arm64 product-consumer gate.
# Imported-global store identity crosses the next x86_64 linker size bucket;
# imported-table store/address identity and direct linked functions cross two
# more. Keep the simulator compatibility budget explicit without changing the
# arm64 product ceiling.
# The simulator slice crosses three matching 16 KiB linker buckets.
MAX_X86_64_LINKED_BYTES=1736704
test "$ARM64_LINKED_BYTES" -le "$MAX_ARM64_LINKED_BYTES"
test "$X86_64_LINKED_BYTES" -le "$MAX_X86_64_LINKED_BYTES"
test "$HOST_PROFILE_CATALOG_LINKED_BYTES" -le "$MAX_ARM64_LINKED_BYTES"
test "$REPLAY_LINKED_BYTES" -le "$MAX_ARM64_LINKED_BYTES"
test "$PRIVATE_LIBRARY_LINKED_BYTES" -le "$MAX_ARM64_LINKED_BYTES"
test "$GAME_SESSION_LINKED_BYTES" -le "$MAX_ARM64_LINKED_BYTES"
test "$COMPLETION_LINKED_BYTES" -le "$MAX_ARM64_LINKED_BYTES"
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
  COMPLETION_CARTRIDGE="$TEMP/async-completion-0.1.0.wasm"
  "$CRATE/build-depth-well-cartridge.sh" "$DEPTH_CARTRIDGE" >/dev/null
  "$CRATE/build-paddle-guard-cartridge.sh" "$PADDLE_CARTRIDGE" >/dev/null
  "$CRATE/build-async-completion-cartridge.sh" "$COMPLETION_CARTRIDGE" >/dev/null
  xcrun simctl spawn booted "$TEMP/TinyArcadeSmoke-arm64" \
    "$DEPTH_CARTRIDGE" "$PADDLE_CARTRIDGE"
  xcrun simctl spawn booted "$TEMP/TinyArcadeHostProfileCatalogSmoke-arm64"
  xcrun simctl spawn booted "$TEMP/TinyArcadeReviewedFlowSmoke-arm64" \
    "$PADDLE_CARTRIDGE"
  xcrun simctl spawn booted "$TEMP/TinyArcadeSnapshotStoreSmoke-arm64" \
    "$PADDLE_CARTRIDGE"
  xcrun simctl spawn booted "$TEMP/TinyArcadeReplaySmoke-arm64" \
    "$PADDLE_CARTRIDGE"
  xcrun simctl spawn booted "$TEMP/TinyArcadePrivateLibrarySmoke-arm64" \
    "$DEPTH_CARTRIDGE" "$PADDLE_CARTRIDGE"
  xcrun simctl spawn booted "$TEMP/TinyArcadeGameSessionSmoke-arm64" \
    "$PADDLE_CARTRIDGE"
  xcrun simctl spawn booted "$TEMP/TinyArcadeCompletionSmoke-arm64" \
    "$COMPLETION_CARTRIDGE"
fi

echo "OK: iOS device + universal simulator XCFramework and Swift package; links arm64=${ARM64_LINKED_BYTES} x86_64=${X86_64_LINKED_BYTES} profile-catalog=${HOST_PROFILE_CATALOG_LINKED_BYTES} replay=${REPLAY_LINKED_BYTES} private=${PRIVATE_LIBRARY_LINKED_BYTES} session=${GAME_SESSION_LINKED_BYTES} completion=${COMPLETION_LINKED_BYTES} bytes"
