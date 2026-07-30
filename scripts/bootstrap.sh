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
# macOS uses BSD chmod, whose option parser does not accept a standalone `--`.
# WORKER is rooted under target/ and cannot begin with an option.
chmod +x "$WORKER"
AGENTERM_BOOTSTRAP_WORKER="$WORKER"
AGENTERM_BOOTSTRAP_PLATFORM=unix
case "$(uname -s):$(uname -m)" in
    Linux:aarch64|Linux:arm64) AGENTERM_HOST_OS=linux; AGENTERM_HOST_ARCH=aarch64 ;;
    Linux:*) AGENTERM_HOST_OS=linux; AGENTERM_HOST_ARCH=x86_64 ;;
    Darwin:arm64) AGENTERM_HOST_OS=macos; AGENTERM_HOST_ARCH=aarch64 ;;
    Darwin:*) AGENTERM_HOST_OS=macos; AGENTERM_HOST_ARCH=x86_64 ;;
    *) echo "unsupported bootstrap host" >&2; exit 2 ;;
esac
export AGENTERM_BOOTSTRAP_WORKER
export AGENTERM_BOOTSTRAP_PLATFORM
export AGENTERM_HOST_OS AGENTERM_HOST_ARCH

"$WORKER" task run "$AGENTERM_BOOTSTRAP_TASK" \
    --manifest "$REPO/agenterm.tasks.json" \
    -- "$@"
