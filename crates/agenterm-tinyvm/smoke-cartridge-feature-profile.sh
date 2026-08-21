#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
CARGO=${CARGO:-cargo}
TARGET_DIR=${CARGO_TARGET_DIR:-"$ROOT/target"}

"$CRATE/build-depth-well-cartridge.sh" "$TEMP/depth-well.wasm" >/dev/null
"$CRATE/build-paddle-guard-cartridge.sh" "$TEMP/paddle-guard.wasm" >/dev/null
"$CRATE/build-signal-lock-cartridge.sh" "$TEMP/signal-lock.wasm" >/dev/null

CARGO_TARGET_DIR="$TARGET_DIR" "$CARGO" build -q -p agenterm-tinyvm --bin tinyvm
TINYVM="$TARGET_DIR/debug/tinyvm"

check_features() {
  cartridge=$1
  expected=$2
  actual=$($TINYVM module validate "$cartridge" | sed -n 's/^standard_features=//p')
  if [ "$actual" != "$expected" ]; then
    echo "unexpected feature profile for $(basename -- "$cartridge"): $actual" >&2
    exit 1
  fi
}

check_features "$TEMP/depth-well.wasm" "bulk-memory,sign-extension"
check_features "$TEMP/paddle-guard.wasm" "bulk-memory"
check_features "$TEMP/signal-lock.wasm" "bulk-memory,sign-extension"

echo "OK: real cartridges prioritize bulk-memory and sign-extension; no speculative proposal inferred"
