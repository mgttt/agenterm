#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
CARGO=${CARGO:-cargo}
WAT2WASM=${WAT2WASM:-wat2wasm}
WASM_VALIDATE=${WASM_VALIDATE:-wasm-validate}
FIXTURE="$TEMP/boundary-benchmark-v1.wasm"
SWIFT_ORACLE="$TEMP/BoundaryBenchmark"

"$WAT2WASM" "$CRATE/tests/fixtures/boundary-benchmark-v1.wat" -o "$FIXTURE"
"$WASM_VALIDATE" "$FIXTURE"

TINYVM_BOUNDARY_BENCH_WASM="$FIXTURE" \
  "$CARGO" test --release -q -p agenterm-tinyvm \
  --test boundary_benchmark \
  boundary_benchmark_separates_call_view_copy_and_guest_costs -- \
  --ignored --exact --nocapture

xcrun swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -framework JavaScriptCore \
  "$CRATE/tests/webkit/BoundaryBenchmark.swift" \
  -o "$SWIFT_ORACLE"
"$SWIFT_ORACLE" "$FIXTURE"

echo "OK: tinyvm and JavaScriptCore report separated boundary-cost dimensions"
