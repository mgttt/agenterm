#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
  echo "usage: build-async-completion-cartridge.sh OUTPUT.wasm" >&2
  exit 2
fi

crate_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$crate_dir/../.." && pwd)
raw=$(mktemp "${TMPDIR:-/tmp}/tinyarcade-async-completion.XXXXXX")
trap 'rm -f -- "$raw"' EXIT HUP INT TERM
WAT2WASM=${WAT2WASM:-$(command -v wat2wasm || true)}
CARGO=${CARGO:-cargo}

if [ -z "$WAT2WASM" ]; then
  echo "wat2wasm is required (or set WAT2WASM)" >&2
  exit 1
fi
mkdir -p "$(dirname -- "$1")"
"$WAT2WASM" "$crate_dir/tests/fixtures/async-completion-v1.wat" -o "$raw"
if [ -n "${TINYVM_BIN:-}" ]; then
  "$TINYVM_BIN" cartridge attach-manifest \
    "$raw" "$1" org.example.async-completion 0.1.0 1 1
else
  "$CARGO" run -q --manifest-path "$repo_dir/Cargo.toml" -p agenterm-tinyvm -- \
    cartridge attach-manifest \
    "$raw" \
    "$1" \
    org.example.async-completion \
    0.1.0 \
    1 \
    1
fi

printf '%s\n' "$1"
