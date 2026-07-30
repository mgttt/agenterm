#!/usr/bin/env bash
# cursor_agent_chat.sh — Cursor Cloud Agent ↔ Agent chat helper (desensitized)
#
# Sends a prompt to another Cloud Agent via REST, silently wrapping the body
# with an envelope so the recipient can see who spoke:
#   <from::主控><to::分身1>
#   <message>
#
# Auth: CURSOR_API (HTTP Basic user, empty password). Never printed or committed.
#
# Usage:
#   scripts/cursor_agent_chat.sh --list
#   scripts/cursor_agent_chat.sh --from 主控 --to 分身1 '请继续 Linux GUI 黑盒'
#   scripts/cursor_agent_chat.sh --from 主控 --to bc-5a9c83b4-3a39-42e4-9d33-cb705d848f8f --dry-run 'ping'
#   echo '正文' | scripts/cursor_agent_chat.sh --from 分身1 --to 主控 --stdin
set -euo pipefail

API_BASE="${CURSOR_AGENT_API_BASE:-https://api.cursor.com/v1}"
FROM_NAME=""
TO_NAME=""
DRY_RUN=0
DO_LIST=0
USE_STDIN=0
REGISTRY=""
MESSAGE=""

die() {
  printf '%s\n' "$*" >&2
  exit 2
}

usage() {
  cat <<'EOF'
Usage:
  scripts/cursor_agent_chat.sh --list
  scripts/cursor_agent_chat.sh --from <name|bcId> --to <name|bcId> [--dry-run] [--] MESSAGE...
  scripts/cursor_agent_chat.sh --from <name|bcId> --to <name|bcId> --stdin

Options:
  --from NAME|bcId   Sender display name or bc-… id (required to send)
  --to NAME|bcId     Recipient display name or bc-… id (required to send)
  --list             List agents visible to CURSOR_API (name, id, status)
  --dry-run          Build envelope + JSON; do not call the API
  --stdin            Read message body from stdin
  --registry PATH    Fallback name→bcId hints only if live API miss
                     (default: skills/cursor/session-registry.md)
  -h, --help         Show this help

Resolution (adaptive):
  1) If argument is bc-…, use it (optionally confirmed via live list)
  2) Else resolve display name via live GET /v1/agents (prefer non-archived)
  3) Else fall back to --registry table (may be stale — last resort)

Envelope (prepended silently to the prompt text):
  <from::SENDER><to::RECIPIENT>

Desensitize:
  - Never prints CURSOR_API or Authorization material
  - Redacts bearer/token-looking substrings in API error bodies
  - Logs only name, bcId prefix, run id, and HTTP status
  - Set CURSOR_AGENT_CHAT_VERBOSE=1 to log resolve source (api|registry)
EOF
}

redact() {
  # Strip common secret shapes from tool output before printing.
  sed -E \
    -e 's/[Aa]uthorization:[[:space:]]*Basic[[:space:]]+[A-Za-z0-9+/=]+/Authorization: Basic <redacted>/g' \
    -e 's/[Bb]earer[[:space:]]+[A-Za-z0-9._~+/=-]+/Bearer <redacted>/g' \
    -e 's/key_[A-Za-z0-9]+/key_<redacted>/g' \
    -e 's/cursor_[A-Za-z0-9]{20,}/cursor_<redacted>/g' \
    -e 's/"apiKey"[[:space:]]*:[[:space:]]*"[^"]+"/"apiKey":"<redacted>"/g' \
    -e 's/"token"[[:space:]]*:[[:space:]]*"[^"]+"/"token":"<redacted>"/g'
}

require_api() {
  if [ -z "${CURSOR_API:-}" ]; then
    die "CURSOR_API is unset; inject the Cloud Agent API key into the environment (never commit it)"
  fi
}

bc_prefix() {
  local id="$1"
  if [ "${#id}" -ge 11 ]; then
    printf '%s…' "${id:0:11}"
  else
    printf '%s' "$id"
  fi
}

is_bc_id() {
  [[ "$1" =~ ^bc-[0-9a-fA-F-]{8,}$ ]]
}

repo_root() {
  local here
  here=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
  git -C "$here/.." rev-parse --show-toplevel 2>/dev/null || printf '%s' "$(CDPATH= cd -- "$here/.." && pwd)"
}

