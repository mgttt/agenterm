#!/usr/bin/env bash
# Reset this repository to a credential-free GitHub remote and persist one
# browser login through Git Credential Manager.
#
# Usage:
#   bash scripts/git_auth_reset.sh [github-username]
#
# This script never accepts, prints, or writes a PAT into the remote URL.
set -euo pipefail

USERNAME="${1:-mgttt}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ORIGIN_URL="https://github.com/mgttt/agenterm.git"

if ! command -v git >/dev/null 2>&1; then
  echo "git is required." >&2
  exit 1
fi
if ! git credential-manager --version >/dev/null 2>&1; then
  echo "Git Credential Manager is required." >&2
  exit 1
fi
if [[ ! "$USERNAME" =~ ^[A-Za-z0-9-]+$ ]]; then
  echo "Invalid GitHub username: $USERNAME" >&2
  exit 2
fi

case "$(uname -s)" in
  MINGW*|MSYS*|CYGWIN*)
    cd "$REPO_ROOT"
    REPO_ROOT="$(pwd -W)"
    git config --global credential.credentialStore wincredman
    ;;
  *)
    echo "This helper currently configures persistent storage only on Windows." >&2
    echo "Run it from Git Bash, MSYS2, or Cygwin on the AgenTerm Windows host." >&2
    exit 2
    ;;
esac

if [[ "$(git rev-parse --show-toplevel)" != "$REPO_ROOT" ]]; then
  echo "Refusing to modify authentication outside the AgenTerm Git root." >&2
  exit 1
fi

git config --global credential.https://github.com.username "$USERNAME"
git config --global credential.https://github.com.useHttpPath false
git remote set-url origin "$ORIGIN_URL"

echo "Opening GitHub browser authentication for $USERNAME ..."
git credential-manager github login \
  --username "$USERNAME" \
  --browser \
  --force

echo "Verifying the persisted credential without changing the repository ..."
git ls-remote --exit-code origin HEAD >/dev/null

echo
echo "GitHub authentication is stored in Windows Credential Manager."
echo "Origin: $ORIGIN_URL"
echo "Verification: PASS"
