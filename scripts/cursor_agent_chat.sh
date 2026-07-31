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
# Robustness (default):
#   - On HTTP 409 agent_busy, wait + exponential backoff + jitter until IDLE
#     (latest run FINISHED/ERROR/…) or --wait-timeout, then retry POST
#   - Adaptive re-resolve of display names during wait (stale bcId foresight)
#   - Curl connect/max timeouts; transient 5xx/429 retry; fatal 4xx fail-fast
#   - Payload via temp file (no ARG_MAX); message byte budget
#
# Exit codes:
#   0 ok | 1 busy/timeout | 2 usage | 3 auth | 4 network | 5 api error
#
# Usage:
#   scripts/cursor_agent_chat.sh --list
#   scripts/cursor_agent_chat.sh --from 主控 --to 分身1 '请继续 Linux GUI 黑盒'
#   scripts/cursor_agent_chat.sh --from 主控 --to 分身1 --no-wait 'ping'
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

# Wait policy: default ON (robust). Override with --no-wait or env.
WAIT="${CURSOR_AGENT_CHAT_WAIT:-1}"
WAIT_TIMEOUT="${CURSOR_AGENT_CHAT_WAIT_TIMEOUT:-600}"
MAX_BYTES="${CURSOR_AGENT_CHAT_MAX_BYTES:-100000}"
CONNECT_TIMEOUT="${CURSOR_AGENT_CHAT_CONNECT_TIMEOUT:-15}"
CURL_MAX_TIME="${CURSOR_AGENT_CHAT_CURL_MAX_TIME:-60}"
BACKOFF_INITIAL="${CURSOR_AGENT_CHAT_BACKOFF_INITIAL:-2}"
BACKOFF_MAX="${CURSOR_AGENT_CHAT_BACKOFF_MAX:-45}"
RE_RESOLVE_EVERY="${CURSOR_AGENT_CHAT_RE_RESOLVE_EVERY:-3}"
PRECHECK="${CURSOR_AGENT_CHAT_PRECHECK:-1}"

# shellcheck disable=SC2034
EXIT_OK=0
EXIT_BUSY=1
EXIT_USAGE=2
EXIT_AUTH=3
EXIT_NETWORK=4
EXIT_API=5

TMP_FILES=()
cleanup() {
  local f
  for f in "${TMP_FILES[@]:-}"; do
    rm -f "$f" 2>/dev/null || true
  done
}
trap cleanup EXIT

mktemp_tracked() {
  local destination="$1"
  local f
  # Command substitutions do not inherit the parent shell's EXIT trap.
  trap cleanup EXIT
  f=$(mktemp)
  TMP_FILES+=("$f")
  printf -v "$destination" '%s' "$f"
}

die() {
  local code="${2:-$EXIT_USAGE}"
  printf '%s\n' "$1" >&2
  exit "$code"
}

require_non_negative_integer() {
  local name="$1"
  local value="$2"
  case "$value" in
    ''|*[!0-9]*) die "$name must be a non-negative integer" ;;
  esac
}

require_positive_integer() {
  local name="$1"
  local value="$2"
  require_non_negative_integer "$name" "$value"
  [ "$value" -gt 0 ] || die "$name must be greater than zero"
}

require_binary_flag() {
  local name="$1"
  local value="$2"
  case "$value" in
    0|1) ;;
    *) die "$name must be 0 or 1" ;;
  esac
}

usage() {
  cat <<'EOF'
Usage:
  scripts/cursor_agent_chat.sh --list
  scripts/cursor_agent_chat.sh --from <name|bcId> --to <name|bcId> [options] [--] MESSAGE...
  scripts/cursor_agent_chat.sh --from <name|bcId> --to <name|bcId> --stdin

Options:
  --from NAME|bcId     Sender display name or bc-… id (required to send)
  --to NAME|bcId       Recipient display name or bc-… id (required to send)
  --list               List agents visible to CURSOR_API (name, id, status, latestRun)
  --dry-run            Build envelope + JSON; do not call the API
  --stdin              Read message body from stdin
  --wait               Wait/retry on 409 agent_busy (default)
  --no-wait            Fail immediately on 409 (exit 1)
  --wait-timeout SEC   Max seconds to wait for recipient (default 600;
                       env CURSOR_AGENT_CHAT_WAIT_TIMEOUT)
  --registry PATH      Fallback name→bcId hints only if live API miss
                       (default: skills/cursor/session-registry.md)
  -h, --help           Show this help

Resolution (adaptive):
  1) If argument is bc-…, use it (optionally confirmed via live list)
  2) Else resolve display name via live GET /v1/agents (prefer non-archived)
  3) Else fall back to --registry table (may be stale — last resort)
  During --wait, display names are re-resolved every few attempts.

