#!/bin/sh
set -eu

ROOT=$(unset CDPATH; cd -- "$(dirname -- "$0")/../.." && pwd)
CRATE="$ROOT/crates/agenterm-tinyvm"
TEMP=$(mktemp -d)
trap 'rm -rf -- "$TEMP"' EXIT HUP INT TERM
CARGO=${CARGO:-cargo}
ORACLE="$TEMP/TinyArcadeWebKitOracle"
ADAPTER="$CRATE/tests/webkit/TinyArcadeWebKitOracle.js"
H5="$CRATE/tests/webkit/TinyArcadeH5Oracle.html"
H5_BROWSER=${H5_BROWSER:-/Applications/Google Chrome.app/Contents/MacOS/Google Chrome}

if [ ! -x "$H5_BROWSER" ]; then
  echo "H5 browser is not executable: $H5_BROWSER" >&2
  exit 1
fi

base64url() {
  base64 <"$1" | tr -d '\n' | tr '+/' '-_' | tr -d '='
}

run_h5() {
  name=$1
  wasm=$2
  replay=$3
  dom="$TEMP/$name.html"
  log="$TEMP/$name-browser.log"
  url="file://$H5#wasm=$(base64url "$wasm")&replay=$(base64url "$replay")"
  "$H5_BROWSER" \
    --headless=new \
    --disable-gpu \
    --no-first-run \
    --disable-default-apps \
    --allow-file-access-from-files \
    --user-data-dir="$TEMP/$name-browser-profile" \
    --virtual-time-budget=10000 \
    --dump-dom \
    "$url" >"$dom" 2>"$log" &
  browser_pid=$!

  # Chrome 151 on macOS writes --dump-dom successfully but can retain its
  # parent process when another GUI browser session is open. Stop only this
  # isolated headless process after it has emitted a terminal page state.
  attempts=0
  while [ "$attempts" -lt 200 ]; do
    if grep -Eq 'data-status="(pass|fail)"' "$dom"; then
      break
    fi
    if ! kill -0 "$browser_pid" 2>/dev/null; then
      break
    fi
    sleep 0.1
    attempts=$((attempts + 1))
  done
  if kill -0 "$browser_pid" 2>/dev/null; then
    kill "$browser_pid"
  fi
  wait "$browser_pid" 2>/dev/null || true
  if ! grep -Fq 'data-status="pass"' "$dom"; then
    cat "$dom" >&2
    cat "$log" >&2
    return 1
  fi
  sed -n 's/.*\(OK: H5 WebAssembly == tinyvm[^<]*\).*/\1/p' "$dom"
}

xcrun swiftc \
  -parse-as-library \
  -warnings-as-errors \
  -O \
  -framework JavaScriptCore \
  -framework CryptoKit \
  "$CRATE/tests/webkit/TinyArcadeWebKitOracle.swift" \
  -o "$ORACLE"

run_game() {
  name=$1
  build_script=$2
  input_plan=$3
  wasm="$TEMP/$name.wasm"
  replay="$TEMP/$name.tareplay"
  "$build_script" "$wasm" >/dev/null
  "$CARGO" run -q -p agenterm-tinyvm --features replay -- \
    replay record "$wasm" "$input_plan" "$replay" >/dev/null
  "$CARGO" run -q -p agenterm-tinyvm --features replay -- \
    replay check "$wasm" "$replay" >/dev/null
  "$ORACLE" "$ADAPTER" "$wasm" "$replay"
  run_h5 "$name" "$wasm" "$replay"
}

run_game \
  depth-well-0.1.0 \
  "$CRATE/build-depth-well-cartridge.sh" \
  "$CRATE/tests/fixtures/depth-well-replay-v1.inputs"
run_game \
  paddle-guard-0.1.0 \
  "$CRATE/build-paddle-guard-cartridge.sh" \
  "$CRATE/tests/fixtures/paddle-guard-replay-v1.inputs"
run_game \
  signal-lock-0.1.0 \
  "$CRATE/build-signal-lock-cartridge.sh" \
  "$CRATE/tests/fixtures/signal-lock-replay-v1.inputs"
run_game \
  fan-c-cartridge-0.1.0 \
  "$CRATE/build-fan-c-cartridge.sh" \
  "$CRATE/tests/fixtures/fan-c-replay-v1.inputs"

echo "OK: development-only JSC + H5 differential; no web runtime enters the iOS app"
