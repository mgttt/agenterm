#!/usr/bin/env bash
# cursor_agent_fleet_pulse.sh — build a compact <fleet-pulse> for organic awareness
#
# Thin wrapper over scripts/lib/cursor_agent.py (shared registry/mailbox/live parse).
#
# Usage:
#   scripts/cursor_agent_fleet_pulse.sh              # print pulse block
#   scripts/cursor_agent_fleet_pulse.sh --plain      # without XML tags
#   scripts/cursor_agent_fleet_pulse.sh --no-live    # skip API (git only)
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
PY="$ROOT/scripts/lib/cursor_agent.py"
ARGS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --plain|--no-live) ARGS+=("$1"); shift ;;
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

CMD=(python3 "$PY" --root "$ROOT")
if [ -n "${CURSOR_AGENT_REGISTRY:-}" ]; then
  CMD+=(--registry "$CURSOR_AGENT_REGISTRY")
fi
if [ -n "${CURSOR_AGENT_MAILBOX:-}" ]; then
  CMD+=(--mailbox "$CURSOR_AGENT_MAILBOX")
fi
CMD+=(pulse "${ARGS[@]}")
exec "${CMD[@]}"
