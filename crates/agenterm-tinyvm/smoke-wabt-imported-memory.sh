#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
CARGO=${CARGO:-cargo}
WAT2WASM=${WAT2WASM:-wat2wasm}
WASM_VALIDATE=${WASM_VALIDATE:-wasm-validate}
WASM="$TEMP/imported-memory-v1.wasm"
PROVIDER_WASM="$TEMP/exported-memory-v1.wasm"
ALIAS_WASM="$TEMP/imported-memory-alias-v1.wasm"
ORACLE="$TEMP/ImportedMemoryOracle"

"$WAT2WASM" "$CRATE/tests/fixtures/imported-memory-v1.wat" -o "$WASM"
"$WASM_VALIDATE" "$WASM"
"$WAT2WASM" "$CRATE/tests/fixtures/exported-memory-v1.wat" -o "$PROVIDER_WASM"
"$WASM_VALIDATE" "$PROVIDER_WASM"
"$WAT2WASM" --enable-multi-memory \
  "$CRATE/tests/fixtures/imported-memory-alias-v1.wat" -o "$ALIAS_WASM"
"$WASM_VALIDATE" --enable-multi-memory "$ALIAS_WASM"

TINYVM_WABT_IMPORTED_MEMORY_WASM="$WASM" \
  TINYVM_WABT_EXPORTED_MEMORY_WASM="$PROVIDER_WASM" \
  "$CARGO" test -q -p agenterm-tinyvm \
  --test wabt_imported_memory_oracle wabt_compiled_imported_memory_matches_tinyvm \
  -- --ignored --exact
TINYVM_WABT_IMPORTED_MEMORY_ALIAS_WASM="$ALIAS_WASM" "$CARGO" test -q \
  -p agenterm-tinyvm --test wabt_imported_memory_oracle \
  aliased_import_indices_keep_one_memory_identity -- --ignored --exact

xcrun swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -framework JavaScriptCore \
  "$CRATE/tests/webkit/ImportedMemoryOracle.swift" \
  -o "$ORACLE"
"$ORACLE" "$WASM" "$PROVIDER_WASM"

echo "OK: WABT validation, tinyvm and JavaScriptCore agree on linked exported memory"