Busy foresight:
  Agent list status ACTIVE ≠ free for a new run. Free when latest run is
  FINISHED / ERROR / FAILED / CANCELLED (or missing). Otherwise POST yields
  409 agent_busy. This script probes GET /agents/{id} + GET …/runs/{latestRunId}
  before/between POSTs when waiting.

Envelope (prepended silently to the prompt text):
  <from::SENDER><to::RECIPIENT>

Exit codes:
  0 ok | 1 busy/timeout | 2 usage | 3 auth | 4 network | 5 api error

Desensitize:
  - Never prints CURSOR_API or Authorization material
  - Redacts bearer/token-looking substrings in API error bodies
  - Logs only name, bcId prefix, run id, and HTTP status
  - Set CURSOR_AGENT_CHAT_VERBOSE=1 to log resolve source / wait probes
EOF
}

logv() {
  if [ "${CURSOR_AGENT_CHAT_VERBOSE:-0}" != "0" ]; then
    printf '%s\n' "$*" >&2
  fi
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

require_tools() {
  command -v curl >/dev/null 2>&1 || die "curl is required" "$EXIT_USAGE"
  command -v python3 >/dev/null 2>&1 || die "python3 is required" "$EXIT_USAGE"
}

require_api() {
  if [ -z "${CURSOR_API:-}" ]; then
    die "CURSOR_API is unset; inject the Cloud Agent API key into the environment (never commit it)" "$EXIT_AUTH"
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

now_epoch() {
  date +%s
}

# Sleep SEC, or less if remaining budget is smaller. Returns 1 if budget exhausted.
sleep_budget() {
  local want="$1"
  local deadline="$2"
  local now rem
  now=$(now_epoch)
  rem=$((deadline - now))
  if [ "$rem" -le 0 ]; then
    return 1
  fi
  if [ "$want" -gt "$rem" ]; then
    want=$rem
  fi
  # shellcheck disable=SC2034
  sleep "$want" || true
  return 0
}

# Exponential backoff with ±25% jitter; capped by BACKOFF_MAX.
next_backoff() {
  local current="$1"
  local next jitter
  next=$((current * 2))
  if [ "$next" -gt "$BACKOFF_MAX" ]; then
    next=$BACKOFF_MAX
  fi
  # jitter in 0..current/4 (bash $RANDOM)
  jitter=$((RANDOM % (current / 4 + 1)))
  if ((RANDOM % 2)); then
    next=$((next + jitter))
  else
    next=$((next - jitter))
  fi
  if [ "$next" -lt 1 ]; then
    next=1
  fi
  printf '%s' "$next"
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

# Generic curl: sets globals CURL_HTTP_CODE and CURL_BODY_FILE.
# Args: METHOD URL [data_file_for_POST]
# Returns 0 on HTTP exchange completed (any code); 1 on transport failure.
curl_api() {
  local method="$1"
  local url="$2"
  local data_file="${3:-}"
  require_api
  mktemp_tracked CURL_BODY_FILE
  local code curl_rc=0
  local -a args
  args=(-sS -o "$CURL_BODY_FILE" -w '%{http_code}'
    --connect-timeout "$CONNECT_TIMEOUT"
    --max-time "$CURL_MAX_TIME"
    -u "${CURSOR_API}:"
    --url "$url")
  case "$method" in
    GET) ;;
    POST)
      args+=(-X POST --header 'Content-Type: application/json')
      if [ -n "$data_file" ]; then
        args+=(--data-binary @"$data_file")
      else
        args+=(--data '{}')
      fi
      ;;
    *)
      die "internal: unsupported method $method" "$EXIT_USAGE"
      ;;
  esac
  code=$(curl "${args[@]}" || curl_rc=$?)
  CURL_HTTP_CODE="$code"
  if [ "$curl_rc" -ne 0 ]; then
    logv "curl transport failure rc=$curl_rc url=$(printf '%s' "$url" | sed -E 's#[0-9a-fA-F-]{20,}#…#g')"
    return 1
  fi
  return 0
}