default_registry() {
  local root
  root=$(repo_root)
  if [ -f "$root/skills/cursor/session-registry.md" ]; then
    printf '%s' "$root/skills/cursor/session-registry.md"
  fi
}

# Parse optional registry markdown table rows: | **Name** | `bc-…` |
registry_lookup() {
  local want="$1"
  local file="${2:-}"
  [ -n "$file" ] && [ -f "$file" ] || return 1
  # shellcheck disable=SC2016
  awk -v want="$want" '
    /^\|/ {
      line=$0
      gsub(/\*\*/, "", line)
      gsub(/`/, "", line)
      n=split(line, cells, "|")
      if (n < 3) next
      name=cells[2]; id=cells[3]
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", name)
      gsub(/^[[:space:]]+|[[:space:]]+$/, "", id)
      if (name == "" || name == "显示名") next
      if (name == want || id == want) {
        if (id ~ /^bc-/) { print id; exit 0 }
      }
    }
  ' "$file"
}

api_list_json() {
  require_api
  local url="${API_BASE}/agents?limit=100"
  local body code
  body=$(mktemp)
  code=$(curl -sS -o "$body" -w '%{http_code}' \
    -u "${CURSOR_API}:" \
    --url "$url" || true)
  if [ "$code" != "200" ]; then
    printf 'list agents failed HTTP %s\n' "$code" >&2
    redact <"$body" >&2 || true
    rm -f "$body"
    return 1
  fi
  cat "$body"
  rm -f "$body"
}

api_resolve_name() {
  local want="$1"
  api_list_json | python3 -c '
import json, sys
want = sys.argv[1]
data = json.load(sys.stdin)
items = data.get("items") or data.get("agents") or []

def status_rank(status: str) -> int:
    s = (status or "").upper()
    # Prefer live sessions; archived/expired are last-resort only.
    order = {
        "RUNNING": 0,
        "ACTIVE": 0,
        "IDLE": 1,
        "WAITING_FOR_BACKGROUND_WORK": 2,
        "NOT_YET_STARTED": 3,
        "ERROR": 4,
        "EXPIRED": 8,
        "ARCHIVED": 9,
    }
    return order.get(s, 5)

matches = []
for index, item in enumerate(items):
    name = item.get("name") or ""
    agent_id = item.get("id") or item.get("agentId") or ""
    status = item.get("status") or item.get("agentStatus") or ""
    if want == agent_id or want == name:
        matches.append((status_rank(status), index, agent_id, name, status))

if not matches:
    raise SystemExit(1)

# API list is typically newest-first; keep that as the tie-breaker via index.
matches.sort(key=lambda row: (row[0], row[1]))
print(matches[0][2])
' "$want"
}

resolve_agent() {
  local label="$1"
  local role="$2"
  local id=""
  local source=""

  if is_bc_id "$label"; then
    # Explicit bcId: still verify via live list when possible; keep as-is if offline.
    if id=$(api_resolve_name "$label" 2>/dev/null); then
      printf '%s' "$id"
      return 0
    fi
    printf '%s' "$label"
    return 0
  fi

  # Adaptive: live Cloud Agents API by display name wins over a stale registry bcId.
  if id=$(api_resolve_name "$label" 2>/dev/null); then
    source=api
  elif [ -n "$REGISTRY" ]; then
    id=$(registry_lookup "$label" "$REGISTRY" || true)
    [ -n "$id" ] && source=registry
  fi

  if [ -z "$id" ]; then
    die "cannot resolve ${role} '${label}' to a live bcId (try --list; registry is fallback only)"
  fi

  if [ "${CURSOR_AGENT_CHAT_VERBOSE:-0}" != "0" ]; then
    printf 'resolve %s=%s via %s -> %s\n' "$role" "$label" "$source" "$(bc_prefix "$id")" >&2
  fi
  printf '%s' "$id"
}

list_agents() {
  require_api
  api_list_json | python3 -c "
import json, sys
data = json.load(sys.stdin)
items = data.get('items') or data.get('agents') or []
print(f\"{'name':<12} {'id':<40} {'status':<10} url\")
print('-' * 100)
for item in items:
    name = item.get('name') or ''
    agent_id = item.get('id') or item.get('agentId') or ''
    status = item.get('status') or item.get('agentStatus') or ''
    url = item.get('url') or (f'https://cursor.com/agents/{agent_id}' if agent_id else '')
    print(f'{name:<12} {agent_id:<40} {status:<10} {url}')
"
}

build_envelope_message() {
  local from_label="$1"
  local to_label="$2"
  local body="$3"
  printf '<from::%s><to::%s>\n%s' "$from_label" "$to_label" "$body"
}

json_payload() {
  local text="$1"
  PROMPT_TEXT="$text" python3 -c '
import json, os
print(json.dumps({"prompt": {"text": os.environ["PROMPT_TEXT"]}}, ensure_ascii=False))
'
}

send_run() {
  local to_id="$1"
  local payload="$2"
  require_api
  local body code
  body=$(mktemp)
  code=$(curl -sS -o "$body" -w '%{http_code}' \
    -X POST \
    -u "${CURSOR_API}:" \
    --header 'Content-Type: application/json' \
    --url "${API_BASE}/agents/${to_id}/runs" \
    --data "$payload" || true)

  local redacted
  redacted=$(redact <"$body" || true)
  rm -f "$body"

  case "$code" in
    200|201|202)
      printf 'ok http=%s to=%s run=%s\n' \
        "$code" \
        "$(bc_prefix "$to_id")" \
        "$(printf '%s' "$redacted" | python3 -c 'import json,sys
try:
 d=json.load(sys.stdin)
 run=(d.get("run") or {})
 print(run.get("id") or d.get("id") or "unknown")
except Exception:
 print("unknown")' 2>/dev/null || echo unknown)"
      return 0
      ;;
    409)
      printf 'busy http=409 to=%s (recipient has an active run; retry when IDLE)\n' \
        "$(bc_prefix "$to_id")" >&2
      printf '%s\n' "$redacted" >&2
      return 1
      ;;
    *)
      printf 'error http=%s to=%s\n' "$code" "$(bc_prefix "$to_id")" >&2
      printf '%s\n' "$redacted" >&2
      return 1
      ;;
  esac
}

# ---- argv ----
while [ $# -gt 0 ]; do
  case "$1" in
    --from)
      [ $# -ge 2 ] || die "--from needs a value"
      FROM_NAME=$2
      shift 2
      ;;
    --to)
      [ $# -ge 2 ] || die "--to needs a value"
      TO_NAME=$2
      shift 2
      ;;
    --registry)
      [ $# -ge 2 ] || die "--registry needs a path"
      REGISTRY=$2
      shift 2
      ;;
    --list)
      DO_LIST=1
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    --stdin)
      USE_STDIN=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    --)
      shift
      break
      ;;
    -*)
      die "unknown option: $1"
      ;;
    *)
      break
      ;;
  esac
done

if [ -z "$REGISTRY" ]; then
  REGISTRY=$(default_registry || true)
fi

if [ "$DO_LIST" -eq 1 ]; then
  list_agents
  exit 0
fi

[ -n "$FROM_NAME" ] || die "--from is required (or pass --list)"
[ -n "$TO_NAME" ] || die "--to is required (or pass --list)"

if [ "$USE_STDIN" -eq 1 ]; then
  MESSAGE=$(cat)
elif [ $# -gt 0 ]; then
  MESSAGE="$*"
else
  die "message body required (argv or --stdin)"
fi

[ -n "${MESSAGE//[[:space:]]/}" ] || die "message body is empty"

FROM_ID=$(resolve_agent "$FROM_NAME" from)
TO_ID=$(resolve_agent "$TO_NAME" to)

ENVELOPED=$(build_envelope_message "$FROM_NAME" "$TO_NAME" "$MESSAGE")
PAYLOAD=$(json_payload "$ENVELOPED")

if [ "$DRY_RUN" -eq 1 ]; then
  printf 'dry-run from=%s(%s) to=%s(%s) bytes=%s\n' \
    "$FROM_NAME" "$(bc_prefix "$FROM_ID")" \
    "$TO_NAME" "$(bc_prefix "$TO_ID")" \
    "$(printf '%s' "$ENVELOPED" | wc -c | tr -d ' ')"
  printf 'envelope:\n'
  printf '%s\n' "$ENVELOPED" | head -n 2
  if [ "$(printf '%s' "$ENVELOPED" | wc -l | tr -d ' ')" -gt 2 ]; then
    printf '… (%s more lines omitted)\n' \
      "$(( $(printf '%s' "$ENVELOPED" | wc -l | tr -d ' ') - 2 ))"
  fi
  exit 0
fi

send_run "$TO_ID" "$PAYLOAD"
