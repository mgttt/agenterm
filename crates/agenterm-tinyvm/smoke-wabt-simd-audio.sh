#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
CARGO=${CARGO:-cargo}
WAT2WASM=${WAT2WASM:-wat2wasm}
WASM_VALIDATE=${WASM_VALIDATE:-wasm-validate}
H5_BROWSER=${H5_BROWSER:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}
WASM="$TEMP/simd-audio-mix-v1.wasm"
ORACLE="$TEMP/SimdAudioOracle"
DOM="$TEMP/simd-audio.html"
LOG="$TEMP/simd-audio-browser.log"

test -x "$H5_BROWSER"
"$WAT2WASM" "$CRATE/tests/fixtures/simd-audio-mix-v1.wat" -o "$WASM"
"$WASM_VALIDATE" "$WASM"

TINYVM_CORE_FEATURES=staticcore,simd \
TINYVM_CORE_MAX_BYTES=122880 \
TINYVM_CORE_LIMIT_LABEL='120 KiB optional SIMD' \
CARGO="$CARGO" CARGO_TARGET_DIR="$TEMP/core-target" \
  "$CRATE/measure-core.sh"

TINYVM_WABT_SIMD_WASM="$WASM" "$CARGO" test -q -p agenterm-tinyvm \
  --features simd --test wabt_simd_audio_oracle \
  wabt_compiled_simd_audio_and_masks_match_tinyvm -- --ignored --exact

xcrun swiftc -parse-as-library -warnings-as-errors -O -framework JavaScriptCore \
  "$CRATE/tests/webkit/SimdAudioOracle.swift" -o "$ORACLE"
"$ORACLE" "$WASM"

encoded=$(base64 <"$WASM" | tr -d '\n' | tr '+/' '-_' | tr -d '=')
"$H5_BROWSER" --headless=new --disable-gpu --no-first-run --disable-default-apps \
  --allow-file-access-from-files --user-data-dir="$TEMP/browser-profile" \
  --virtual-time-budget=5000 --dump-dom \
  "file://$CRATE/tests/webkit/SimdAudioH5Oracle.html#wasm=$encoded" >"$DOM" 2>"$LOG" &
browser_pid=$!
attempts=0
while [ "$attempts" -lt 200 ]; do
  if grep -Eq 'data-status="(pass|fail)"' "$DOM"; then break; fi
  if ! kill -0 "$browser_pid" 2>/dev/null; then break; fi
  sleep 0.1
  attempts=$((attempts + 1))
done
if kill -0 "$browser_pid" 2>/dev/null; then kill "$browser_pid"; fi
wait "$browser_pid" 2>/dev/null || true
if ! grep -Fq 'data-status="pass"' "$DOM"; then
  sed -n '1,120p' "$DOM" >&2
  sed -n '1,120p' "$LOG" >&2
  exit 1
fi
sed -n 's/.*\(OK: H5 SIMD audio\/mask=[^<]*\).*/\1/p' "$DOM"
echo 'OK: WABT, tinyvm, JavaScriptCore and H5 agree on SIMD audio and whole-vector masks'
