#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
CARGO=${CARGO:-cargo}
WAT2WASM=${WAT2WASM:-wat2wasm}
WASM_VALIDATE=${WASM_VALIDATE:-wasm-validate}
WASM_INTERP=${WASM_INTERP:-wasm-interp}
WASM="$TEMP/multi-memory-v1.wasm"
ORACLE="$TEMP/MultiMemoryOracle"

"$WAT2WASM" --enable-multi-memory \
  "$CRATE/tests/fixtures/multi-memory-v1.wat" -o "$WASM"
"$WASM_VALIDATE" --enable-multi-memory "$WASM"
INTERP_OUTPUT=$("$WASM_INTERP" --enable-multi-memory "$WASM" --run-all-exports)
if [ "$INTERP_OUTPUT" != "run() => i32:1225" ]; then
  echo "FAIL: unexpected WABT interpreter result: $INTERP_OUTPUT" >&2
  exit 1
fi

TINYVM_WABT_MULTI_MEMORY_WASM="$WASM" "$CARGO" test -q -p agenterm-tinyvm \
  --test wabt_multi_memory_oracle wabt_compiled_multi_memory_matches_tinyvm -- --ignored --exact

xcrun swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -framework JavaScriptCore \
  "$CRATE/tests/webkit/MultiMemoryOracle.swift" \
  -o "$ORACLE"
"$ORACLE" "$WASM"

echo "OK: WABT and tinyvm agree on standard multiple memories; JavaScriptCore capability recorded"
