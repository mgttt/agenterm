#!/bin/sh
set -eu

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
OUTPUT=${1:-"$ROOT/dist/TinyArcade.xcframework"}
PROFILE=tinyvm-ios-release
TARGET_DIR=${CARGO_TARGET_DIR:-"$ROOT/target/tinyarcade-xcframework"}
CARGO=${CARGO:-cargo}
IOS_DEPLOYMENT_TARGET=${IOS_DEPLOYMENT_TARGET:-14.0}
RUST_FEATURES=${TINYVM_XCFRAMEWORK_FEATURES:-ios-c-api}
HEADERS=${TINYVM_XCFRAMEWORK_HEADERS:-"$CRATE/include"}

if [ -e "$OUTPUT" ]; then
  echo "output already exists: $OUTPUT" >&2
  exit 2
fi

mkdir -p "$(dirname -- "$OUTPUT")"

IPHONEOS_DEPLOYMENT_TARGET="$IOS_DEPLOYMENT_TARGET" CARGO_TARGET_DIR="$TARGET_DIR" "$CARGO" rustc -p agenterm-tinyvm \
  --manifest-path "$ROOT/Cargo.toml" --profile "$PROFILE" \
  --target aarch64-apple-ios --features "$RUST_FEATURES" \
  --lib --crate-type staticlib
IPHONEOS_DEPLOYMENT_TARGET="$IOS_DEPLOYMENT_TARGET" CARGO_TARGET_DIR="$TARGET_DIR" "$CARGO" rustc -p agenterm-tinyvm \
  --manifest-path "$ROOT/Cargo.toml" --profile "$PROFILE" \
  --target aarch64-apple-ios-sim --features "$RUST_FEATURES" \
  --lib --crate-type staticlib
IPHONEOS_DEPLOYMENT_TARGET="$IOS_DEPLOYMENT_TARGET" CARGO_TARGET_DIR="$TARGET_DIR" "$CARGO" rustc -p agenterm-tinyvm \
  --manifest-path "$ROOT/Cargo.toml" --profile "$PROFILE" \
  --target x86_64-apple-ios --features "$RUST_FEATURES" \
  --lib --crate-type staticlib

DEVICE="$TARGET_DIR/aarch64-apple-ios/$PROFILE/libagenterm_tinyvm.a"
SIMULATOR_ARM64="$TARGET_DIR/aarch64-apple-ios-sim/$PROFILE/libagenterm_tinyvm.a"
SIMULATOR_X86_64="$TARGET_DIR/x86_64-apple-ios/$PROFILE/libagenterm_tinyvm.a"
SIMULATOR_DIR="$TARGET_DIR/universal-apple-ios-simulator/$PROFILE"
SIMULATOR="$SIMULATOR_DIR/libagenterm_tinyvm.a"
test -f "$DEVICE"
test -f "$SIMULATOR_ARM64"
test -f "$SIMULATOR_X86_64"
mkdir -p "$SIMULATOR_DIR"
xcrun lipo -create "$SIMULATOR_ARM64" "$SIMULATOR_X86_64" -output "$SIMULATOR"
xcrun lipo "$SIMULATOR" -verify_arch arm64 x86_64

xcodebuild -create-xcframework \
  -library "$DEVICE" -headers "$HEADERS" \
  -library "$SIMULATOR" -headers "$HEADERS" \
  -output "$OUTPUT"

echo "$OUTPUT"
