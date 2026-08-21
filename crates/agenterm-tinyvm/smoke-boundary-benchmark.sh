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

TINYVM_OUTPUT=$(TINYVM_BOUNDARY_BENCH_WASM="$FIXTURE" \
  "$CARGO" test --release -q -p agenterm-tinyvm \
  --test boundary_benchmark \
  boundary_benchmark_separates_call_view_copy_and_guest_costs -- \
  --ignored --exact --nocapture)
printf '%s\n' "$TINYVM_OUTPUT"

xcrun swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -framework JavaScriptCore \
  "$CRATE/tests/webkit/BoundaryBenchmark.swift" \
  -o "$SWIFT_ORACLE"
JSC_OUTPUT=$("$SWIFT_ORACLE" "$FIXTURE")
printf '%s\n' "$JSC_OUTPUT"

printf '%s\n' "$TINYVM_OUTPUT" | awk -F, '
  $1 == "tinyvm" {
    if (NF != 5 || $4 < 100 || $5 <= 0) exit 1
    print $2 "," $3
    count++
  }
  END { if (count != 32) exit 1 }
' >"$TEMP/tinyvm-dimensions.txt"
printf '%s\n' "$JSC_OUTPUT" | awk -F, '
  $1 == "javascriptcore" {
    if (NF != 5 || $4 < 100 || $5 <= 0) exit 1
    print $2 "," $3
    count++
  }
  END { if (count != 32) exit 1 }
' >"$TEMP/jsc-dimensions.txt"
diff -u "$TEMP/tinyvm-dimensions.txt" "$TEMP/jsc-dimensions.txt"

echo "OK: tinyvm and JavaScriptCore report the same 32 separated boundary-cost dimensions"
