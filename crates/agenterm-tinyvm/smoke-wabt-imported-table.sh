#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
CARGO=${CARGO:-cargo}
WAT2WASM=${WAT2WASM:-wat2wasm}
WASM_VALIDATE=${WASM_VALIDATE:-wasm-validate}
WASM="$TEMP/imported-table-v1.wasm"
ALIAS_WASM="$TEMP/imported-table-alias-v1.wasm"
CYCLE_WASM="$TEMP/imported-table-cycle-v1.wasm"
ORACLE="$TEMP/ImportedTableOracle"

"$WAT2WASM" "$CRATE/tests/fixtures/imported-table-v1.wat" -o "$WASM"
"$WASM_VALIDATE" "$WASM"
"$WAT2WASM" "$CRATE/tests/fixtures/imported-table-alias-v1.wat" -o "$ALIAS_WASM"
"$WASM_VALIDATE" "$ALIAS_WASM"
"$WAT2WASM" "$CRATE/tests/fixtures/imported-table-cycle-v1.wat" -o "$CYCLE_WASM"
"$WASM_VALIDATE" "$CYCLE_WASM"
TINYVM_WABT_IMPORTED_TABLE_WASM="$WASM" "$CARGO" test -q -p agenterm-tinyvm \
  --test wabt_imported_table_oracle \
  wabt_compiled_imported_table_decodes_in_standard_index_space \
  -- --ignored --exact
TINYVM_WABT_IMPORTED_TABLE_ALIAS_WASM="$ALIAS_WASM" "$CARGO" test -q \
  -p agenterm-tinyvm --test wabt_imported_table_oracle \
  aliased_import_indices_keep_one_table_identity -- --ignored --exact
TINYVM_WABT_IMPORTED_TABLE_CYCLE_WASM="$CYCLE_WASM" "$CARGO" test -q \
  -p agenterm-tinyvm --test wabt_imported_table_oracle \
  cross_instance_cycles_use_the_store_trampoline -- --ignored --exact

xcrun swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -framework JavaScriptCore \
  "$CRATE/tests/webkit/ImportedTableOracle.swift" \
  -o "$ORACLE"
"$ORACLE" "$WASM"

echo "OK: imported-table decode/alias gate and JavaScriptCore sibling oracle"
