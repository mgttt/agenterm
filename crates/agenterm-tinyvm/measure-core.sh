#!/usr/bin/env sh
# Build the no_std static core, link a tiny C driver, strip, and report size.
# Passes if the stripped executable is < 100 KiB and the selftest returns 42.
set -e
cd "$(dirname "$0")/../.."
TD="${CARGO_TARGET_DIR:-target}"
cargo rustc -p agenterm-tinyvm --lib --release --features staticcore \
  --crate-type staticlib -- -Copt-level=z -Cpanic=abort -Ccodegen-units=1
printf 'extern int tinyvm_selftest(void);\nint main(void){return tinyvm_selftest();}\n' > "$TD/tvmain.c"
case "$(uname -s)" in
  Darwin)
    cc -Os -Wl,-dead_strip "$TD/tvmain.c" "$TD/release/libagenterm_tinyvm.a" -o "$TD/tinycore" -lm
    # Keep only undefined and dynamically referenced symbols. `-x` still
    # retains external Rust symbols in a fully linked executable on Darwin.
    strip -u -r "$TD/tinycore"
    ;;
  *)
    cc -Os -Wl,--gc-sections "$TD/tvmain.c" "$TD/release/libagenterm_tinyvm.a" -o "$TD/tinycore" -lm
    strip -s "$TD/tinycore"
    ;;
esac
SIZE=$(stat -c%s "$TD/tinycore" 2>/dev/null || stat -f%z "$TD/tinycore")
RC=0; "$TD/tinycore" || RC=$?
echo "static core: ${SIZE} bytes; selftest rc=${RC}"
[ "$SIZE" -lt 102400 ] || { echo "FAIL: core >= 100 KiB"; exit 1; }
[ "$RC" -eq 42 ] || { echo "FAIL: selftest != 42"; exit 1; }
echo "OK: < 100 KiB and selftest==42"
