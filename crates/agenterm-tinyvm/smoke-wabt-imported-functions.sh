#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
CARGO=${CARGO:-cargo}
WAT2WASM=${WAT2WASM:-wat2wasm}
WASM_VALIDATE=${WASM_VALIDATE:-wasm-validate}
PROVIDER_WASM="$TEMP/exported-functions-v1.wasm"
CONSUMER_WASM="$TEMP/imported-functions-v1.wasm"
RELAY_WASM="$TEMP/relinked-function-v1.wasm"
ORACLE="$TEMP/ImportedFunctionsOracle"

"$WAT2WASM" "$CRATE/tests/fixtures/exported-functions-v1.wat" -o "$PROVIDER_WASM"
"$WASM_VALIDATE" "$PROVIDER_WASM"
"$WAT2WASM" --enable-tail-call "$CRATE/tests/fixtures/imported-functions-v1.wat" \
  -o "$CONSUMER_WASM"
"$WASM_VALIDATE" --enable-tail-call "$CONSUMER_WASM"
"$WAT2WASM" "$CRATE/tests/fixtures/relinked-function-v1.wat" -o "$RELAY_WASM"
"$WASM_VALIDATE" "$RELAY_WASM"

TINYVM_WABT_EXPORTED_FUNCTIONS_WASM="$PROVIDER_WASM" \
  TINYVM_WABT_IMPORTED_FUNCTIONS_WASM="$CONSUMER_WASM" \
  TINYVM_WABT_RELINKED_FUNCTION_WASM="$RELAY_WASM" \
  "$CARGO" test -q -p agenterm-tinyvm \
  --test wabt_imported_functions_oracle \
  wabt_compiled_exported_functions_link_across_instances -- --ignored --exact

xcrun swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -framework JavaScriptCore \
  "$CRATE/tests/webkit/ImportedFunctionsOracle.swift" \
  -o "$ORACLE"
"$ORACLE" "$PROVIDER_WASM" "$CONSUMER_WASM" "$RELAY_WASM"

echo "OK: WABT validation, tinyvm and JavaScriptCore agree on linked functions"
