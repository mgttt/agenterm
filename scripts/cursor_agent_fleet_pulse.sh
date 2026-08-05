#!/usr/bin/env bash
# cursor_agent_fleet_pulse.sh — build a compact <fleet-pulse> for organic awareness
#
# Reads session-registry + mailbox (git SSOT) and optionally merges live API
# agent status. Never prints CURSOR_API. Exit 0 even on partial/degraded pulse.
#
# Usage:
#   scripts/cursor_agent_fleet_pulse.sh              # print pulse block
#   scripts/cursor_agent_fleet_pulse.sh --plain      # without XML tags
#   scripts/cursor_agent_fleet_pulse.sh --no-live    # skip API (git only)
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
REGISTRY="${CURSOR_AGENT_REGISTRY:-$ROOT/skills/cursor/session-registry.md}"
MAILBOX="${CURSOR_AGENT_MAILBOX:-$ROOT/skills/cursor/mailbox.md}"
API_BASE="${CURSOR_AGENT_API_BASE:-https://api.cursor.com/v1}"
PLAIN=0
LIVE=1

while [ $# -gt 0 ]; do
  case "$1" in
    --plain) PLAIN=1; shift ;;
    --no-live) LIVE=0; shift ;;
    -h|--help)
      sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      printf 'unknown option: %s\n' "$1" >&2
      exit 2
      ;;
  esac
done

MAIN_SHA=""
if git -C "$ROOT" rev-parse --short HEAD >/dev/null 2>&1; then
  MAIN_SHA=$(git -C "$ROOT" rev-parse --short HEAD 2>/dev/null || true)
  # Prefer origin/main when available (agents may be on task branches).
  if git -C "$ROOT" rev-parse --short origin/main >/dev/null 2>&1; then
    MAIN_SHA=$(git -C "$ROOT" rev-parse --short origin/main 2>/dev/null || echo "$MAIN_SHA")
  fi
fi

LIVE_JSON=""
if [ "$LIVE" -eq 1 ] && [ -n "${CURSOR_API:-}" ]; then
  LIVE_JSON=$(curl -sS --connect-timeout 10 --max-time 25 \
    -u "${CURSOR_API}:" \
    --url "${API_BASE}/agents?limit=100" 2>/dev/null || true)
fi

export REGISTRY MAILBOX MAIN_SHA LIVE_JSON PLAIN
python3 - <<'PY'
import json, os, re, sys
from pathlib import Path

registry = Path(os.environ["REGISTRY"])
mailbox = Path(os.environ["MAILBOX"])
main_sha = os.environ.get("MAIN_SHA") or "?"
live_raw = os.environ.get("LIVE_JSON") or ""
plain = os.environ.get("PLAIN") == "1"

def read(p: Path) -> str:
    try:
        return p.read_text(encoding="utf-8")
    except Exception:
        return ""

reg = read(registry)
mail = read(mailbox)

# Parse registry table rows: | **name** | `bc-…` | ...
roster = []
for line in reg.splitlines():
    if not line.strip().startswith("|"):
        continue
    cells = [c.strip() for c in line.strip().strip("|").split("|")]
    if len(cells) < 4:
        continue
    name = re.sub(r"\*+", "", cells[0]).strip()
    bc = cells[1].strip().strip("`")
    role = cells[3] if len(cells) > 3 else ""
    if name in ("显示名", "") or not bc.startswith("bc-"):
        continue
    roster.append({"name": name, "bcId": bc, "role": role})

# Shared facts: first table after "## 共享事实"
shared = []
in_shared = False
for line in mail.splitlines():
    if line.startswith("## 共享事实"):
        in_shared = True
        continue
    if in_shared:
        if line.startswith("## "):
            break
        if line.strip().startswith("|") and "---" not in line:
            cells = [c.strip() for c in line.strip().strip("|").split("|")]
            if len(cells) >= 2 and cells[0] not in ("键", ""):
                shared.append(f"{cells[0]}={cells[1]}")
shared_s = "; ".join(shared[:6]) if shared else "(none)"

# Seat blocks: ### Name · date
seats = {}
current = None
for line in mail.splitlines():
    m = re.match(r"^###\s+(.+?)\s*·", line)
    if m:
        current = m.group(1).strip()
        seats[current] = []
        continue
    if current and line.startswith("- "):
        seats[current].append(line[2:].strip())
        if len(seats[current]) >= 6:
            current = None

live_by_id = {}
live_by_name = {}
if live_raw:
    try:
        data = json.loads(live_raw)
        items = data.get("items") or data.get("agents") or []
        for item in items:
            aid = item.get("id") or item.get("agentId") or ""
            name = item.get("name") or ""
            status = item.get("status") or item.get("agentStatus") or "?"
            entry = {
                "status": status,
                "branch": item.get("branchName") or "",
            }
            if aid:
                live_by_id[aid] = entry
            if name:
                live_by_name[name] = entry
    except Exception:
        pass

lines = []
lines.append(f"main:{main_sha}")
lines.append(f"shared:{shared_s}")
lines.append("peers:")
if not roster:
    lines.append("- (registry empty or unreadable)")
for peer in roster:
    live = live_by_id.get(peer["bcId"]) or live_by_name.get(peer["name"]) or {}
    live_s = live.get("status") or "git-only"
    branch = live.get("branch") or ""
    # Exact seat heading match only (avoid 主控 ⊂ 主控2).
    seat_lines = seats.get(peer["name"]) or []
    if not seat_lines:
        for k, v in seats.items():
            if k == peer["name"]:
                seat_lines = v
                break
    seat_s = " | ".join(seat_lines[:4]) if seat_lines else peer.get("role", "")
    branch_s = f" branch={branch}" if branch else ""
    lines.append(f"- {peer['name']} [{live_s}]{branch_s} :: {seat_s}")

body = "\n".join(lines)
if plain:
    sys.stdout.write(body + "\n")
else:
    sys.stdout.write("<fleet-pulse>\n" + body + "\n</fleet-pulse>\n")
PY
