#!/usr/bin/env bash
# Cross-compile + byte-measure Q3 lowering artifacts for Linux/x86_64 (no WSL here:
# built and measured, NOT executed — same honest split as Q0). Mirrors Q0 build_linux.sh.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
L="$(cd "$HERE/.." && pwd)"          # .../lowering
ROOT="$(cd "$L/.." && pwd)"          # .../dynamic-core
OUT="$L/out"; mkdir -p "$OUT"
TARGET=x86_64-unknown-linux-gnu
SYSROOT="$(rustc --print sysroot)"; HOST="$(rustc -vV | sed -n 's/host: //p')"
LLD="$SYSROOT/lib/rustlib/$HOST/bin/rust-lld.exe"; [ -x "$LLD" ] || LLD="$SYSROOT/lib/rustlib/$HOST/bin/rust-lld"
RF=(--edition 2021 -O -C panic=abort -C debuginfo=0 -C force-unwind-tables=no -A unexpected_cfgs -A dead_code --cfg 'dc_os="linux"' --target "$TARGET")

build_exe(){ local src="$1" cfg="$2" out="$3"; rustc "${RF[@]}" --cfg "$cfg" --emit=obj "$src" -o "$OUT/tmp.o"; "$LLD" -flavor gnu -e _start -static --strip-all -o "$OUT/$out" "$OUT/tmp.o"; rm -f "$OUT/tmp.o"; }
# -min-jump-table-entries suppresses jump tables (flat blob is copied+jumped with no
# relocations; a jump table breaks under that). See RESULTS §8 — a real out-of-kernel cost.
build_blob(){ local src="$1" out="$2"; rustc "${RF[@]}" -C relocation-model=pic -C llvm-args=-min-jump-table-entries=200 --emit=obj "$src" -o "$OUT/tmp.o"; "$LLD" -flavor gnu --oformat binary -T "$ROOT/build/flat.ld" -o "$OUT/$out" "$OUT/tmp.o"; rm -f "$OUT/tmp.o"; }

# IR (author-time)
rustc -O "$L/tools/ir_gen.rs" -o "$OUT/ir_gen.exe" >/dev/null; "$OUT/ir_gen.exe" "$OUT" >/dev/null

for p in pure rhp spawn; do
  export DC_IR="$OUT/ir_$p.bin"
  build_exe "$L/pack/in_kernel.rs" 'dc_variant="a"' "A_lower_${p}_linux"
  build_blob "$L/pack/out_kernel_blob.rs" "lowblob_${p}_linux.bin"
  export DC_BLOB="$OUT/lowblob_${p}_linux.bin"
  build_exe "$ROOT/pack/variant_b_twolayer/kernel.rs" 'dc_variant="b"' "B_lower_${p}_linux"
  unset DC_BLOB DC_IR
done

# baseline: minimal kernel + precompiled NATIVE pure blob (no lowerer)
build_blob "$ROOT/pack/variant_b_twolayer/payload_pure.rs" "native_pure_linux.bin"
export DC_BLOB="$OUT/native_pure_linux.bin"; build_exe "$ROOT/pack/variant_b_twolayer/kernel.rs" 'dc_variant="b"' "baseline_kernel_linux"; unset DC_BLOB

# X isolation: two flat blobs differing only by the lowerer
build_blob "$L/pack/measure_lower_flat.rs"  "mx_lower_flat.bin"
build_blob "$L/pack/measure_driver_flat.rs" "mx_driver_flat.bin"
XL=$(stat -c%s "$OUT/mx_lower_flat.bin"); XD=$(stat -c%s "$OUT/mx_driver_flat.bin")
echo "X (linux flat) = $((XL-XD))  (lower=$XL driver=$XD)"
( cd "$OUT" && ls -la A_lower_*_linux B_lower_*_linux baseline_kernel_linux lowblob_*_linux.bin native_pure_linux.bin ir_*.bin )
