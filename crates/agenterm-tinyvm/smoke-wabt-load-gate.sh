#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FIXTURE="$ROOT/crates/agenterm-tinyvm/tests/fixtures/validate_gate.txt"

command -v wasm-validate >/dev/null 2>&1 || {
  echo "FAIL: wasm-validate is required (install WABT)" >&2
  exit 1
}
command -v xxd >/dev/null 2>&1 || {
  echo "FAIL: xxd is required" >&2
  exit 1
}

accepted=0
rejected=0
while IFS='|' read -r id verdict wasm_hex extra; do
  if [[ -z "$id" || "$id" == \#* ]]; then
    continue
  fi
  if [[ -n "$extra" || -z "$wasm_hex" ]]; then
    echo "FAIL: malformed load-gate fixture row $id" >&2
    exit 1
  fi
  case "$verdict" in
    accept)
      if ! printf '%s' "$wasm_hex" | xxd -r -p | wasm-validate -; then
        echo "FAIL: WABT rejected accepted load-gate case $id" >&2
        exit 1
      fi
      ((accepted += 1))
      ;;
    reject)
      if printf '%s' "$wasm_hex" | xxd -r -p | wasm-validate - >/dev/null 2>&1; then
        echo "FAIL: WABT accepted rejected load-gate case $id" >&2
        exit 1
      fi
      ((rejected += 1))
      ;;
    *)
      echo "FAIL: unknown load-gate verdict for $id: $verdict" >&2
      exit 1
      ;;
  esac
done <"$FIXTURE"

if ((accepted == 0 || rejected == 0)); then
  echo "FAIL: load-gate oracle needs both accepted and rejected cases" >&2
  exit 1
fi

echo "OK: WABT agrees with tinyvm on $rejected rejected and $accepted accepted load-gate cases"
