#!/usr/bin/env bash
# Build all Windows ARM64 binaries via cargo-xwin on Linux/macOS hosts.
#
# Rust toolchain: install the aarch64-pc-windows-msvc target (e.g. rustup target
# add aarch64-pc-windows-msvc). It is not yet pinned in rust-toolchain.toml;
# add it there when the CI multi-arch matrix lands.
set -euo pipefail

TARGET="aarch64-pc-windows-msvc"
PROFILE="${AGENTERM_BUILD_PROFILE:-debug}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BINS=(agenterm agenterm-server agenterm-cli agenterm-mux agenterm-script)
ARGS=(xwin build --target "$TARGET")
if [[ $PROFILE == release ]]; then
  ARGS+=(--release)
fi
for bin in "${BINS[@]}"; do
  ARGS+=(--bin "$bin")
done

echo "==> cargo ${ARGS[*]}"
cargo "${ARGS[@]}"

OUT="target/$TARGET/$PROFILE"
echo "==> built:"
for bin in "${BINS[@]}"; do
  echo "    $OUT/$bin.exe"
done
