#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
WASM="$TEMP/externref-table-v1.wasm"
ORACLE="$TEMP/ExternrefTableOracle"

CARGO=${CARGO:-cargo}
WAT2WASM=${WAT2WASM:-wat2wasm}
WASM_VALIDATE=${WASM_VALIDATE:-wasm-validate}
"$WAT2WASM" "$CRATE/tests/fixtures/externref-table-v1.wat" -o "$WASM"
"$WASM_VALIDATE" "$WASM"

TINYVM_EXTERNREF_TABLE_WASM="$WASM" "$CARGO" test -q -p agenterm-tinyvm --all-features \
  --test wabt_externref_table_oracle wabt_compiled_externref_tables_preserve_host_identity \
  -- --ignored --exact

xcrun swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -framework JavaScriptCore \
  "$CRATE/tests/webkit/ExternrefTableOracle.swift" \
  -o "$ORACLE"
"$ORACLE" "$WASM"

echo "OK: WABT validation, tinyvm and JavaScriptCore agree on standard externref tables"
