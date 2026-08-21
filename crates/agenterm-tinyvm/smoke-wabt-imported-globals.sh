#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
CARGO=${CARGO:-cargo}
WAT2WASM=${WAT2WASM:-wat2wasm}
WASM_VALIDATE=${WASM_VALIDATE:-wasm-validate}
WASM="$TEMP/imported-globals-v1.wasm"
ORACLE="$TEMP/ImportedGlobalsOracle"

"$WAT2WASM" --enable-extended-const \
  "$CRATE/tests/fixtures/imported-globals-v1.wat" -o "$WASM"
"$WASM_VALIDATE" --enable-extended-const "$WASM"

TINYVM_WABT_IMPORTED_GLOBALS_WASM="$WASM" "$CARGO" test -q -p agenterm-tinyvm \
  --test wabt_imported_globals_oracle wabt_compiled_imported_globals_match_tinyvm \
  -- --ignored --exact

xcrun swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -framework JavaScriptCore \
  "$CRATE/tests/webkit/ImportedGlobalsOracle.swift" \
  -o "$ORACLE"
"$ORACLE" "$WASM"

echo "OK: WABT validation, tinyvm and JavaScriptCore agree on imported globals"
