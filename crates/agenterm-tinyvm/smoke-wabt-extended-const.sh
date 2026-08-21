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
WASM="$TEMP/extended-const-v1.wasm"
ORACLE="$TEMP/ExtendedConstOracle"

"$WAT2WASM" --enable-extended-const \
  "$CRATE/tests/fixtures/extended-const-v1.wat" -o "$WASM"
"$WASM_VALIDATE" --enable-extended-const "$WASM"
WABT_OUTPUT=$("$WASM_INTERP" --enable-extended-const "$WASM" --run-all-exports)
printf '%s\n' "$WABT_OUTPUT" | grep -Fq 'run() => i32:199'

TINYVM_WABT_EXTENDED_CONST_WASM="$WASM" "$CARGO" test -q -p agenterm-tinyvm \
  --test wabt_extended_const_oracle wabt_compiled_extended_const_matches_tinyvm \
  -- --ignored --exact

xcrun swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -framework JavaScriptCore \
  "$CRATE/tests/webkit/ExtendedConstOracle.swift" \
  -o "$ORACLE"
"$ORACLE" "$WASM"

echo "OK: WABT, tinyvm and JavaScriptCore agree on standard extended constant expressions"
