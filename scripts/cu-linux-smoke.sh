#!/usr/bin/env bash
# Public black-box smoke for agenterm-cu on Linux/X11 + AT-SPI2.
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
# libagenterm requires panic=unwind; the workspace dev/release profiles abort.
cargo build -p agenterm-cu --bin cu --profile abi-dev
CU="$ROOT/target/abi-dev/cu"

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

echo "== at-spi tree =="
OUT="$(run_json --target current --grant observe tree)"
test "$(json_field "$OUT" ok)" = "True"
python3 - "$OUT" <<'PY'
import json, sys
data = json.loads(sys.argv[1])["data"]
assert data["degraded"] is False
assert data["backend"] == "at-spi2"
assert data["addressing"] == "accessibility-tree"
assert len(data["nodes"]) > 0
node = data["nodes"][0]
for key in ("id", "role", "name", "states", "bounds", "actions"):
    assert key in node
bounds = node["bounds"]
for key in ("x", "y", "width", "height"):
    assert key in bounds
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

echo "== invalid at-spi node path fails typed =="
OUT="$(run_json --target current --grant actuate click --node /0/999999)"
test "$(json_field "$OUT" ok)" = "False"
python3 - "$OUT" <<'PY'
import json, sys
err = json.loads(sys.argv[1])["error"]
assert err["code"] == "a11y_node_not_found"
PY

echo "== name-addressed click requires --window =="
OUT="$(run_json --target current --grant actuate click --name Reload)"
test "$(json_field "$OUT" ok)" = "False"
python3 - "$OUT" <<'PY'
import json, sys
err = json.loads(sys.argv[1])["error"]
assert err["code"] == "invalid_input"
PY

echo "== name-addressed click missing node fails typed =="
HANDLE="$(python3 - "$("$CU" --target current --grant observe windows)" <<'PY'
import json, sys
payload = json.loads(sys.argv[1])
if payload.get("ok") and payload.get("data"):
    print(payload["data"][0]["handle"])
PY
)"
if [[ -n "${HANDLE:-}" ]]; then
  OUT="$(run_json --target current --grant actuate click --window "$HANDLE" --name agenterm-no-such-control)"
  test "$(json_field "$OUT" ok)" = "False"
  python3 - "$OUT" <<'PY'
import json, sys
err = json.loads(sys.argv[1])["error"]
assert err["code"] in ("a11y_node_not_found", "unsupported")
PY
fi

echo "== structured at-spi click when node exists =="
OUT="$(run_json --target current --grant observe tree)"
NODE_ID="$(python3 - "$OUT" <<'PY'
import json, sys

def showing(node):
    bounds = node.get("bounds") or {}
    return bounds.get("width", 0) > 0 and bounds.get("height", 0) > 0

def named_click(node):
    return any(str(action).lower() in ("click", "press") for action in node.get("actions", []))

def actionable_control(node):
    role = str(node.get("role") or "").lower()
    name = str(node.get("name") or "").lower()
    if name in ("minimize", "maximize", "close", "close this view"):
        return False
    return showing(node) and (
        named_click(node)
        or role in ("button", "push button", "toggle button", "link")
        or "button" in role
    )

nodes = json.loads(sys.argv[1])["data"]["nodes"]
for node in nodes:
    if named_click(node) and showing(node):
        print(node["id"])
        raise SystemExit
for node in nodes:
    if actionable_control(node):
        print(node["id"])
        break
PY
)"
if [[ -n "${NODE_ID:-}" ]]; then
  OUT="$(run_json --target current --grant actuate click --node "$NODE_ID")"
  test "$(json_field "$OUT" ok)" = "True"
  python3 - "$OUT" <<'PY'
import json, sys
data = json.loads(sys.argv[1])["data"]
assert data["addressing"] == "accessibility-tree"
assert data.get("addressing") != "degraded-coordinates"
PY
else
  echo "SKIP: no showing AT-SPI button/link or named click action in current desktop tree"
fi

echo "PASS: cu-linux-smoke"
