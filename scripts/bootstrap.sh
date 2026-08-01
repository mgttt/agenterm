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

case "$(uname -s):$(uname -m)" in
    Linux:aarch64|Linux:arm64) AGENTERM_HOST_OS=linux; AGENTERM_HOST_ARCH=aarch64 ;;
    Linux:*) AGENTERM_HOST_OS=linux; AGENTERM_HOST_ARCH=x86_64 ;;
    Darwin:arm64) AGENTERM_HOST_OS=macos; AGENTERM_HOST_ARCH=aarch64 ;;
    Darwin:*) AGENTERM_HOST_OS=macos; AGENTERM_HOST_ARCH=x86_64 ;;
    *) echo "unsupported bootstrap host" >&2; exit 2 ;;
esac

TARGET_ROOT=${CARGO_TARGET_DIR:-target}
CACHE_DIR="$TARGET_ROOT/bootstrap-worker-cache/$AGENTERM_HOST_OS-$AGENTERM_HOST_ARCH"
CACHE_WORKER="$CACHE_DIR/agenterm-script"
CACHE_STAMP="$CACHE_DIR/compatibility.stamp"
IDENTITY_FILE="$CACHE_DIR/identity-$$.txt"
POST_IDENTITY_FILE="$CACHE_DIR/post-identity-$$.txt"
UNTRACKED_FILE="$CACHE_DIR/untracked-$$.txt"
CACHE_TEMP="$CACHE_DIR/worker-$$.tmp"
STAMP_TEMP="$CACHE_DIR/stamp-$$.tmp"
SOURCE="$TARGET_ROOT/debug/agenterm-script"
BOOTSTRAP_DIR="$TARGET_ROOT/task-bootstrap-$$"
WORKER="$BOOTSTRAP_DIR/agenterm-script"

cleanup() {
    rm -f -- "$WORKER" "$IDENTITY_FILE" "$POST_IDENTITY_FILE" \
        "$UNTRACKED_FILE" "$CACHE_TEMP" "$STAMP_TEMP"
    rmdir -- "$BOOTSTRAP_DIR" 2>/dev/null || true
}
trap cleanup EXIT HUP INT TERM

write_identity() {
    output=$1
    {
        printf '%s\n' 'bootstrap_worker_build_schema=2'
        rustc -Vv
        cargo -Vv
        printf '%s\n' 'tracked-index'
        git ls-files -s -- Cargo.toml Cargo.lock build.rs \
            rust-toolchain.toml .cargo crates src docs/agenterm-script-runtime.md \
            assets/agenterm.ico agenterm.tasks.json
        printf '%s\n' 'tracked-worktree'
        git diff --no-ext-diff --binary -- Cargo.toml Cargo.lock build.rs \
            rust-toolchain.toml .cargo crates src docs/agenterm-script-runtime.md \
            assets/agenterm.ico agenterm.tasks.json
        git ls-files --others --exclude-standard -- Cargo.toml Cargo.lock \
            build.rs rust-toolchain.toml .cargo crates src \
            docs/agenterm-script-runtime.md assets/agenterm.ico \
            agenterm.tasks.json > "$UNTRACKED_FILE"
        printf '%s\n' 'untracked-paths'
        cat "$UNTRACKED_FILE"
        printf '%s\n' 'untracked-content'
        git hash-object --stdin-paths < "$UNTRACKED_FILE"
    } > "$output"
}

mkdir -p -- "$CACHE_DIR"
# Compatibility uses source content and tool identities, never timestamps.
write_identity "$IDENTITY_FILE"
AGENTERM_BOOTSTRAP_FINGERPRINT=$(git hash-object -- "$IDENTITY_FILE")
CACHE_VALID=false
if [ -f "$CACHE_WORKER" ] && [ ! -L "$CACHE_WORKER" ] && \
        [ -f "$CACHE_STAMP" ] && [ ! -L "$CACHE_STAMP" ]; then
    stamp_schema=''; stamp_fingerprint=''; stamp_hash=''; stamp_extra=''
    read -r stamp_schema stamp_fingerprint stamp_hash stamp_extra < "$CACHE_STAMP" || true
    actual_hash=$(git hash-object -- "$CACHE_WORKER")
    stamp_content=$(cat "$CACHE_STAMP")
    if [ "$stamp_content" = "1 $stamp_fingerprint $stamp_hash" ] && \
            [ "$stamp_schema" = 1 ] && \
            [ "$stamp_fingerprint" = "$AGENTERM_BOOTSTRAP_FINGERPRINT" ] && \
            [ "$stamp_hash" = "$actual_hash" ] && [ -z "$stamp_extra" ]; then
        CACHE_VALID=true
        cache_hash=$stamp_hash
    fi
