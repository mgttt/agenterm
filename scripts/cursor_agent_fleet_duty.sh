#!/usr/bin/env bash
# cursor_agent_fleet_duty.sh — one-shot fleet duty / auto-dream scan
#
# Read-only by default: prints actionable findings from git + mailbox + optional
# live agent list. With --apply, sends short chats for nudge-worthy peers.
#
# Usage:
#   scripts/cursor_agent_fleet_duty.sh
#   scripts/cursor_agent_fleet_duty.sh --apply --from 主控2
#   scripts/cursor_agent_fleet_duty.sh --json
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
REGISTRY="${CURSOR_AGENT_REGISTRY:-$ROOT/skills/cursor/session-registry.md}"
MAILBOX="${CURSOR_AGENT_MAILBOX:-$ROOT/skills/cursor/mailbox.md}"
API_BASE="${CURSOR_AGENT_API_BASE:-https://api.cursor.com/v1}"
APPLY=0
FROM_NAME=""
JSON_OUT=0
STALE_HOURS="${CURSOR_AGENT_DUTY_STALE_HOURS:-4}"

while [ $# -gt 0 ]; do
  case "$1" in
    --apply) APPLY=1; shift ;;
    --from)
      [ $# -ge 2 ] || { echo "--from needs a name" >&2; exit 2; }
      FROM_NAME=$2
      shift 2
      ;;
    --json) JSON_OUT=1; shift ;;
    --stale-hours)
      STALE_HOURS=$2
      shift 2
      ;;
    -h|--help)
      sed -n '2,14p' "$0" | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      exit 2
      ;;
  esac
done

if [ "$APPLY" -eq 1 ] && [ -z "$FROM_NAME" ]; then
  echo "--apply requires --from <displayName>" >&2
  exit 2
fi

LIVE_JSON=""
if [ -n "${CURSOR_API:-}" ]; then
  LIVE_JSON=$(curl -sS --connect-timeout 10 --max-time 25 \
    -u "${CURSOR_API}:" \
    --url "${API_BASE}/agents?limit=100" 2>/dev/null || true)
fi

