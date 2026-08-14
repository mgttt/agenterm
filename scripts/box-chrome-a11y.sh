#!/usr/bin/env bash
# Always launch box Chrome with AT-SPI renderer tree for cu.
# Extra args are passed through. Do not add a second --force-renderer-accessibility.
#
# box-chrome rewrites XDG_RUNTIME_DIR to /tmp/xdg-runtime-box-$DISPLAY, so
# atk-bridge misses $XDG_RUNTIME_DIR/at-spi/bus and then asks
# org.a11y.Bus.GetAddress on a session bus that later daemons may share.
# Pin AT_SPI_BUS_ADDRESS (and the standard bus file) to the host socket
# the observer already set as AT_SPI_BUS / AT_SPI_BUS_ADDRESS.
set -euo pipefail

if [ -z "${AT_SPI_BUS_ADDRESS:-}" ] && [ -n "${AT_SPI_BUS:-}" ]; then
  AT_SPI_BUS_ADDRESS="${AT_SPI_BUS}"
fi
if [ -n "${AT_SPI_BUS_ADDRESS:-}" ]; then
  AT_SPI_BUS_ADDRESS="${AT_SPI_BUS_ADDRESS%%,guid=*}"
  export AT_SPI_BUS_ADDRESS
  export AT_SPI_BUS="${AT_SPI_BUS:-$AT_SPI_BUS_ADDRESS}"
  box_display_num="${DISPLAY#:}"
  box_display_num="${box_display_num%%.*}"
  if [ "${box_display_num:-1}" -ge 2 ] 2>/dev/null; then
    chrome_xdg="/tmp/xdg-runtime-box-${box_display_num}"
  else
    chrome_xdg="/tmp/xdg-runtime-box"
  fi
  mkdir -p "${chrome_xdg}/at-spi"
  printf '%s\n' "${AT_SPI_BUS_ADDRESS}" > "${chrome_xdg}/at-spi/bus"
fi
export QT_ACCESSIBILITY="${QT_ACCESSIBILITY:-1}"
export GNOME_ACCESSIBILITY="${GNOME_ACCESSIBILITY:-1}"
export ACCESSIBILITY_ENABLED="${ACCESSIBILITY_ENABLED:-1}"

exec /usr/local/bin/box-chrome --force-renderer-accessibility "$@"
