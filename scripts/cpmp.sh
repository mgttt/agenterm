#!/usr/bin/env bash
# cpmp.sh — Commit-Pull-Merge-Push (CPMP): one-shot, race-tolerant git sync.
#
# Encoded habit: commit local work -> backup branch -> fetch+merge remote ->
# push. If another agent pushes inside the window, the fetch/merge/push cycle
# retries immediately. On a real merge conflict the merge is aborted, local
# state returns to the backup commit, and recovery commands are printed.
#
# Usage:
#   scripts/cpmp.sh -m "message" [--branch main] [--remote origin]
#                   [--retries N] [--ours | --theirs]
#
# Exit codes: 0 synced, 2 merge conflict (aborted, see output), 1 other error.

set -uo pipefail

REMOTE=origin
BRANCH=""
MESSAGE=""
RETRIES=8
XSTRAT=""

die() { printf 'cpmp: error: %s\n' "$*" >&2; exit 1; }

while [[ $# -gt 0 ]]; do
  case "$1" in
    -m|--message) MESSAGE="${2:?}"; shift 2 ;;
    -b|--branch)  BRANCH="${2:?}";  shift 2 ;;
    -r|--remote)  REMOTE="${2:?}";  shift 2 ;;
    --retries)    RETRIES="${2:?}"; shift 2 ;;
    --ours)       XSTRAT=ours;  shift ;;
    --theirs)     XSTRAT=theirs; shift ;;
    -h|--help)    sed -n '2,14p' "$0"; exit 0 ;;
    *) die "unknown argument: $1" ;;
  esac
done

git rev-parse --show-toplevel >/dev/null 2>&1 || die "not inside a git repository"

CURRENT=$(git symbolic-ref --short -q HEAD || true)
if [[ -z $CURRENT ]]; then
  [[ -n $BRANCH ]] || die "detached HEAD; pass --branch <name>"
  git checkout "$BRANCH" || die "cannot checkout $BRANCH"
else
  [[ -z $BRANCH || $BRANCH == "$CURRENT" ]] || die "on '$CURRENT' but --branch '$BRANCH' given"
  BRANCH=$CURRENT
fi

# 1. Commit local work (requires an explicit message).
if [[ -n $(git status --porcelain) ]]; then
  [[ -n $MESSAGE ]] || die "working tree dirty; pass -m \"message\""
  git add -A || die "git add failed"
  git diff --cached --quiet || git commit -m "$MESSAGE" || die "git commit failed"
fi

# 2. Backup: cheap branch ref at the pre-pull commit.
TS=$(date -u +%Y%m%d-%H%M%S)
BACKUP="cpmp/${BRANCH//\//-}-$TS"
git branch -f "$BACKUP" HEAD || die "backup branch failed"
printf 'cpmp: backup -> %s (%s)\n' "$BACKUP" "$(git rev-parse --short HEAD)"

# 3+4. Fetch+merge, then push; re-merge and retry when raced.
merge_remote() { # returns: 0 merged, 3 fetch error, anything else = merge failure
  git fetch "$REMOTE" "$BRANCH" || return 3
  local opts=(--no-edit)
  [[ -n $XSTRAT ]] && opts+=(-X "$XSTRAT")
  git merge "${opts[@]}" "$REMOTE/$BRANCH"
}

backoff=4
attempt=0
while (( attempt < RETRIES )); do
  attempt=$((attempt + 1))
  merge_remote
  rc=$?
  if (( rc == 3 )); then
    printf 'cpmp: fetch failed; retry in %ds (%d/%d)\n' "$backoff" "$attempt" "$RETRIES" >&2
    sleep "$backoff"; backoff=$(( backoff < 32 ? backoff * 2 : 32 ))
    continue
  elif (( rc != 0 )); then
    CONFLICTS=$(git diff --name-only --diff-filter=U)
    git merge --abort 2>/dev/null || true
    printf 'cpmp: merge conflict with %s/%s — merge aborted, local state intact.\n' "$REMOTE" "$BRANCH" >&2
    printf 'cpmp: conflicting files:\n%s\n' "${CONFLICTS:-  (unknown)}" >&2
    printf 'cpmp: backup ref: %s\n' "$BACKUP" >&2
    printf 'cpmp: recover with: git merge %s   (or: git reset --hard %s)\n' "$BACKUP" "$BACKUP" >&2
    exit 2
  fi

  PUSH_OUT=$(git push "$REMOTE" "HEAD:$BRANCH" 2>&1)
  if [[ $? -eq 0 ]]; then
    printf 'cpmp: done — %s == %s (%s)\n' "$BRANCH" "$REMOTE/$BRANCH" "$(git rev-parse --short HEAD)"
    exit 0
  fi
  printf '%s\n' "$PUSH_OUT" >&2
  if grep -qi 'non-fast-forward\|fetch first\|stale info' <<<"$PUSH_OUT"; then
    printf 'cpmp: raced by another push; re-merging now (%d/%d)\n' "$attempt" "$RETRIES" >&2
  else
    printf 'cpmp: push failed; retry in %ds (%d/%d)\n' "$backoff" "$attempt" "$RETRIES" >&2
    sleep "$backoff"; backoff=$(( backoff < 32 ? backoff * 2 : 32 ))
  fi
done

die "still not pushed after $RETRIES attempts; local work safe at $BACKUP"
