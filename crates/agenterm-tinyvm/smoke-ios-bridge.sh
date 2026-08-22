#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
XCFRAMEWORK="$TEMP/TinyArcade.xcframework"
TARGET_DIR=${CARGO_TARGET_DIR:-"$ROOT/target/tinyarcade-ios-smoke"}
CARGO=${CARGO:-cargo}
RUST_FEATURES=${TINYVM_XCFRAMEWORK_FEATURES:-ios-c-api}

grep -Fq 'var rgba = Data(count: pixels.count * 4)' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"
if grep -Fq 'bytes.reserveCapacity(pixels.count * 4)' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"; then
  echo "indexed2d RGBA expansion must not allocate an Array before Data" >&2
  exit 1
fi
grep -Fq 'static let maximumCachedWaveCount = 8' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"
grep -Fq 'static let maximumCachedWaveBytes = 512 * 1_024' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"
grep -Fq 'var wave = Data(count: 44 + pcmBytes)' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"
if grep -Fq 'var pcm = Data(' "$CRATE/bindings/swift/TinyArcadeRuntime.swift"; then
  echo "tone synthesis must write the final WAV without a full PCM staging buffer" >&2
  exit 1
fi
grep -Fq '@preconcurrency import GameController' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"
grep -Fq 'public final class TinyArcadeAppleInputV1' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"
grep -Fq 'controller.handlerQueue = .main' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"
grep -Fq 'keyboard.handlerQueue = .main' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"
grep -Fq 'sourceHandler(binding.source, [])' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"
grep -Fq 'pressedKeyboardAliases: UInt16' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"
if grep -Fq 'Set<GCKeyCode>' "$CRATE/bindings/swift/TinyArcadeRuntime.swift"; then
  echo "bounded keyboard aliases must not require a generic hash set" >&2
  exit 1
fi
grep -Fq 'data.reserveCapacity(envelopeLength)' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"
grep -Fq '.snapshot-v1.prepared"' "$CRATE/bindings/swift/TinyArcadeRuntime.swift"
grep -Fq 'try removePreparedFileIfPresent(temporaryURL)' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"
grep -Fq 'snapshot: data[(32 + idLength)..<data.count]' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"
grep -Fq 'options: .usingNewMetadataOnly' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"
if grep -Fq 'data.write(to: url, options: .atomic)' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"; then
  echo "snapshot protection must be applied before atomic publication" >&2
  exit 1
fi
if grep -Fq 'Array(gameID.utf8)' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"; then
  echo "snapshot envelope must append the bounded UTF-8 view without staging an Array" >&2
  exit 1
fi
if grep -Fq '.snapshot-v1.\(UUID().uuidString).prepared' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"; then
  echo "snapshot preparation must use one reclaimable per-game slot" >&2
  exit 1
fi
if grep -Fq 'snapshot: data.subdata(' \
  "$CRATE/bindings/swift/TinyArcadeRuntime.swift"; then
  echo "snapshot restore must borrow the bounded Data slice without a full copy" >&2
  exit 1
fi

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
SIMD_LINKED_BYTES=0
case ",$RUST_FEATURES," in
  *,simd,*)
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
      "$CRATE/tests/ios/TinyArcadeSimdSmoke.swift" \
      -o "$TEMP/TinyArcadeSimdSmoke-arm64"
    SIMD_LINKED_BYTES=$(stat -f%z "$TEMP/TinyArcadeSimdSmoke-arm64")
    ;;
esac
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
# allocator and late-delivery guards. The native tone owner now also handles
# interruption, route-loss and media-service-reset notifications without App
# glue. Indexed2d application metadata then adds strict Rust/Swift decoding and
# exact host-profile negotiation. The bounded tone-wave LRU and direct
# single-buffer WAV synthesis fund one further 16 KiB step. The Apple
# keyboard/gamepad adapter and its two-device executable proof fund three more
# steps for GameController discovery, mapping and disconnect release. Keep these
# complete boundaries explicit rather than hiding them in an unbounded ceiling.
# Preparing the complete snapshot beside its destination, applying protection
# before publication and then replacing the old generation atomically funds one
# further step. This is the persistence transaction itself, not test scaffolding.
# ABI v1.11's profile-bound TAD1 return stayed inside the default product gate.
# ABI v1.12 adds the public typed Swift issue model, strict TAC1 decoder and
# full matching/missing/signature-mismatch paths. That creator-tooling boundary
# crosses two 16 KiB buckets; keep the new ceiling explicit and finite.
MAX_ARM64_LINKED_BYTES=1802240
# The optional SIMD profile keeps v128 inline and adds its portable interpreter
# path only when explicitly requested. Give that opt-in product two separate
# 16 KiB graduation steps. Its ABI v1.11 combined compatibility return crosses
# the prior bucket and receives one further explicit step; never weaken the
# default iOS product ceiling.
case ",$RUST_FEATURES," in
  *,simd,*) MAX_ARM64_LINKED_BYTES=1802240 ;;
