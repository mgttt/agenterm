#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
OUTPUT=${1:-"$ROOT/dist/TinyArcade.xcframework"}
PROFILE=tinyvm-ios-release
TARGET_DIR=${CARGO_TARGET_DIR:-"$ROOT/target/tinyarcade-xcframework"}
CARGO=${CARGO:-cargo}
IOS_DEPLOYMENT_TARGET=${IOS_DEPLOYMENT_TARGET:-14.0}

if [ -e "$OUTPUT" ]; then
  echo "output already exists: $OUTPUT" >&2
  exit 2
fi

mkdir -p "$(dirname -- "$OUTPUT")"

IPHONEOS_DEPLOYMENT_TARGET="$IOS_DEPLOYMENT_TARGET" CARGO_TARGET_DIR="$TARGET_DIR" "$CARGO" rustc -p agenterm-tinyvm \
  --profile "$PROFILE" --target aarch64-apple-ios --features ios-c-api \
  --lib --crate-type staticlib
IPHONEOS_DEPLOYMENT_TARGET="$IOS_DEPLOYMENT_TARGET" CARGO_TARGET_DIR="$TARGET_DIR" "$CARGO" rustc -p agenterm-tinyvm \
  --profile "$PROFILE" --target aarch64-apple-ios-sim --features ios-c-api \
  --lib --crate-type staticlib

DEVICE="$TARGET_DIR/aarch64-apple-ios/$PROFILE/libagenterm_tinyvm.a"
SIMULATOR="$TARGET_DIR/aarch64-apple-ios-sim/$PROFILE/libagenterm_tinyvm.a"
test -f "$DEVICE"
test -f "$SIMULATOR"

xcodebuild -create-xcframework \
  -library "$DEVICE" -headers "$CRATE/include" \
  -library "$SIMULATOR" -headers "$CRATE/include" \
  -output "$OUTPUT"

echo "$OUTPUT"
