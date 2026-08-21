#!/bin/sh
set -eu

CRATE=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
TINYVM_XCFRAMEWORK_FEATURES=ios-wasi-host \
TINYVM_XCFRAMEWORK_HEADERS="$CRATE/include-wasi-host" \
  exec "$CRATE/build-xcframework.sh" "$@"
