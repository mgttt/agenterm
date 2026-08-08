#!/usr/bin/env bash
# Build all Linux/x86_64 artifacts for the dynamic-core experiment.
# Produces stripped release binaries + flat payload blobs in ../out/.
# Cross-compiles from any host (uses rustc + bundled rust-lld; no C toolchain,
# no libc). Reproducible: all flags are explicit below.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
OUT="$ROOT/out"
mkdir -p "$OUT"

TARGET=x86_64-unknown-linux-gnu
# Bundled LLVM linker shipped with the Rust toolchain (multi-target).
SYSROOT="$(rustc --print sysroot)"
HOST="$(rustc -vV | sed -n 's/host: //p')"
LLD="$SYSROOT/lib/rustlib/$HOST/bin/rust-lld.exe"
[ -x "$LLD" ] || LLD="$SYSROOT/lib/rustlib/$HOST/bin/rust-lld"

RFLAGS="--edition 2021 -O -C panic=abort -C debuginfo=0 -C force-unwind-tables=no -A unexpected_cfgs --cfg dc_os=\"linux\" --target $TARGET"

echo "== rustc: $(rustc --version)"
echo "== target: $TARGET"
echo "== lld: $LLD"

# --- helper: build a static ELF executable (variant A / kernel) ---
build_exe() { # <src> <cfg> <out>
  local src="$1" cfg="$2" out="$3"
  rustc $RFLAGS --cfg "$cfg" --emit=obj "$src" -o "$OUT/tmp.o"
  "$LLD" -flavor gnu -e _start -static --strip-all -o "$OUT/$out" "$OUT/tmp.o"
  rm -f "$OUT/tmp.o"
}

# --- helper: build a flat position-independent payload blob ---
build_blob() { # <src> <out.bin>
  local src="$1" out="$2"
  rustc $RFLAGS -C relocation-model=pic --emit=obj "$src" -o "$OUT/tmp.o"
  "$LLD" -flavor gnu --oformat binary -T "$HERE/flat.ld" -o "$OUT/$out" "$OUT/tmp.o"
  rm -f "$OUT/tmp.o"
}

echo "== variant A (1 layer) =="
build_exe "$ROOT/pack/variant_a_onelayer/pure_compute.rs"    'dc_variant="a"' A_pure_linux
build_exe "$ROOT/pack/variant_a_onelayer/read_hash_print.rs" 'dc_variant="a"' A_rhp_linux

echo "== variant B (2 layer) — payload blobs =="
build_blob "$ROOT/pack/variant_b_twolayer/payload_pure.rs" blob_pure_linux.bin
build_blob "$ROOT/pack/variant_b_twolayer/payload_rhp.rs"  blob_rhp_linux.bin

echo "== variant B (2 layer) — kernel/loader (blob embedded) =="
export DC_BLOB="$OUT/blob_pure_linux.bin"; build_exe "$ROOT/pack/variant_b_twolayer/kernel.rs" 'dc_variant="b"' B_kernel_pure_linux
export DC_BLOB="$OUT/blob_rhp_linux.bin";  build_exe "$ROOT/pack/variant_b_twolayer/kernel.rs" 'dc_variant="b"' B_kernel_rhp_linux
unset DC_BLOB

echo "== done. sizes: =="
( cd "$OUT" && ls -la A_pure_linux A_rhp_linux blob_pure_linux.bin blob_rhp_linux.bin B_kernel_pure_linux B_kernel_rhp_linux )
