#!/usr/bin/env bash
# macOS AX current-tree smoke — PLACEHOLDER recipe (cut 3.45).
#
# Status: NOT YET RUN as a live gate. This Linux cut only landed the AX
# adapter + PRD branch tree. A later macOS agent must execute this recipe
# on Darwin and claim live evidence only after the fixture reply matches.
#
# Canonical argv:
#   agenterm-cu --target current --grant observe tree --window "$HANDLE"
#
# Fixture (cut-owned native window):
#   - unique title + stable PID
#   - editable text seeded with 345AXTREE
#   - button named "Fixture Press"
#
# Required reply:
#   ok:true  target:"current"  command:"tree"  data.backend:"ax"
#   exactly one 345AXTREE text control and one "Fixture Press" button
#   scoped to HANDLE. Never screenshot / --coords / CGEvent / AT-SPI / UIA.
#
# Typed failures to keep:
#   a11y_permission_denied | unsupported | a11y_tree_timeout | a11y_*_limit
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "SKIP: cu-macos-smoke requires Darwin (PLACEHOLDER recipe only; live AX not claimed here)" >&2
  exit 0
fi

AUDIT_DIR="${AGENTERM_CU_AUDIT_PATH:-${TMPDIR:-/tmp}/agenterm-cu-macos-smoke-$$}"
export AGENTERM_CU_AUDIT_PATH="${AUDIT_DIR}/audit.jsonl"
mkdir -p "$(dirname "$AGENTERM_CU_AUDIT_PATH")"

echo "Building agenterm-cu + libagenterm (abi-dev unwind profile)..."
cargo build -p agenterm-cu --bin agenterm-cu --profile abi-dev
CU="$ROOT/target/abi-dev/agenterm-cu"

json_field() {
  python3 - "$1" "$2" <<'PY'
import json, sys
payload = json.loads(sys.argv[1])
key = sys.argv[2]
cur = payload
for part in key.split("."):
    cur = cur[part]
print(cur)
PY
}

run_json() {
  "$CU" "$@"
}

echo "== capabilities =="
OUT="$(run_json --target current --grant observe capabilities)"
test "$(json_field "$OUT" ok)" = "True"

echo "== windows (resolve HANDLE for the 345AXTREE fixture) =="
OUT="$(run_json --target current --grant observe windows)"
test "$(json_field "$OUT" ok)" = "True"
# macOS agent: set HANDLE from the fixture window (unique title / PID).
# Example once the fixture is running:
#   HANDLE="$(python3 -c 'import json,sys; ...')"
if [[ -z "${HANDLE:-}" ]]; then
  echo "FAIL: set HANDLE to the fixture CGWindowID before running the live gate" >&2
  echo "  agenterm-cu --target current --grant observe windows" >&2
  exit 1
fi

echo "== ax tree (canonical observe path) =="
OUT="$(run_json --target current --grant observe tree --window "$HANDLE")"
test "$(json_field "$OUT" ok)" = "True"
test "$(json_field "$OUT" target)" = "current"
test "$(json_field "$OUT" command)" = "tree"
python3 - "$OUT" "$HANDLE" <<'PY'
import json, sys
reply = json.loads(sys.argv[1])
handle = int(sys.argv[2])
assert reply["ok"] is True
assert reply["target"] == "current"
assert reply["command"] == "tree"
data = reply["data"]
assert data["degraded"] is False
assert data["backend"] == "ax", data.get("backend")
assert data["addressing"] == "accessibility-tree"
assert data.get("window") == handle or data.get("window") == handle
nodes = data["nodes"]
assert nodes, "expected non-empty AX tree"
texts = [n for n in nodes if (n.get("text") or "") == "345AXTREE"]
press = [n for n in nodes if n.get("name") == "Fixture Press"]
assert len(texts) == 1, f"expected exactly one 345AXTREE text node, got {len(texts)}"
assert len(press) == 1, f"expected exactly one Fixture Press button, got {len(press)}"
print("PASS: ax tree fixture nodes present")
PY

echo "== typed permission path is documented (manual when TCC denied) =="
echo "  When AXIsProcessTrusted() is false, tree must fail a11y_permission_denied."
echo "  Do not fall back to pixels or coordinates."

echo "PASS: cu-macos-smoke (live AX evidence for this Darwin host)"
