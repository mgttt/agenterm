#!/usr/bin/env sh
set -eu
AGENTERM_BOOTSTRAP_TASK=lint
export AGENTERM_BOOTSTRAP_TASK
exec "$(dirname "$0")/scripts/bootstrap.sh" "$@"