classify_http() {
  # stdout: ok | busy | auth | not_found | rate | transient | fatal
  local code="$1"
  case "$code" in
    200|201|202) printf 'ok' ;;
    409) printf 'busy' ;;
    401|403) printf 'auth' ;;
    404) printf 'not_found' ;;
    408|425|429) printf 'rate' ;;
    500|502|503|504) printf 'transient' ;;
    "") printf 'transient' ;;
    *) printf 'fatal' ;;
  esac
}

api_list_json() {
  if ! curl_api GET "${API_BASE}/agents?limit=100"; then
    return 1
  fi
  case "$(classify_http "$CURL_HTTP_CODE")" in
    ok)
      cat "$CURL_BODY_FILE"
      return 0
      ;;
    auth)
      printf 'list agents failed HTTP %s (auth)\n' "$CURL_HTTP_CODE" >&2
      redact <"$CURL_BODY_FILE" >&2 || true
      return 3
      ;;
    *)
      printf 'list agents failed HTTP %s\n' "$CURL_HTTP_CODE" >&2
      redact <"$CURL_BODY_FILE" >&2 || true
      return 1
      ;;
  esac
}

api_resolve_name() {
  local want="$1"
  local json
  json=$(api_list_json) || return 1
  printf '%s' "$json" | python3 -c '
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
    die "cannot resolve ${role} '${label}' to a live bcId (try --list; registry is fallback only)" "$EXIT_USAGE"
  fi

  logv "resolve ${role}=${label} via ${source} -> $(bc_prefix "$id")"
  printf '%s' "$id"
}

# Print: agentStatus|latestRunId|runStatus  (runStatus may be empty/"?")
probe_recipient() {
  local agent_id="$1"
  if ! curl_api GET "${API_BASE}/agents/${agent_id}"; then
    printf 'NETWORK||'
    return 1
  fi
  local kind
  kind=$(classify_http "$CURL_HTTP_CODE")
  if [ "$kind" != "ok" ]; then
    printf 'HTTP%s||' "$CURL_HTTP_CODE"
    return 1
  fi
  local agent_status latest_run
  # API may return bare agent object or {"agent":{...}}
  read -r agent_status latest_run < <(python3 -c '
import json,sys
d=json.load(open(sys.argv[1]))
a=d.get("agent") if isinstance(d.get("agent"), dict) else d
print((a.get("status") or a.get("agentStatus") or "?") + " " + (a.get("latestRunId") or ""))
' "$CURL_BODY_FILE")

  local run_status=""
  if [ -n "${latest_run:-}" ]; then
    if curl_api GET "${API_BASE}/agents/${agent_id}/runs/${latest_run}"; then
      if [ "$(classify_http "$CURL_HTTP_CODE")" = "ok" ]; then
        run_status=$(python3 -c '
import json,sys
d=json.load(open(sys.argv[1]))
r=d.get("run") if isinstance(d.get("run"), dict) else d
print(r.get("status") or "?")
' "$CURL_BODY_FILE")
      else
        run_status="?"
      fi
    else
      run_status="?"
    fi
  fi
  printf '%s|%s|%s' "${agent_status:-?}" "${latest_run:-}" "${run_status:-}"
  return 0
}

# 0 = free to accept a new run; 1 = busy; 2 = unknown (probe failed)
recipient_is_free() {
  local probe="$1"
  local agent_status run_status
  agent_status=$(printf '%s' "$probe" | awk -F'|' '{print $1}')
  run_status=$(printf '%s' "$probe" | awk -F'|' '{print $3}')
  case "$agent_status" in
    NETWORK*|HTTP*|'')
      return 2
      ;;
  esac
  case "$(printf '%s' "$run_status" | tr '[:lower:]' '[:upper:]')" in
    ""|FINISHED|ERROR|FAILED|CANCELLED|CANCELED|EXPIRED|COMPLETED|SUCCESS)
      return 0
      ;;
    "?")
      return 2
      ;;
    *)
      return 1
      ;;
  esac
}