# Remote task branches ahead of main (name + tip + ahead count).
BRANCH_REPORT=$(
  git -C "$ROOT" fetch origin --prune 2>/dev/null || true
  git -C "$ROOT" for-each-ref --format='%(refname:short) %(objectname:short)' refs/remotes/origin/cursor 2>/dev/null \
    | while read -r ref sha; do
        branch=${ref#origin/}
        ahead=$(git -C "$ROOT" rev-list --count "origin/main..$ref" 2>/dev/null || echo 0)
        if [ "${ahead:-0}" -gt 0 ]; then
          printf '%s\t%s\t%s\n' "$branch" "$sha" "$ahead"
        fi
      done
)

export ROOT REGISTRY MAILBOX LIVE_JSON BRANCH_REPORT STALE_HOURS JSON_OUT APPLY FROM_NAME
FINDINGS=$(python3 - <<'PY'
import json, os, re, subprocess, sys
from pathlib import Path
from datetime import datetime, timezone

root = Path(os.environ["ROOT"])
registry = Path(os.environ["REGISTRY"]).read_text(encoding="utf-8", errors="replace")
mailbox = Path(os.environ["MAILBOX"]).read_text(encoding="utf-8", errors="replace")
live_raw = os.environ.get("LIVE_JSON") or ""
branch_report = os.environ.get("BRANCH_REPORT") or ""
stale_hours = float(os.environ.get("STALE_HOURS") or "4")
json_out = os.environ.get("JSON_OUT") == "1"

def main_sha():
    try:
        return subprocess.check_output(
            ["git", "-C", str(root), "rev-parse", "--short", "origin/main"],
            text=True,
        ).strip()
    except Exception:
        return "?"

findings = []

# Parse registry peers
peers = []
for line in registry.splitlines():
    if not line.strip().startswith("|"):
        continue
    cells = [c.strip() for c in line.strip().strip("|").split("|")]
    if len(cells) < 4:
        continue
    name = re.sub(r"\*+", "", cells[0]).strip()
    bc = cells[1].strip().strip("`")
    role = cells[3]
    if name in ("显示名", "") or not bc.startswith("bc-"):
        continue
    peers.append({"name": name, "bcId": bc, "role": role})

# Seats
seats = {}
current = None
for line in mailbox.splitlines():
    m = re.match(r"^###\s+(.+?)\s*·", line)
    if m:
        current = m.group(1).strip()
        seats[current] = {"raw": [], "status": "", "branch": "", "tip": ""}
        continue
    if current and line.startswith("- "):
        item = line[2:].strip()
        seats[current]["raw"].append(item)
        if item.startswith("状态:"):
            seats[current]["status"] = item.split(":", 1)[1].strip()
        if item.startswith("分支:"):
            seats[current]["branch"] = item.split(":", 1)[1].strip().strip("`")
        if item.startswith("tip:") or item.startswith("证据:"):
            seats[current]["tip"] = item

# Open 请示 without 主控回复 filled
open_asks = []
for block in re.split(r"(?=### 请示#)", mailbox):
    if not block.startswith("### 请示#"):
        continue
    if "已决" in block.split("\n", 1)[0]:
        continue
    if re.search(r"主控回复:\s*（空着|待|空|\s*$)", block, re.M) or re.search(
        r"主控回复:\s*$", block, re.M
    ):
        title = block.splitlines()[0]
        open_asks.append(title)

live_by_id = {}
live_by_name = {}
if live_raw:
    try:
        data = json.loads(live_raw)
        for item in data.get("items") or data.get("agents") or []:
            aid = item.get("id") or item.get("agentId") or ""
            name = item.get("name") or ""
            entry = {
                "status": item.get("status") or item.get("agentStatus") or "?",
                "branch": item.get("branchName") or "",
                "updatedAtMs": item.get("updatedAtMs") or item.get("lastMessageActivityAtMs") or 0,
            }
            if aid:
                live_by_id[aid] = entry
            if name:
                live_by_name[name] = entry
    except Exception:
        pass

now_ms = datetime.now(timezone.utc).timestamp() * 1000
stale_ms = stale_hours * 3600 * 1000

# Branches ahead of main
for line in branch_report.splitlines():
    if not line.strip():
        continue
    branch, sha, ahead = line.split("\t")
    findings.append(
        {
            "kind": "unmerged_branch",
            "severity": "high",
            "branch": branch,
            "tip": sha,
            "ahead": int(ahead),
            "summary": f"branch {branch} @{sha} is {ahead} commit(s) ahead of origin/main — review/merge",
            "nudge": None,
        }
    )

# Peers: stale running / undigested
for peer in peers:
    live = live_by_id.get(peer["bcId"]) or live_by_name.get(peer["name"]) or {}
    seat = seats.get(peer["name"]) or {}
    status_seat = seat.get("status") or peer.get("role") or ""
    live_status = (live.get("status") or "").upper()
    updated = int(live.get("updatedAtMs") or 0)
    age_h = ((now_ms - updated) / 3600000) if updated else None

    if live_status in ("RUNNING", "ACTIVE") and updated and (now_ms - updated) >= stale_ms:
        findings.append(
            {
                "kind": "stale_live",
                "severity": "medium",
                "peer": peer["name"],
                "bcId": peer["bcId"],
                "summary": f"{peer['name']} live={live_status} last activity ~{age_h:.1f}h ago — probe",
                "nudge": {
                    "to": peer["name"],
                    "text": (
                        f"duty探活：mailbox 见你席位「{status_seat[:80]}」。"
                        f"请 pull main、刷新席位心跳；若已交付请 tip SHA，若阻塞请写请示。"
                    ),
                },
            }
        )

    if "等" in status_seat and "合 main" in mailbox and "设计" in status_seat:
        findings.append(
            {
                "kind": "dependency",
                "severity": "medium",
                "peer": peer["name"],
                "bcId": peer["bcId"],
                "summary": f"{peer['name']} seat still waiting on dependency — verify unlock",
                "nudge": {
                    "to": peer["name"],
                    "text": "duty：请 pull main 核对依赖是否已合；可开工则更新席位并继续，否则写阻塞。",
                },
            }
        )

for ask in open_asks:
    findings.append(
        {
            "kind": "open_ask",
            "severity": "high",
            "summary": f"unanswered 请示: {ask}",
            "nudge": None,
        }
    )

# Controller identity
controller = next((p for p in peers if "当前主控" in p.get("role", "")), None)

result = {
    "main": main_sha(),
    "generatedAt": datetime.now(timezone.utc).isoformat(),
    "controller": controller,
    "findingCount": len(findings),
    "findings": findings,
}

if json_out:
    print(json.dumps(result, ensure_ascii=False, indent=2))
else:
    print(f"duty scan main={result['main']} findings={len(findings)}")
    if not findings:
        print("noop: fleet quiet")
    for i, f in enumerate(findings, 1):
        print(f"{i}. [{f['severity']}] {f['kind']}: {f['summary']}")
    nudges = [f for f in findings if f.get("nudge")]
    if nudges:
        print(f"nudge-candidates: {len(nudges)}")

# Stash for apply path
Path("/tmp/cursor-fleet-duty-findings.json").write_text(
    json.dumps(result, ensure_ascii=False), encoding="utf-8"
)
PY
)

printf '%s\n' "$FINDINGS"

if [ "$APPLY" -ne 1 ]; then
  exit 0
fi

python3 - <<'PY'
import json, os, subprocess, sys
from pathlib import Path

data = json.loads(Path("/tmp/cursor-fleet-duty-findings.json").read_text(encoding="utf-8"))
from_name = os.environ["FROM_NAME"]
root = os.environ["ROOT"]
chat = str(Path(root) / "scripts" / "cursor_agent_chat.sh")
sent = 0
seen = set()
for f in data.get("findings") or []:
    n = f.get("nudge")
    if not n:
        continue
    to = n["to"]
    if to in seen:
        continue
    seen.add(to)
    msg = n["text"]
    r = subprocess.run(
        [chat, "--from", from_name, "--to", to, "--no-wait", "--stdin"],
        input=msg,
        text=True,
        cwd=root,
        capture_output=True,
    )
    print(f"nudge {to}: rc={r.returncode} { (r.stdout or r.stderr or '')[-160:] }")
    sent += 1
print(f"apply done nudges={sent}")
sys.exit(0)
PY
