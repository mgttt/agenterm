#!/usr/bin/env bash
# Public black-box smoke for agenterm-cu on Linux/X11 (or Xvfb).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ -z "${DISPLAY:-}" ]]; then
  echo "SKIP: DISPLAY is not set; export DISPLAY=:1 or start Xvfb" >&2
  exit 0
fi

AUDIT_DIR="${AGENTERM_CU_AUDIT_PATH:-${TMPDIR:-/tmp}/agenterm-cu-smoke-$$}"
export AGENTERM_CU_AUDIT_PATH="${AUDIT_DIR}/audit.jsonl"
mkdir -p "$(dirname "$AGENTERM_CU_AUDIT_PATH")"

echo "Building cu..."
cargo build -p agenterm-cu --bin cu
CU="$ROOT/target/debug/cu"

json_field() {
  python3 - "$1" "$2" <<'PY'
import json, sys
payload = json.loads(sys.argv[1])
print(payload[sys.argv[2]])
PY
}

run_json() {
  "$CU" "$@"
}

echo "== capabilities =="
OUT="$(run_json --target current --grant observe capabilities)"
test "$(json_field "$OUT" ok)" = "True"

echo "== windows =="
OUT="$(run_json --target current --grant observe windows)"
test "$(json_field "$OUT" ok)" = "True"

echo "== degraded tree =="
OUT="$(run_json --target current --grant observe tree)"
test "$(json_field "$OUT" ok)" = "True"
python3 - "$OUT" <<'PY'
import json, sys
data = json.loads(sys.argv[1])["data"]
assert data["degraded"] is True
assert "AT-SPI2" in data["reason"]
PY

echo "== wait window-count =="
OUT="$(run_json --target current --grant observe wait --timeout-ms 2000 --window-count-gte 1)"
test "$(json_field "$OUT" ok)" = "True"

echo "== refused actuation without grant =="
OUT="$(run_json --target current --grant observe send-text smoke-refused)"
test "$(json_field "$OUT" ok)" = "False"
python3 - "$OUT" <<'PY'
import json, sys
err = json.loads(sys.argv[1])["error"]
assert err["code"] == "refused"
PY

echo "== audited degraded click =="
OUT="$(run_json --target current --grant actuate click --coords 1,1 --degraded)"
test "$(json_field "$OUT" ok)" = "True"
test -s "$AGENTERM_CU_AUDIT_PATH"

echo "== node click stays unsupported =="
OUT="$(run_json --target current --grant actuate click --window 1 --node btn-1)"
test "$(json_field "$OUT" ok)" = "False"
python3 - "$OUT" <<'PY'
import json, sys
err = json.loads(sys.argv[1])["error"]
assert err["code"] == "unsupported"
PY

echo "PASS: cu-linux-smoke"
