#!/usr/bin/env sh
set -eu
AGENTERM_BOOTSTRAP_TASK=client-build
export AGENTERM_BOOTSTRAP_TASK
profile=${AGENTERM_BUILD_PROFILE:-dev}
[ "$profile" = debug ] && profile=dev
arch=${ARCH:-x86_64}
[ "$arch" = arm64 ] && arch=aarch64
exec "$(dirname "$0")/bootstrap.sh" "$profile" \
    --target "$arch-pc-windows-msvc" --driver cargo-xwin
