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

"$WAT2WASM" "$CRATE/tests/fixtures/imported-table-v1.wat" -o "$WASM"
"$WASM_VALIDATE" "$WASM"
TINYVM_WABT_IMPORTED_TABLE_WASM="$WASM" "$CARGO" test -q -p agenterm-tinyvm \
  --test wabt_imported_table_oracle \
  wabt_compiled_imported_table_decodes_in_standard_index_space \
  -- --ignored --exact

echo "OK: WABT and tinyvm agree on standard imported-table decoding"
