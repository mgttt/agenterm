#!/usr/bin/env bash
# Launch Reasonix desktop with WebKit AT-SPI tree visible to cu.
# Without WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1 the web process aborts
# (signal 6) and cu tree is only GTK fillers.
# WebKit 2.52 exposes AT-SPI Text on the composer <textarea> but never
# EditableText; Component.ScrollTo returns true without moving geometry.
# The eval helper implements the missing set-value and scrollIntoView on
# the GTK thread so cu send-text --name can confirm via Text.GetText and
# cu scroll --name can confirm via independent Component.GetExtents.
# Text.SetSelection / GetNSelections / GetSelection already work on that
# textarea — do not add an eval-helper select path.
# Text.SetCaretOffset / CaretOffset (GetCaretOffset) already work too —
# do not add an eval-helper caret path.
# Extra args are passed through.
set -euo pipefail
export WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1

here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
src="$here/lib/webkit_a11y_eval.c"
runtime="${XDG_RUNTIME_DIR:-/tmp}"
so="${AGENTERM_WEBKIT_EVAL_SO:-$runtime/libagenterm_webkit_eval.so}"
sock="${PLATFORM_WEBKIT_EVAL_SOCK:-$runtime/agenterm-webkit-eval.sock}"
if [[ -f "$src" ]] && command -v gcc >/dev/null 2>&1; then
  if [[ ! -f "$so" || "$src" -nt "$so" ]]; then
    gcc -shared -fPIC -O2 -ldl -lpthread -o "$so" "$src"
  fi
  export LD_PRELOAD="$so${LD_PRELOAD:+:$LD_PRELOAD}"
  export PLATFORM_WEBKIT_EVAL_SOCK="$sock"
fi
exec "${REASONIX_DESKTOP:-/usr/bin/reasonix-desktop}" "$@"
