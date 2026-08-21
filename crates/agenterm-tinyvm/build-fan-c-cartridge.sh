#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
  echo "usage: build-fan-c-cartridge.sh OUTPUT.wasm" >&2
  exit 2
fi

crate_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$crate_dir/../.." && pwd)
raw=$(mktemp "${TMPDIR:-/tmp}/tinyarcade-fan-c.XXXXXX")
trap 'rm -f -- "$raw"' EXIT HUP INT TERM
CARGO=${CARGO:-cargo}

"$crate_dir/build-c-cartridge.sh" \
  "$crate_dir/tests/fixtures/fan-c-cartridge.c" \
  "$raw" >/dev/null
if [ -n "${TINYVM_BIN:-}" ]; then
  "$TINYVM_BIN" cartridge attach-manifest \
    "$raw" "$1" org.example.fan-c-cartridge 0.1.0 1 1
else
  "$CARGO" run -q --manifest-path "$repo_dir/Cargo.toml" -p agenterm-tinyvm -- \
    cartridge attach-manifest \
    "$raw" \
    "$1" \
    org.example.fan-c-cartridge \
    0.1.0 \
    1 \
    1
fi

printf '%s\n' "$1"
