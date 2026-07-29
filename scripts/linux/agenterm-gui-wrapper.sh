#!/usr/bin/env bash
# Launch the Linux GUI with bundled user-space display libraries when present.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")" && pwd)"
if [[ -d "$ROOT/lib" ]]; then
  export LD_LIBRARY_PATH="$ROOT/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
fi
exec "$ROOT/.agenterm.bin" "$@"
