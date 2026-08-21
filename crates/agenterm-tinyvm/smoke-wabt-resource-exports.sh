#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
CARGO=${CARGO:-cargo}
WAT2WASM=${WAT2WASM:-wat2wasm}
WASM_VALIDATE=${WASM_VALIDATE:-wasm-validate}
WASM="$TEMP/resource-exports-v1.wasm"
ORACLE="$TEMP/ResourceExportsOracle"

"$WAT2WASM" "$CRATE/tests/fixtures/resource-exports-v1.wat" -o "$WASM"
"$WASM_VALIDATE" "$WASM"

TINYVM_WABT_RESOURCE_EXPORTS_WASM="$WASM" "$CARGO" test -q -p agenterm-tinyvm \
  --test wabt_resource_exports_oracle wabt_compiled_resource_exports_match_tinyvm \
  -- --ignored --exact

xcrun swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -framework JavaScriptCore \
  "$CRATE/tests/webkit/ResourceExportsOracle.swift" \
  -o "$ORACLE"
"$ORACLE" "$WASM"

echo "OK: WABT validation, tinyvm and JavaScriptCore agree on resource exports"
