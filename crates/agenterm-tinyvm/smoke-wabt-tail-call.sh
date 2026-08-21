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
WASM="$TEMP/tail-call-v1.wasm"
ORACLE="$TEMP/TailCallOracle"

"$WAT2WASM" --enable-tail-call "$CRATE/tests/fixtures/tail-call-v1.wat" -o "$WASM"
"$WASM_VALIDATE" --enable-tail-call "$WASM"
INTERP_OUTPUT=$("$WASM_INTERP" --enable-tail-call "$WASM" --run-all-exports)
if [ "$INTERP_OUTPUT" != "run() => i32:143" ]; then
  echo "FAIL: unexpected WABT interpreter result: $INTERP_OUTPUT" >&2
  exit 1
fi

TINYVM_WABT_TAIL_CALL_WASM="$WASM" "$CARGO" test -q -p agenterm-tinyvm \
  --test wabt_tail_call_oracle wabt_compiled_tail_calls_match_tinyvm -- --ignored --exact

xcrun swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -framework JavaScriptCore \
  "$CRATE/tests/webkit/TailCallOracle.swift" \
  -o "$ORACLE"
"$ORACLE" "$WASM"

echo "OK: WABT interpreter, tinyvm and JavaScriptCore agree on standard tail calls"
