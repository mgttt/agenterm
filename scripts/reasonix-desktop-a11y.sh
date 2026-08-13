#!/usr/bin/env bash
# Launch Reasonix desktop with WebKit AT-SPI tree visible to cu.
# Without WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 the web process aborts
# (signal 6) and cu tree is only GTK fillers.
# Extra args are passed through.
set -euo pipefail
export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1
exec "${REASONIX_DESKTOP:-/usr/bin/reasonix-desktop}" "$@"
