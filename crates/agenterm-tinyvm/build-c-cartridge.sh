#!/bin/sh
set -eu

if [ "$#" -ne 2 ]; then
  echo "usage: build-c-cartridge.sh SOURCE.c OUTPUT.wasm" >&2
  exit 2
fi

crate_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
source=$1
output=$2
if [ ! -f "$source" ]; then
  echo "cartridge source is not a regular file: $source" >&2
  exit 2
fi

clang_bin=${CLANG:-$(command -v clang || true)}
if [ -x /opt/homebrew/opt/llvm/bin/clang ]; then
  clang_bin=${CLANG:-/opt/homebrew/opt/llvm/bin/clang}
fi
if [ -z "$clang_bin" ]; then
  echo "a clang with the wasm32 backend is required (or set CLANG)" >&2
  exit 1
fi

mkdir -p "$(dirname -- "$output")"
"$clang_bin" \
  --target=wasm32-unknown-unknown \
  -std=c17 \
  -Oz \
  -ffreestanding \
  -fno-builtin \
  -nostdlib \
  -I "$crate_dir/include" \
  -Wl,--no-entry \
  -Wl,--export-memory \
  -Wl,--initial-memory=65536 \
  -Wl,--max-memory=65536 \
  -Wl,-z,stack-size=4096 \
  -Wl,--strip-all \
  "$source" \
  -o "$output"

printf '%s\n' "$output"
