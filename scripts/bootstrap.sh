#!/usr/bin/env sh
set -eu

: "${AGENTERM_BOOTSTRAP_TASK:?missing AGENTERM_BOOTSTRAP_TASK}"
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO=$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel)
cd "$REPO"

cargo build --quiet --locked --bin agenterm-script
TARGET_ROOT=${CARGO_TARGET_DIR:-target}
SOURCE="$TARGET_ROOT/debug/agenterm-script"
BOOTSTRAP_DIR="$TARGET_ROOT/task-bootstrap-$$"
WORKER="$BOOTSTRAP_DIR/agenterm-script"

cleanup() {
    rm -f -- "$WORKER"
    rmdir -- "$BOOTSTRAP_DIR" 2>/dev/null || true
}
trap cleanup EXIT HUP INT TERM
mkdir -p -- "$BOOTSTRAP_DIR"
cp -- "$SOURCE" "$WORKER"
chmod +x "$WORKER"
AGENTERM_BOOTSTRAP_WORKER="$WORKER"
AGENTERM_BOOTSTRAP_PLATFORM=unix
export AGENTERM_BOOTSTRAP_WORKER
export AGENTERM_BOOTSTRAP_PLATFORM

"$WORKER" task run "$AGENTERM_BOOTSTRAP_TASK" \
    --manifest "$REPO/agenterm.tasks.json" \
    --timeout-ms 3600000 \
    --max-operations 100000000 \
    --max-collection-items 100000 \
    --max-string-bytes 8388608 \
    --max-output-bytes 1048576 \
    -- "$@"
