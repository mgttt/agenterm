#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
CARGO=${CARGO:-cargo}
WAT2WASM=${WAT2WASM:-wat2wasm}
WASM_VALIDATE=${WASM_VALIDATE:-wasm-validate}
WASM="$TEMP/scalar-proposals-v1.wasm"
ORACLE="$TEMP/StandardScalarOracle"

"$WAT2WASM" "$CRATE/tests/fixtures/scalar-proposals-v1.wat" -o "$WASM"
"$WASM_VALIDATE" "$WASM"

TINYVM_WABT_SCALAR_WASM="$WASM" "$CARGO" test -q -p agenterm-tinyvm \
  --test wabt_scalar_oracle wabt_compiled_scalar_proposals_match_tinyvm -- --ignored --exact

xcrun swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -framework JavaScriptCore \
  "$CRATE/tests/webkit/StandardScalarOracle.swift" \
  -o "$ORACLE"
"$ORACLE" "$WASM"

echo "OK: WABT validation, tinyvm and JavaScriptCore agree on standard scalar proposals"
