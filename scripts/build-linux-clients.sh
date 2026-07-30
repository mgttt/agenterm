#!/usr/bin/env sh
set -eu
AGENTERM_BOOTSTRAP_TASK=client-build
export AGENTERM_BOOTSTRAP_TASK
profile=${AGENTERM_BUILD_PROFILE:-dev}
[ "$profile" = debug ] && profile=dev
exec "$(dirname "$0")/bootstrap.sh" "$profile" \
    --target "${AGENTERM_LINUX_TARGET:-x86_64-unknown-linux-gnu}"
