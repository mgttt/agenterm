#!/usr/bin/env bash
# Package Unix client release binaries (cli, mux, script) into a tar.gz archive.
#
# Usage: package-client-release.sh VERSION OS ARCH BIN_DIR
# Example: package-client-release.sh 0.1.8 linux x86_64 target/x86_64-unknown-linux-gnu/release
set -euo pipefail

VERSION="${1:?VERSION required}"
OS="${2:?OS required}"
ARCH="${3:?ARCH required}"
BIN_DIR="${4:?BIN_DIR required}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST="$ROOT/scripts/artifacts.json"
DIST="$ROOT/dist"
STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT

if [[ ! -d "$BIN_DIR" ]]; then
  echo "Binary directory not found: $BIN_DIR" >&2
  exit 1
fi

EXEC_NAMES=()
while IFS= read -r name; do
  EXEC_NAMES+=("$name")
done < <(
  python3 - "$MANIFEST" "$OS" "$ARCH" <<'PY'
import json
import sys

manifest_path, os_name, arch = sys.argv[1:4]
with open(manifest_path, encoding="utf-8") as handle:
    manifest = json.load(handle)

platforms = manifest.get("platforms") or []
matches = [
    platform
    for platform in platforms
    if platform.get("os") == os_name and platform.get("arch") == arch
]
if len(matches) != 1:
    sys.exit(f"Expected one platform entry for {os_name}-{arch}, found {len(matches)}")

for entry in matches[0].get("executables") or []:
    name = entry.get("name")
    if not name:
        sys.exit("Platform executable entry is missing name")
    print(name)
PY
)

if [[ ${#EXEC_NAMES[@]} -eq 0 ]]; then
  echo "No executables listed for platform $OS-$ARCH in $MANIFEST" >&2
  exit 1
fi

mkdir -p "$DIST"

for name in "${EXEC_NAMES[@]}"; do
  src="$BIN_DIR/$name"
  if [[ ! -f "$src" ]]; then
    echo "Missing binary: $src" >&2
    exit 1
  fi
  cp "$src" "$STAGING/$name"
  chmod +x "$STAGING/$name"
done

for license_file in LICENSE-APACHE LICENSE-MIT THIRD_PARTY_NOTICES.md; do
  cp "$ROOT/$license_file" "$STAGING/"
done

ARCHIVE="$DIST/agenterm-$VERSION-$OS-$ARCH.tar.gz"
tar -czf "$ARCHIVE" -C "$STAGING" .

echo "==> packaged $ARCHIVE"
