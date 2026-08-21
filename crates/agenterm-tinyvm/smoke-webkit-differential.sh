#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
CARGO=${CARGO:-cargo}
ORACLE="$TEMP/TinyArcadeWebKitOracle"
ADAPTER="$CRATE/tests/webkit/TinyArcadeWebKitOracle.js"

xcrun swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -framework JavaScriptCore \
  -framework CryptoKit \
  "$CRATE/tests/webkit/TinyArcadeWebKitOracle.swift" \
  -o "$ORACLE"

run_game() {
  name=$1
  build_script=$2
  input_plan=$3
  wasm="$TEMP/$name.wasm"
  replay="$TEMP/$name.tareplay"
  "$build_script" "$wasm" >/dev/null
  "$CARGO" run -q -p agenterm-tinyvm --features replay -- \
    replay record "$wasm" "$input_plan" "$replay" >/dev/null
  "$CARGO" run -q -p agenterm-tinyvm --features replay -- \
    replay check "$wasm" "$replay" >/dev/null
  "$ORACLE" "$ADAPTER" "$wasm" "$replay"
}

run_game \
  depth-well-0.1.0 \
  "$CRATE/build-depth-well-cartridge.sh" \
  "$CRATE/tests/fixtures/depth-well-replay-v1.inputs"
run_game \
  paddle-guard-0.1.0 \
  "$CRATE/build-paddle-guard-cartridge.sh" \
  "$CRATE/tests/fixtures/paddle-guard-replay-v1.inputs"

echo "OK: development-only WebKit differential; no H5 runtime enters the iOS app"
