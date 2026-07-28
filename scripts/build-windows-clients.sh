#!/usr/bin/env bash
# Build all four Windows binaries via cargo-xwin (x86_64 default, ARCH=arm64 for ARM64).
#
# Rust toolchain targets:
#   x86_64 (default): x86_64-pc-windows-msvc (pinned in rust-toolchain.toml)
#   arm64:            aarch64-pc-windows-msvc (install manually until CI matrix pins it)
set -euo pipefail

ARCH="${ARCH:-x86_64}"
PROFILE="${AGENTERM_BUILD_PROFILE:-debug}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

case "$ARCH" in
  x86_64|amd64)
    TARGET="x86_64-pc-windows-msvc"
    ;;
  arm64|aarch64)
    exec "$ROOT/scripts/build-windows-arm64.sh"
    ;;
  *)
    echo "unsupported ARCH=$ARCH (use x86_64 or arm64)" >&2
    exit 1
    ;;
esac

cd "$ROOT"

BINS=(agenterm agenterm-cli agenterm-mux agenterm-script)
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
