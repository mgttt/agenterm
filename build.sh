#!/usr/bin/env sh
set -eu
AGENTERM_BOOTSTRAP_TASK=build
export AGENTERM_BOOTSTRAP_TASK
exec "$(dirname "$0")/scripts/bootstrap.sh" "$@"