esac
# x86_64 is a simulator-only compatibility slice. Keep its separate ceiling
# honest instead of weakening the arm64 product-consumer gate.
# Imported-global store identity crosses the next x86_64 linker size bucket;
# imported-table store/address identity and direct linked functions cross two
# more. Keep the simulator compatibility budget explicit without changing the
# arm64 product ceiling.
# The simulator slice crosses four matching 16 KiB linker buckets; the fourth
# pays for its main-queue route-change dispatch path. The indexed2d metadata
# protocol receives the same one-bucket step as the arm64 product. The bounded
# tone-wave cache receives one matching simulator-only step. The Apple input
# adapter receives three matching steps; its synthetic two-controller proof is
# compiled only into this smoke executable and receives one further step. The
# crash-recoverable prepared slot adds regular-file/symlink discrimination and
# receives one simulator-only step; the arm64 product stays under its ceiling.
MAX_X86_64_LINKED_BYTES=1900544
case ",$RUST_FEATURES," in
  *,simd,*) MAX_X86_64_LINKED_BYTES=1900544 ;;
esac
echo "iOS linked sizes: arm64=${ARM64_LINKED_BYTES} x86_64=${X86_64_LINKED_BYTES} profile-catalog=${HOST_PROFILE_CATALOG_LINKED_BYTES} replay=${REPLAY_LINKED_BYTES} private=${PRIVATE_LIBRARY_LINKED_BYTES} session=${GAME_SESSION_LINKED_BYTES} completion=${COMPLETION_LINKED_BYTES} simd=${SIMD_LINKED_BYTES} bytes"
test "$ARM64_LINKED_BYTES" -le "$MAX_ARM64_LINKED_BYTES"
test "$X86_64_LINKED_BYTES" -le "$MAX_X86_64_LINKED_BYTES"
test "$HOST_PROFILE_CATALOG_LINKED_BYTES" -le "$MAX_ARM64_LINKED_BYTES"
test "$REPLAY_LINKED_BYTES" -le "$MAX_ARM64_LINKED_BYTES"
test "$PRIVATE_LIBRARY_LINKED_BYTES" -le "$MAX_ARM64_LINKED_BYTES"
test "$GAME_SESSION_LINKED_BYTES" -le "$MAX_ARM64_LINKED_BYTES"
test "$COMPLETION_LINKED_BYTES" -le "$MAX_ARM64_LINKED_BYTES"
test "$SIMD_LINKED_BYTES" -eq 0 || test "$SIMD_LINKED_BYTES" -le "$MAX_ARM64_LINKED_BYTES"
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
  case ",$RUST_FEATURES," in
    *,simd,*)
      SIMD_CARTRIDGE="$TEMP/simd-audio-0.1.0.wasm"
      CARGO="$CARGO" "$CRATE/build-simd-audio-cartridge.sh" "$SIMD_CARTRIDGE" >/dev/null
      xcrun simctl spawn booted "$TEMP/TinyArcadeSmoke-arm64" \
        "$DEPTH_CARTRIDGE" "$PADDLE_CARTRIDGE"
      xcrun simctl spawn booted "$TEMP/TinyArcadeSimdSmoke-arm64" "$SIMD_CARTRIDGE"
      ;;
    *)
      xcrun simctl spawn booted "$TEMP/TinyArcadeSmoke-arm64" \
        "$DEPTH_CARTRIDGE" "$PADDLE_CARTRIDGE"
      ;;
  esac
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

echo "OK: iOS device + universal simulator XCFramework and Swift package; links arm64=${ARM64_LINKED_BYTES} x86_64=${X86_64_LINKED_BYTES} profile-catalog=${HOST_PROFILE_CATALOG_LINKED_BYTES} replay=${REPLAY_LINKED_BYTES} private=${PRIVATE_LIBRARY_LINKED_BYTES} session=${GAME_SESSION_LINKED_BYTES} completion=${COMPLETION_LINKED_BYTES} simd=${SIMD_LINKED_BYTES} bytes"
