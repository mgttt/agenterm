#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
CARGO=${CARGO:-cargo}
WAT2WASM=${WAT2WASM:-wat2wasm}
WASM_VALIDATE=${WASM_VALIDATE:-wasm-validate}
WASM="$TEMP/typed-host-v1.wasm"
ORACLE="$TEMP/TypedHostOracle"

"$WAT2WASM" "$CRATE/tests/fixtures/typed-host-v1.wat" -o "$WASM"
"$WASM_VALIDATE" "$WASM"

TINYVM_WABT_TYPED_HOST_WASM="$WASM" "$CARGO" test -q -p agenterm-tinyvm \
  --test wabt_typed_host_oracle wabt_compiled_typed_host_import_matches_tinyvm -- --ignored --exact

xcrun swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -framework JavaScriptCore \
  "$CRATE/tests/webkit/TypedHostOracle.swift" \
  -o "$ORACLE"
"$ORACLE" "$WASM"

echo "OK: WABT validation, tinyvm and JavaScriptCore agree on standard typed host imports"
