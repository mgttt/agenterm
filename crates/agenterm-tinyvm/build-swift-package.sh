#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
OUTPUT=${1:-"$ROOT/dist/TinyArcadeRuntimePackage"}
PARENT=$(dirname -- "$OUTPUT")
CARGO=${CARGO:-cargo}
TARGET_DIR=${CARGO_TARGET_DIR:-"$ROOT/target/tinyarcade-swift-package"}

if [ -e "$OUTPUT" ]; then
  echo "output already exists: $OUTPUT" >&2
  exit 2
fi

mkdir -p "$PARENT"
STAGING=$(mktemp -d "$PARENT/.tinyarcade-swift-package.XXXXXX")
trap 'rm -rf "$STAGING"' EXIT HUP INT TERM
PACKAGE="$STAGING/TinyArcadeRuntimePackage"
mkdir -p "$PACKAGE/Sources/TinyArcadeRuntime"

cp "$CRATE/bindings/swift/package/Package.swift" "$PACKAGE/Package.swift"
cp "$CRATE/bindings/swift/package/README.md" "$PACKAGE/README.md"
cp "$CRATE/bindings/swift/TinyArcadeRuntime.swift" \
  "$PACKAGE/Sources/TinyArcadeRuntime/TinyArcadeRuntime.swift"
CARGO="$CARGO" CARGO_TARGET_DIR="$TARGET_DIR" \
  "$CRATE/build-xcframework.sh" "$PACKAGE/TinyArcade.xcframework"

mv "$PACKAGE" "$OUTPUT"
echo "$OUTPUT"
