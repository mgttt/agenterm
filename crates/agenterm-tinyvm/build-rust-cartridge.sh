#!/bin/sh
set -eu

if [ "$#" -ne 3 ]; then
  echo "usage: build-rust-cartridge.sh SOURCE.rs CRATE_NAME OUTPUT.wasm" >&2
  exit 2
fi

crate_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH='' cd -- "$crate_dir/../.." && pwd)
source=$1
crate_name=$2
output=$3

if [ ! -f "$source" ]; then
  echo "cartridge source is not a regular file: $source" >&2
  exit 2
fi

mkdir -p "$(dirname -- "$output")"
rustup_bin=${RUSTUP_BIN:-$(command -v rustup || true)}
if [ -z "$rustup_bin" ]; then
  rustup_bin="${HOME}/.cargo/bin/rustup"
fi
wasm_opt=${WASM_OPT:-$(command -v wasm-opt || true)}
if [ -z "$wasm_opt" ] && [ -x /opt/homebrew/opt/binaryen/bin/wasm-opt ]; then
  wasm_opt=/opt/homebrew/opt/binaryen/bin/wasm-opt
fi
if [ -z "$wasm_opt" ]; then
  echo "wasm-opt is required (install Binaryen or set WASM_OPT)" >&2
  exit 1
fi
raw=$(mktemp "${TMPDIR:-/tmp}/tinyarcade-cartridge.XXXXXX")
trap 'rm -f "$raw"' EXIT HUP INT TERM

"$rustup_bin" run 1.97.0 rustc \
  --edition=2024 \
  --target wasm32-unknown-unknown \
  --crate-name "$crate_name" \
  --crate-type cdylib \
  --remap-path-prefix "$repo_dir"=. \
  -C opt-level=z \
  -C lto=fat \
  -C codegen-units=1 \
  -C panic=abort \
  -C target-feature=-bulk-memory,-reference-types,-multivalue,-sign-ext,-nontrapping-fptoint,-simd128 \
  -C link-arg=--export-memory \
  "$source" \
  -o "$raw"

# Keep Rust's standard bulk memory.copy/fill instructions. The cartridge
# profile still disables passive segments/reference types and the other broad
# post-MVP feature families above; tinyvm meters copied/filled bytes as fuel.
"$wasm_opt" "$raw" \
  --enable-bulk-memory \
  --enable-mutable-globals \
  --disable-reference-types \
  --disable-multivalue \
  --disable-sign-ext \
  --disable-nontrapping-float-to-int \
  --disable-simd \
  --strip-debug \
  --strip-producers \
  -Oz \
  -o "$output"

printf '%s\n' "$output"
