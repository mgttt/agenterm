#!/bin/sh
set -eu

crate_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH= cd -- "$crate_dir/../.." && pwd)
output=${1:-"$repo_dir/target/tinyvm-paddle-guard/paddle-guard-0.1.0.wasm"}

"$crate_dir/build-rust-cartridge.sh" \
  "$crate_dir/guests/paddle-guard/paddle_guard.rs" \
  paddle_guard \
  "$output"
