#!/usr/bin/env sh
# Prune stale rustc incremental-compilation caches.
#
# Cargo garbage-collects old *sessions* inside one crate-unit directory, but it
# never removes the crate-unit directories themselves. Every change to a unit's
# fingerprint — a different feature set, a new rustc, an extra RUSTFLAG, a
# `--bin X` build versus a whole-workspace one — mints a fresh
# `<crate>-<hash>` directory and orphans the previous one forever.
#
# Age alone does not catch this. Measured on this repo: 249 units, 12 GB, and
# *every one* modified within three days, because two agents build it under
# varying feature sets all day. 44 of them were `agenterm-*` at ~450 MB each,
# 9.4 GB for one crate. So the primary rule is per-crate retention: keep the
# few most recently used fingerprints of each crate and drop the rest. Age is
# kept as a secondary sweep for crates that stopped being built at all.
#
# Deleting these is safe: the only cost of a wrong guess is that one crate unit
# recompiles from scratch next time.
#
# Never fails the caller — a cleanup problem must not break a build.
set -u

# Fingerprints to retain per crate. Two covers the common case of alternating
# between two build shapes (say a full build and a single `--bin`) without
# either one losing its cache.
KEEP="${AGENTERM_INCREMENTAL_KEEP:-2}"
AGE_DAYS="${AGENTERM_INCREMENTAL_MAX_AGE_DAYS:-3}"

# Opt out entirely, e.g. when bisecting and every rebuild matters.
if [ "${AGENTERM_SKIP_INCREMENTAL_PRUNE:-}" != "" ]; then
    exit 0
fi

REPO="$(git rev-parse --show-toplevel 2>/dev/null)" || exit 0
[ -n "$REPO" ] || exit 0

removed=0

for incremental in "$REPO"/target*/*/incremental; do
    [ -d "$incremental" ] || continue

    # Crate name is the unit directory minus its trailing `-<hash>`.
    crates="$(ls -1 "$incremental" 2>/dev/null | sed 's/-[^-]*$//' | sort -u)"
    for crate in $crates; do
        [ -n "$crate" ] || continue
        kept=0
        # -t sorts newest first, so the caches an active build loop is using
        # are the ones retained.
        for unit in $(ls -1dt "$incremental/$crate"-* 2>/dev/null); do
            [ -d "$unit" ] || continue
            kept=$((kept + 1))
            if [ "$kept" -le "$KEEP" ]; then
                continue
            fi
            if rm -rf "$unit" 2>/dev/null; then
                removed=$((removed + 1))
            fi
        done
    done

    # Secondary sweep: whole crates nobody builds any more still hold up to
    # KEEP fingerprints each, which adds up across a workspace this size.
    for unit in $(find "$incremental" -mindepth 1 -maxdepth 1 -type d -mtime "+$AGE_DAYS" 2>/dev/null); do
        if rm -rf "$unit" 2>/dev/null; then
            removed=$((removed + 1))
        fi
    done
done

if [ "$removed" -gt 0 ]; then
    printf 'pruned %d stale incremental cache unit(s)\n' "$removed" >&2
fi

exit 0
