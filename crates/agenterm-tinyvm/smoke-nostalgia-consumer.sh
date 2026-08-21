#!/bin/sh
set -eu

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
CONSUMER=${NOSTALGIA_ARCADE_REPO:-"$(dirname -- "$ROOT")/nostalgia-arcade"}
GATE="$CONSUMER/scripts/test-tinyarcade-consumer.sh"
DEPTH="$CONSUMER/App/Resources/depth-well-0.1.0.wasm"
SIGNAL="$CONSUMER/App/Resources/signal-lock-0.1.0.wasm"
PROJECT="$CONSUMER/NostalgiaArcade.xcodeproj/project.pbxproj"
AUDIO_OWNER="$CONSUMER/App/Sources/ArcadeFeedback.swift"
AUDIO_TEST="$CONSUMER/App/Tests/BundledDepthWellCartridgeRuntimeTests.swift"
DEPTH_SCREEN="$CONSUMER/App/Sources/TinyArcadeDepthWellScreen.swift"
SIGNAL_SCREEN="$CONSUMER/App/Sources/TinyArcadeSignalLockScreen.swift"

test -x "$GATE"
test -f "$DEPTH"
test -f "$SIGNAL"
test -f "$PROJECT"
test -f "$AUDIO_OWNER"
test -f "$AUDIO_TEST"
test -f "$DEPTH_SCREEN"
test -f "$SIGNAL_SCREEN"
grep -Fq 'private let cartridgeTonePlayer = TinyArcadeTonePlayer()' "$AUDIO_OWNER"
grep -Fq 'try? cartridgeTonePlayer.play([tone])' "$AUDIO_OWNER"
grep -Fq 'testRealCartridgeToneUsesRuntimePlayerAndDeactivates' "$AUDIO_TEST"
grep -Fq 'perform(tone, hapticCue:' "$DEPTH_SCREEN"
grep -Fq 'perform(tone, hapticCue:' "$SIGNAL_SCREEN"

# This gate is allowed to refresh ignored build products, but a successful run
# must not silently rewrite any committed consumer input. A runtime/cartridge
# change therefore requires an explicit consumer commit before this closes.
before=$(shasum -a 256 "$DEPTH" "$SIGNAL" "$PROJECT")
TINYARCADE_REPO="$ROOT" "$GATE"
after=$(shasum -a 256 "$DEPTH" "$SIGNAL" "$PROJECT")
test "$before" = "$after" || {
  echo 'FAIL: current tinyvm output changed a tracked Nostalgia Arcade input' >&2
  exit 1
}

producer="$ROOT/target/tinyarcade-swift-package/aarch64-apple-ios/tinyvm-ios-release/libagenterm_tinyvm.a"
consumed="$CONSUMER/.build/TinyArcadeRuntimePackage/TinyArcade.xcframework/ios-arm64/libagenterm_tinyvm.a"
app="$CONSUMER/.build/TinyArcadeConsumerGate-device/Build/Products/Release-iphoneos/NostalgiaArcade.app/NostalgiaArcade"

test -f "$producer"
test -f "$consumed"
test -f "$app"
cmp "$producer" "$consumed"
nm -gj "$app" | grep -Fqx '_tinyarcade_v1_completion_create'

echo 'OK: exact current-main tinyvm archive and ABI v1.10 run in the real arm64 App target'
shasum -a 256 "$producer" "$consumed"