fi

if [ "$CACHE_VALID" = true ]; then
    AGENTERM_BOOTSTRAP_CARGO_BUILD_MS=0
    AGENTERM_BOOTSTRAP_WORKER_STATE=reused
    AGENTERM_BOOTSTRAP_LOCK_WAIT_STATE=not_applicable
else
    AGENTERM_BOOTSTRAP_CARGO_START_MS=$(clock_ms)
    cargo build --quiet --locked --bin agenterm-script
    AGENTERM_BOOTSTRAP_CARGO_END_MS=$(clock_ms)
    AGENTERM_BOOTSTRAP_CARGO_BUILD_MS=$((
        AGENTERM_BOOTSTRAP_CARGO_END_MS - AGENTERM_BOOTSTRAP_CARGO_START_MS
    ))
    write_identity "$POST_IDENTITY_FILE"
    post_fingerprint=$(git hash-object -- "$POST_IDENTITY_FILE")
    if [ "$post_fingerprint" != "$AGENTERM_BOOTSTRAP_FINGERPRINT" ]; then
        echo "bootstrap worker inputs changed during build" >&2
        exit 2
    fi
    cp -- "$SOURCE" "$CACHE_TEMP"
    chmod +x "$CACHE_TEMP"
    cache_hash=$(git hash-object -- "$CACHE_TEMP")
    printf '1 %s %s\n' "$AGENTERM_BOOTSTRAP_FINGERPRINT" "$cache_hash" > "$STAMP_TEMP"
    mv -f -- "$CACHE_TEMP" "$CACHE_WORKER"
    mv -f -- "$STAMP_TEMP" "$CACHE_STAMP"
    AGENTERM_BOOTSTRAP_WORKER_STATE=rebuilt
    AGENTERM_BOOTSTRAP_LOCK_WAIT_STATE=included_not_separable
fi

AGENTERM_BOOTSTRAP_COPY_START_MS=$(clock_ms)
mkdir -p -- "$BOOTSTRAP_DIR"
cp -- "$CACHE_WORKER" "$WORKER"
invoked_hash=$(git hash-object -- "$WORKER")
if [ "$invoked_hash" != "$cache_hash" ]; then
    echo "bootstrap worker cache changed during invocation copy" >&2
    exit 2
fi
# macOS uses BSD chmod, whose option parser does not accept a standalone `--`.
# WORKER is rooted under target/ and cannot begin with an option.
chmod +x "$WORKER"
AGENTERM_BOOTSTRAP_COPY_END_MS=$(clock_ms)
AGENTERM_BOOTSTRAP_WORKER_COPY_MS=$((
    AGENTERM_BOOTSTRAP_COPY_END_MS - AGENTERM_BOOTSTRAP_COPY_START_MS
))
AGENTERM_BOOTSTRAP_WORKER="$WORKER"
AGENTERM_BOOTSTRAP_PLATFORM=unix
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
export AGENTERM_BOOTSTRAP_WORKER AGENTERM_BOOTSTRAP_PLATFORM
export AGENTERM_HOST_OS AGENTERM_HOST_ARCH
export AGENTERM_BOOTSTRAP_TIMING_SCHEMA AGENTERM_BOOTSTRAP_CLOCK_RESOLUTION_MS
export AGENTERM_BOOTSTRAP_SETUP_MS AGENTERM_BOOTSTRAP_CARGO_BUILD_MS
export AGENTERM_BOOTSTRAP_WORKER_COPY_MS AGENTERM_BOOTSTRAP_OTHER_SETUP_MS
export AGENTERM_BOOTSTRAP_LOCK_WAIT_STATE AGENTERM_BOOTSTRAP_WORKER_STATE
export AGENTERM_BOOTSTRAP_FINGERPRINT

"$WORKER" task run "$AGENTERM_BOOTSTRAP_TASK" \
    --manifest "$REPO/agenterm.tasks.json" \
    -- "$@"
