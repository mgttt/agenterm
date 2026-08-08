#!/usr/bin/env bash
# Scan text files for host-absolute home paths and common credential leaks.
# Docs must use repo-relative paths or ~/... only (see Agents.md Document redaction).
set -euo pipefail

if [[ $# -lt 1 ]]; then
  echo "usage: $0 <path> [path...]" >&2
  exit 2
fi

# Match host home forms that must become ~/... (see Agents.md conversion table):
# Darwin /Users/, Linux /home/, Windows C:\Users\ and %USERPROFILE% / $env:USERPROFILE
pattern='(^|[^~])(/Users/|/home/|[A-Za-z]:\\Users\\)|%USERPROFILE%|%UserProfile%|\$env:USERPROFILE|@gmail\.com|@qq\.com|@163\.com'

if command -v rg >/dev/null 2>&1; then
  if rg -nE "$pattern" --glob '!target/**' --glob '!**/node_modules/**' "$@"; then
    echo "doc-redact-check: hits above must become repo-relative or ~/..." >&2
    exit 1
  fi
else
  # grep -E fallback (no globs); callers should prefer rg when available
  if grep -nE "$pattern" "$@" 2>/dev/null; then
    echo "doc-redact-check: hits above must become repo-relative or ~/..." >&2
    exit 1
  fi
fi

echo "doc-redact-check: clean"
exit 0
