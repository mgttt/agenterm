#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIXTURE_DIR="$ROOT/crates/agenterm-tinyvm/tests/fixtures"

command -v wasm-validate >/dev/null 2>&1 || {
  echo "FAIL: wasm-validate is required (install WABT)" >&2
  exit 1
}
command -v xxd >/dev/null 2>&1 || {
  echo "FAIL: xxd is required" >&2
  exit 1
}

fixtures=(
  "$FIXTURE_DIR/mvp_goldens.txt"
  "$FIXTURE_DIR/family_extra.txt"
  "$FIXTURE_DIR/family_edge.txt"
)

count=0
for fixture in "${fixtures[@]}"; do
  while IFS='|' read -r id _family _opcodes _expect wasm_hex _bind; do
    if [[ -z "$id" || "$id" == \#* ]]; then
      continue
    fi
    if ! printf '%s' "$wasm_hex" | xxd -r -p | wasm-validate -; then
      echo "FAIL: WABT rejected golden $id" >&2
      exit 1
    fi
    ((count += 1))
  done <"$fixture"
done

if ((count == 0)); then
  echo "FAIL: no golden modules were checked" >&2
  exit 1
fi

echo "OK: WABT validates all $count tinyvm golden modules"
