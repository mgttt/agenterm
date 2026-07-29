#!/usr/bin/env bash
# Package Unix release binaries using the format declared by the platform
# manifest (tar.gz for Linux, ZIP for macOS notarization).
#
# Usage: package-client-release.sh VERSION OS ARCH BIN_DIR
# Example: package-client-release.sh 0.1.8 linux x86_64 target/x86_64-unknown-linux-gnu/release
set -euo pipefail

VERSION="${1:?VERSION required}"
OS="${2:?OS required}"
ARCH="${3:?ARCH required}"
BIN_DIR="${4:?BIN_DIR required}"
PYTHON="${PYTHON:-python3}"

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST="$ROOT/scripts/artifacts.json"
DIST="${AGENTERM_PACKAGE_DIST:-$ROOT/dist}"
STAGING="$(mktemp -d)"
trap 'rm -rf "$STAGING"' EXIT

if [[ ! -d "$BIN_DIR" ]]; then
  echo "Binary directory not found: $BIN_DIR" >&2
  exit 1
fi

EXEC_NAMES=()
PACKAGE_FORMAT=""
PLATFORM_ROWS="$STAGING/platform-rows.tsv"
"$PYTHON" - "$MANIFEST" "$OS" "$ARCH" >"$PLATFORM_ROWS" <<'PY'
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

platform = matches[0]
package = platform.get("package")
if package not in {"tar.gz", "zip"}:
    sys.exit(f"Unsupported package format for {os_name}-{arch}: {package}")
print(f"package\t{package}")
for entry in platform.get("executables") or []:
    name = entry.get("name")
    if not name:
        sys.exit("Platform executable entry is missing name")
    print(f"executable\t{name}")
PY

while IFS=$'\t' read -r kind value; do
  value="${value%$'\r'}"
  if [[ "$kind" == "package" ]]; then
    PACKAGE_FORMAT="$value"
  elif [[ "$kind" == "executable" ]]; then
    EXEC_NAMES+=("$value")
  fi
done <"$PLATFORM_ROWS"

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

if [[ "$OS" == "macos" && "${AGENTERM_MACOS_UNSIGNED_PREVIEW:-0}" == "1" ]]; then
  cp "$ROOT/docs/macos-unsigned-preview.md" \
    "$STAGING/MACOS_UNSIGNED_PREVIEW_README.md"
  cp "$ROOT/docs/macos-unsigned-preview.zh-Hant.md" \
    "$STAGING/MACOS_UNSIGNED_PREVIEW_README.zh-Hant.md"
fi

case "$PACKAGE_FORMAT" in
  tar.gz)
    ARCHIVE="$DIST/agenterm-$VERSION-$OS-$ARCH.tar.gz"
    tar -czf "$ARCHIVE" -C "$STAGING" .
    ;;
  zip)
    ARCHIVE="$DIST/agenterm-$VERSION-$OS-$ARCH.zip"
    ZIP_STAGING="$STAGING" \
      ZIP_ARCHIVE="$ARCHIVE" \
      ZIP_EXECUTABLES="${EXEC_NAMES[*]}" \
      "$PYTHON" - <<'PY'
import os
import pathlib
import stat
import zipfile

staging = pathlib.Path(os.environ["ZIP_STAGING"])
archive = pathlib.Path(os.environ["ZIP_ARCHIVE"])
executables = set(os.environ["ZIP_EXECUTABLES"].split())
with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as output:
    for path in sorted(item for item in staging.rglob("*") if item.is_file()):
        relative = path.relative_to(staging).as_posix()
        info = zipfile.ZipInfo.from_file(path, relative)
        info.create_system = 3
        mode = 0o755 if relative in executables else 0o644
        info.external_attr = (stat.S_IFREG | mode) << 16
        info.compress_type = zipfile.ZIP_DEFLATED
        output.writestr(info, path.read_bytes())
PY
    ;;
  *)
    echo "Unsupported package format: $PACKAGE_FORMAT" >&2
    exit 1
    ;;
esac

echo "==> packaged $ARCHIVE"