list_agents() {
  require_api
  local json
  json=$(api_list_json) || exit $?
  printf '%s' "$json" | python3 -c "
import json, sys
data = json.load(sys.stdin)
items = data.get('items') or data.get('agents') or []
print(f\"{'name':<12} {'id':<40} {'status':<10} {'latestRun':<12} url\")
print('-' * 110)
for item in items:
    name = item.get('name') or ''
    agent_id = item.get('id') or item.get('agentId') or ''
    status = item.get('status') or item.get('agentStatus') or ''
    lr = item.get('latestRunId') or ''
    lr_short = (lr[:12] + '…') if len(lr) > 12 else lr
    url = item.get('url') or (f'https://cursor.com/agents/{agent_id}' if agent_id else '')
    print(f'{name:<12} {agent_id:<40} {status:<10} {lr_short:<12} {url}')
"
}

build_envelope_message() {
  local from_label="$1"
  local to_label="$2"
  local body="$3"
  printf '<from::%s><to::%s>\n%s' "$from_label" "$to_label" "$body"
}

# Write JSON payload to file path $1 from text in env/file to avoid ARG_MAX.
json_payload_to_file() {
  local out="$1"
  local text="$2"
  PROMPT_TEXT="$text" python3 -c '
import json, os, sys
path = sys.argv[1]
payload = {"prompt": {"text": os.environ["PROMPT_TEXT"]}}
with open(path, "w", encoding="utf-8") as f:
    json.dump(payload, f, ensure_ascii=False)
' "$out"
}

parse_run_id() {
  local body_file="$1"
  python3 -c '
import json,sys
try:
    d=json.load(open(sys.argv[1]))
    run=(d.get("run") or {})
    print(run.get("id") or d.get("id") or "unknown")
except Exception:
    print("unknown")
' "$body_file" 2>/dev/null || echo unknown
}

# POST once. Sets LAST_HTTP_CODE / LAST_CLASS. Returns 0 on success.
send_run_once() {
  local to_id="$1"
  local payload_file="$2"
  LAST_HTTP_CODE=""
  LAST_CLASS=""
  if ! curl_api POST "${API_BASE}/agents/${to_id}/runs" "$payload_file"; then
    LAST_HTTP_CODE=""
    LAST_CLASS=network
    return 1
  fi
  LAST_HTTP_CODE="$CURL_HTTP_CODE"
  LAST_CLASS=$(classify_http "$CURL_HTTP_CODE")
  case "$LAST_CLASS" in
    ok)
      printf 'ok http=%s to=%s run=%s\n' \
        "$CURL_HTTP_CODE" \
        "$(bc_prefix "$to_id")" \
        "$(parse_run_id "$CURL_BODY_FILE")"
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

send_run_resilient() {
  local to_label="$1"
  local to_id="$2"
  local payload_file="$3"
  local deadline attempt=0 backoff="$BACKOFF_INITIAL"
  local probe class

  deadline=$(( $(now_epoch) + WAIT_TIMEOUT ))

  # Foresight: if waiting, avoid burning a 409 when latest run is clearly active.
  if [ "$WAIT" -eq 1 ] && [ "$PRECHECK" -eq 1 ]; then
    probe=$(probe_recipient "$to_id" || true)
    logv "precheck to=$(bc_prefix "$to_id") probe=$probe"
    local free_rc=0
    recipient_is_free "$probe" || free_rc=$?
    if [ "$free_rc" -eq 1 ]; then
      printf 'wait to=%s probe=%s (recipient busy; polling up to %ss)\n' \
        "$(bc_prefix "$to_id")" "$probe" "$WAIT_TIMEOUT" >&2
      while true; do
        now=$(now_epoch)
        if [ "$now" -ge "$deadline" ]; then
          printf 'timeout http=409 to=%s waited=%ss (still busy: %s)\n' \
            "$(bc_prefix "$to_id")" "$WAIT_TIMEOUT" "$probe" >&2
          return "$EXIT_BUSY"
        fi
        sleep_budget "$backoff" "$deadline" || {
          printf 'timeout http=409 to=%s waited=%ss\n' \
            "$(bc_prefix "$to_id")" "$WAIT_TIMEOUT" >&2
          return "$EXIT_BUSY"
        }
        backoff=$(next_backoff "$backoff")
        # Adaptive re-resolve by display name (not raw bcId arg).
        if ! is_bc_id "$to_label"; then
          attempt=$((attempt + 1))
          if [ $((attempt % RE_RESOLVE_EVERY)) -eq 0 ]; then
            local new_id
            if new_id=$(api_resolve_name "$to_label" 2>/dev/null); then
              if [ "$new_id" != "$to_id" ]; then
                printf 're-resolve to=%s -> %s (was %s)\n' \
                  "$to_label" "$(bc_prefix "$new_id")" "$(bc_prefix "$to_id")" >&2
                to_id=$new_id
              fi
            fi
          fi
        fi
        probe=$(probe_recipient "$to_id" || true)
        logv "poll probe=$probe backoff=${backoff}s"
        free_rc=0
        recipient_is_free "$probe" || free_rc=$?
        if [ "$free_rc" -eq 0 ]; then
          printf 'ready to=%s probe=%s\n' "$(bc_prefix "$to_id")" "$probe" >&2
          break
        fi
        # free_rc=2 (unknown): keep polling rather than hammering POST.
      done
      backoff=$BACKOFF_INITIAL
    fi
  fi

  attempt=0
  while true; do
    attempt=$((attempt + 1))
    if send_run_once "$to_id" "$payload_file"; then
      return 0
    fi
    class="${LAST_CLASS:-fatal}"
    case "$class" in
      busy)
        if [ "$WAIT" -ne 1 ]; then
          printf 'busy http=409 to=%s (recipient has an active run; use --wait or retry when free)\n' \
            "$(bc_prefix "$to_id")" >&2
          redact <"$CURL_BODY_FILE" >&2 || true
          return "$EXIT_BUSY"
        fi
        now=$(now_epoch)
        if [ "$now" -ge "$deadline" ]; then
          printf 'timeout http=409 to=%s waited=%ss after %s attempts\n' \
            "$(bc_prefix "$to_id")" "$WAIT_TIMEOUT" "$attempt" >&2
          redact <"$CURL_BODY_FILE" >&2 || true
          return "$EXIT_BUSY"
        fi
        printf 'busy http=409 to=%s attempt=%s; sleep %ss (deadline in %ss)\n' \
          "$(bc_prefix "$to_id")" "$attempt" "$backoff" "$((deadline - now))" >&2
        sleep_budget "$backoff" "$deadline" || {
          printf 'timeout http=409 to=%s\n' "$(bc_prefix "$to_id")" >&2
          return "$EXIT_BUSY"
        }
        backoff=$(next_backoff "$backoff")
        if ! is_bc_id "$to_label" && [ $((attempt % RE_RESOLVE_EVERY)) -eq 0 ]; then
          local new_id
          if new_id=$(api_resolve_name "$to_label" 2>/dev/null); then
            if [ "$new_id" != "$to_id" ]; then
              printf 're-resolve to=%s -> %s\n' "$to_label" "$(bc_prefix "$new_id")" >&2
              to_id=$new_id
            fi
          fi
        fi
        # Opportunistic probe to skip useless POSTs.
        probe=$(probe_recipient "$to_id" || true)
        logv "post-409 probe=$probe"
        ;;
      auth)
        printf 'error http=%s to=%s (auth)\n' "${LAST_HTTP_CODE:-?}" "$(bc_prefix "$to_id")" >&2
        redact <"${CURL_BODY_FILE:-/dev/null}" >&2 || true
        return "$EXIT_AUTH"
        ;;
      not_found)
        # Maybe stale id — one adaptive re-resolve then retry once-ish inside loop.
        printf 'error http=404 to=%s (not found; re-resolving)\n' "$(bc_prefix "$to_id")" >&2
        if ! is_bc_id "$to_label"; then
          local new_id
          if new_id=$(api_resolve_name "$to_label" 2>/dev/null) && [ -n "$new_id" ]; then
            to_id=$new_id
            printf 're-resolve after 404 -> %s\n' "$(bc_prefix "$to_id")" >&2
            if [ "$WAIT" -eq 1 ]; then
              now=$(now_epoch)
              [ "$now" -lt "$deadline" ] || return "$EXIT_API"
              sleep_budget 1 "$deadline" || true
              continue
            fi
          fi
        fi
        redact <"${CURL_BODY_FILE:-/dev/null}" >&2 || true
        return "$EXIT_API"
        ;;
      rate|transient|network)
        if [ "$WAIT" -ne 1 ]; then
          printf 'error http=%s to=%s class=%s (transient; pass --wait to retry)\n' \
            "${LAST_HTTP_CODE:-?}" "$(bc_prefix "$to_id")" "$class" >&2
          redact <"${CURL_BODY_FILE:-/dev/null}" >&2 || true
          if [ "$class" = "network" ]; then
            return "$EXIT_NETWORK"
          fi
          return "$EXIT_API"
        fi
        now=$(now_epoch)
        if [ "$now" -ge "$deadline" ]; then
          printf 'timeout http=%s to=%s class=%s after %ss\n' \
            "${LAST_HTTP_CODE:-?}" "$(bc_prefix "$to_id")" "$class" "$WAIT_TIMEOUT" >&2
          return "$EXIT_NETWORK"
        fi
        printf 'retry http=%s to=%s class=%s attempt=%s; sleep %ss\n' \
          "${LAST_HTTP_CODE:-?}" "$(bc_prefix "$to_id")" "$class" "$attempt" "$backoff" >&2
        sleep_budget "$backoff" "$deadline" || return "$EXIT_NETWORK"
        backoff=$(next_backoff "$backoff")
        ;;
      *)
        printf 'error http=%s to=%s\n' "${LAST_HTTP_CODE:-?}" "$(bc_prefix "$to_id")" >&2
        redact <"${CURL_BODY_FILE:-/dev/null}" >&2 || true
        return "$EXIT_API"
        ;;
    esac
  done
}

