#!/bin/sh
set -eu

if [ "$#" -ne 1 ]; then
  echo "usage: build-simd-audio-cartridge.sh OUTPUT.wasm" >&2
  exit 2
fi

CRATE=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
ROOT=$(CDPATH='' cd -- "$CRATE/../.." && pwd)
RAW=$(mktemp "${TMPDIR:-/tmp}/tinyarcade-simd-audio.XXXXXX")
trap 'rm -f -- "$RAW"' EXIT HUP INT TERM
CARGO=${CARGO:-cargo}
WAT2WASM=${WAT2WASM:-wat2wasm}

"$WAT2WASM" "$CRATE/tests/fixtures/simd-audio-cartridge-v1.wat" -o "$RAW"
"$CARGO" run -q --manifest-path "$ROOT/Cargo.toml" -p agenterm-tinyvm \
  --features simd -- cartridge attach-manifest \
  "$RAW" "$1" org.example.simd-audio 0.1.0 1 1
printf '%s\n' "$1"
