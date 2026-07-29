#!/usr/bin/env bash
# Build native Linux binaries (gui, cli, mux, script).
set -euo pipefail

TARGET="${AGENTERM_LINUX_TARGET:-x86_64-unknown-linux-gnu}"
PROFILE="${AGENTERM_BUILD_PROFILE:-debug}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

BINS=(agenterm agenterm-cli agenterm-mux agenterm-script agenterm-mcp)
ARGS=(build --target "$TARGET")
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
  echo "    $OUT/$bin"
done