# ---- argv ----
require_tools

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
    --wait)
      WAIT=1
      shift
      ;;
    --no-wait)
      WAIT=0
      shift
      ;;
    --wait-timeout)
      [ $# -ge 2 ] || die "--wait-timeout needs seconds"
      WAIT_TIMEOUT=$2
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

# Validate arithmetic knobs before they reach tests, comparisons, or modulo.
require_binary_flag "CURSOR_AGENT_CHAT_WAIT" "$WAIT"
require_binary_flag "CURSOR_AGENT_CHAT_PRECHECK" "$PRECHECK"
require_non_negative_integer "--wait-timeout" "$WAIT_TIMEOUT"
require_non_negative_integer "CURSOR_AGENT_CHAT_MAX_BYTES" "$MAX_BYTES"
require_positive_integer "CURSOR_AGENT_CHAT_BACKOFF_INITIAL" "$BACKOFF_INITIAL"
require_positive_integer "CURSOR_AGENT_CHAT_BACKOFF_MAX" "$BACKOFF_MAX"
require_positive_integer "CURSOR_AGENT_CHAT_RE_RESOLVE_EVERY" "$RE_RESOLVE_EVERY"

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

ENVELOPED=$(build_envelope_message "$FROM_NAME" "$TO_NAME" "$MESSAGE")
BYTE_LEN=$(printf '%s' "$ENVELOPED" | wc -c | tr -d ' ')
if [ "$BYTE_LEN" -gt "$MAX_BYTES" ]; then
  die "message too large: ${BYTE_LEN} bytes (budget ${MAX_BYTES}; set CURSOR_AGENT_CHAT_MAX_BYTES to raise)" "$EXIT_USAGE"
fi

FROM_ID=$(resolve_agent "$FROM_NAME" from)
TO_ID=$(resolve_agent "$TO_NAME" to)

mktemp_tracked PAYLOAD_FILE
json_payload_to_file "$PAYLOAD_FILE" "$ENVELOPED"

if [ "$DRY_RUN" -eq 1 ]; then
  printf 'dry-run from=%s(%s) to=%s(%s) bytes=%s wait=%s wait_timeout=%s\n' \
    "$FROM_NAME" "$(bc_prefix "$FROM_ID")" \
    "$TO_NAME" "$(bc_prefix "$TO_ID")" \
    "$BYTE_LEN" \
    "$WAIT" \
    "$WAIT_TIMEOUT"
  printf 'envelope:\n'
  printf '%s\n' "$ENVELOPED" | head -n 2
  if [ "$(printf '%s' "$ENVELOPED" | wc -l | tr -d ' ')" -gt 2 ]; then
    printf '… (%s more lines omitted)\n' \
      "$(( $(printf '%s' "$ENVELOPED" | wc -l | tr -d ' ') - 2 ))"
  fi
  exit 0
fi

send_run_resilient "$TO_NAME" "$TO_ID" "$PAYLOAD_FILE"
exit $?
