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
WASM="$TEMP/multi-table-v1.wasm"
ORACLE="$TEMP/MultiTableOracle"

"$WAT2WASM" "$CRATE/tests/fixtures/multi-table-v1.wat" -o "$WASM"
"$WASM_VALIDATE" "$WASM"
INTERP_OUTPUT=$("$WASM_INTERP" "$WASM" --run-all-exports)
if [ "$INTERP_OUTPUT" != "run() => i32:143" ]; then
  echo "FAIL: unexpected WABT interpreter result: $INTERP_OUTPUT" >&2
  exit 1
fi

TINYVM_WABT_MULTI_TABLE_WASM="$WASM" "$CARGO" test -q -p agenterm-tinyvm \
  --test wabt_multi_table_oracle wabt_compiled_multi_table_matches_tinyvm -- --ignored --exact

xcrun swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -framework JavaScriptCore \
  "$CRATE/tests/webkit/MultiTableOracle.swift" \
  -o "$ORACLE"
"$ORACLE" "$WASM"

echo "OK: WABT interpreter, tinyvm and JavaScriptCore agree on standard multiple tables"
