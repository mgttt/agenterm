#!/usr/bin/env bash
# cursor_agent_fleet_duty.sh — one-shot fleet duty / auto-dream scan
#
# Thin wrapper over scripts/lib/cursor_agent.py.
#
# Usage:
#   scripts/cursor_agent_fleet_duty.sh
#   scripts/cursor_agent_fleet_duty.sh --apply --from 主控2
#   scripts/cursor_agent_fleet_duty.sh --json
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
PY="$ROOT/scripts/lib/cursor_agent.py"
ARGS=()
FROM_NAME=""
APPLY=0

while [ $# -gt 0 ]; do
  case "$1" in
    --apply) APPLY=1; ARGS+=(--apply); shift ;;
    --from)
      [ $# -ge 2 ] || { echo "--from needs a name" >&2; exit 2; }
      FROM_NAME=$2
      ARGS+=(--from "$2")
      shift 2
      ;;
    --json|--no-live) ARGS+=("$1"); shift ;;
    --stale-hours)
      ARGS+=(--stale-hours "$2")
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

CMD=(python3 "$PY" --root "$ROOT")
if [ -n "${CURSOR_AGENT_REGISTRY:-}" ]; then
  CMD+=(--registry "$CURSOR_AGENT_REGISTRY")
fi
if [ -n "${CURSOR_AGENT_MAILBOX:-}" ]; then
  CMD+=(--mailbox "$CURSOR_AGENT_MAILBOX")
fi
CMD+=(duty "${ARGS[@]}")
exec "${CMD[@]}"
