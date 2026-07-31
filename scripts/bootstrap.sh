#!/usr/bin/env sh
set -eu

: "${AGENTERM_BOOTSTRAP_TASK:?missing AGENTERM_BOOTSTRAP_TASK}"

CLOCK_PROBE=$(date +%s%N 2>/dev/null || true)
case "$CLOCK_PROBE" in
    *N*|'') AGENTERM_BOOTSTRAP_CLOCK_RESOLUTION_MS=1000 ;;
    *) AGENTERM_BOOTSTRAP_CLOCK_RESOLUTION_MS=1 ;;
esac
clock_ms() {
    if [ "$AGENTERM_BOOTSTRAP_CLOCK_RESOLUTION_MS" -eq 1 ]; then
        value=$(date +%s%N)
        printf '%s\n' "$((value / 1000000))"
    else
        value=$(date +%s)
        printf '%s\n' "$((value * 1000))"
    fi
}

AGENTERM_BOOTSTRAP_START_MS=$(clock_ms)
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
REPO=$(git -C "$SCRIPT_DIR" rev-parse --show-toplevel)
cd "$REPO"

AGENTERM_BOOTSTRAP_CARGO_START_MS=$(clock_ms)
cargo build --quiet --locked --bin agenterm-script
AGENTERM_BOOTSTRAP_CARGO_END_MS=$(clock_ms)
AGENTERM_BOOTSTRAP_CARGO_BUILD_MS=$((
    AGENTERM_BOOTSTRAP_CARGO_END_MS - AGENTERM_BOOTSTRAP_CARGO_START_MS
))
TARGET_ROOT=${CARGO_TARGET_DIR:-target}
SOURCE="$TARGET_ROOT/debug/agenterm-script"
BOOTSTRAP_DIR="$TARGET_ROOT/task-bootstrap-$$"
WORKER="$BOOTSTRAP_DIR/agenterm-script"

cleanup() {
    rm -f -- "$WORKER"
    rmdir -- "$BOOTSTRAP_DIR" 2>/dev/null || true
}
trap cleanup EXIT HUP INT TERM
AGENTERM_BOOTSTRAP_COPY_START_MS=$(clock_ms)
mkdir -p -- "$BOOTSTRAP_DIR"
cp -- "$SOURCE" "$WORKER"
# macOS uses BSD chmod, whose option parser does not accept a standalone `--`.
# WORKER is rooted under target/ and cannot begin with an option.
chmod +x "$WORKER"
AGENTERM_BOOTSTRAP_COPY_END_MS=$(clock_ms)
AGENTERM_BOOTSTRAP_WORKER_COPY_MS=$((
    AGENTERM_BOOTSTRAP_COPY_END_MS - AGENTERM_BOOTSTRAP_COPY_START_MS
))
AGENTERM_BOOTSTRAP_WORKER="$WORKER"
AGENTERM_BOOTSTRAP_PLATFORM=unix
case "$(uname -s):$(uname -m)" in
    Linux:aarch64|Linux:arm64) AGENTERM_HOST_OS=linux; AGENTERM_HOST_ARCH=aarch64 ;;
    Linux:*) AGENTERM_HOST_OS=linux; AGENTERM_HOST_ARCH=x86_64 ;;
    Darwin:arm64) AGENTERM_HOST_OS=macos; AGENTERM_HOST_ARCH=aarch64 ;;
    Darwin:*) AGENTERM_HOST_OS=macos; AGENTERM_HOST_ARCH=x86_64 ;;
    *) echo "unsupported bootstrap host" >&2; exit 2 ;;
esac
AGENTERM_BOOTSTRAP_SETUP_END_MS=$(clock_ms)
AGENTERM_BOOTSTRAP_SETUP_MS=$((
    AGENTERM_BOOTSTRAP_SETUP_END_MS - AGENTERM_BOOTSTRAP_START_MS
))
AGENTERM_BOOTSTRAP_OTHER_SETUP_MS=$((
    AGENTERM_BOOTSTRAP_SETUP_MS - AGENTERM_BOOTSTRAP_CARGO_BUILD_MS -
        AGENTERM_BOOTSTRAP_WORKER_COPY_MS
))
if [ "$AGENTERM_BOOTSTRAP_OTHER_SETUP_MS" -lt 0 ]; then
    AGENTERM_BOOTSTRAP_OTHER_SETUP_MS=0
fi
AGENTERM_BOOTSTRAP_TIMING_SCHEMA=1
AGENTERM_BOOTSTRAP_LOCK_WAIT_STATE=included_not_separable
export AGENTERM_BOOTSTRAP_WORKER
export AGENTERM_BOOTSTRAP_PLATFORM
export AGENTERM_HOST_OS AGENTERM_HOST_ARCH
export AGENTERM_BOOTSTRAP_TIMING_SCHEMA AGENTERM_BOOTSTRAP_CLOCK_RESOLUTION_MS
export AGENTERM_BOOTSTRAP_SETUP_MS AGENTERM_BOOTSTRAP_CARGO_BUILD_MS
export AGENTERM_BOOTSTRAP_WORKER_COPY_MS AGENTERM_BOOTSTRAP_OTHER_SETUP_MS
export AGENTERM_BOOTSTRAP_LOCK_WAIT_STATE

"$WORKER" task run "$AGENTERM_BOOTSTRAP_TASK" \
    --manifest "$REPO/agenterm.tasks.json" \
    -- "$@"
