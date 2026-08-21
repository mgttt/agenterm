#!/bin/sh
set -eu

ROOT=$(CDPATH='' cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
MATRIX="$CRATE/tests/fixtures/standard-feature-matrix.tsv"
CARGO=${CARGO:-cargo}

test -f "$MATRIX"
gates=$(awk -F '\t' '!/^#/ && NF == 5 && !seen[$3]++ { print $3 }' "$MATRIX")
test "$(printf '%s\n' "$gates" | sed '/^$/d' | wc -l | tr -d ' ')" -eq 10

for gate in $gates; do
  case "$gate" in
    smoke-wabt-*.sh) ;;
    *) echo "invalid feature-matrix gate: $gate" >&2; exit 1 ;;
  esac
  test -x "$CRATE/$gate"
  echo "== standard feature gate: $gate =="
  CARGO="$CARGO" "$CRATE/$gate"
done

echo '== default static-core budget =='
CARGO="$CARGO" "$CRATE/measure-core.sh"

if [ "${TINYVM_MATRIX_RUN_IOS:-1}" -eq 1 ]; then
  echo '== default iOS product budget =='
  CARGO="$CARGO" "$CRATE/smoke-ios-bridge.sh"
  echo '== optional SIMD iOS product budget =='
  TINYVM_XCFRAMEWORK_FEATURES=ios-c-api,simd \
    CARGO="$CARGO" "$CRATE/smoke-ios-bridge.sh"
fi

echo 'OK: every reported standard feature has independent semantics and product budgets'
