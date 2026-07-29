#!/usr/bin/env bash
# Stage dlopened GUI libraries beside a Linux release tarball (no sudo on end hosts).
set -euo pipefail

DEST="${1:?destination directory (e.g. packaging staging root)}"
ARCH="${2:-x86_64}"

case "$ARCH" in
  x86_64)
    LIB_ROOTS=(/usr/lib/x86_64-linux-gnu /lib/x86_64-linux-gnu)
    ;;
  aarch64)
    LIB_ROOTS=(/usr/lib/aarch64-linux-gnu /lib/aarch64-linux-gnu)
    ;;
  *)
    echo "Unsupported Linux arch for GUI lib staging: $ARCH" >&2
    exit 1
    ;;
esac

PKGS=(
  libx11-6
  libx11-xcb1
  libxcb1
  libxcb-xkb1
  libxau6
  libxdmcp6
  libbsd0
  libmd0
  libxkbcommon0
  libxkbcommon-x11-0
  libxfixes3
  libxrender1
  libxcursor1
  libxext6
  libxi6
  libwayland-client0
)

LIB_DIR="$DEST/lib"
mkdir -p "$LIB_DIR"

stage_soname() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    return 0
  fi
  local base
  base="$(basename "$path")"
  cp -Lf "$path" "$LIB_DIR/$base" 2>/dev/null || cp -f "$path" "$LIB_DIR/$base"
}

if command -v dpkg-query >/dev/null 2>&1; then
  for pkg in "${PKGS[@]}"; do
    if ! dpkg-query -W -f='${Status}' "$pkg" 2>/dev/null | grep -q "install ok installed"; then
      echo "warning: package $pkg is not installed; GUI lib bundle may be incomplete" >&2
      continue
    fi
    while IFS= read -r path; do
      stage_soname "$path"
    done < <(dpkg-query -L "$pkg" | grep -E '/[^/]+\.so(\.[0-9]+)*$' || true)
  done
else
  echo "warning: dpkg-query not found; copying common sonames from system lib dirs" >&2
  for root in "${LIB_ROOTS[@]}"; do
    for pattern in \
      libX11.so.6 libX11-xcb.so.1 libxcb.so.1 libxcb-xkb.so.1 libXau.so.6 libXdmcp.so.6 \
      libbsd.so.0 libmd.so.0 libxkbcommon.so.0 libxkbcommon-x11.so.0 libXfixes.so.3 \
      libXrender.so.1 libXcursor.so.1 libXext.so.6 libXi.so.6 libwayland-client.so.0
    do
      stage_soname "$root/$pattern"
    done
  done
fi

if [[ -z "$(find "$LIB_DIR" -maxdepth 1 -name '*.so*' -print -quit)" ]]; then
  echo "No GUI libraries were staged into $LIB_DIR" >&2
  exit 1
fi

echo "==> staged $(find "$LIB_DIR" -maxdepth 1 -name '*.so*' | wc -l) libraries into $LIB_DIR"
