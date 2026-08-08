#!/usr/bin/env bash
# Build all Linux/x86_64 artifacts for the adapter-reuse (Q5) experiment.
# Cross-compiled from any host (rustc + bundled rust-lld; no C toolchain, no libc).
# BYTE-MEASURED ONLY here (the host has no WSL, so Linux binaries are not executed);
# the content-addressing MECHANISM is proven on Windows. The store's dedup is
# structural (identical content -> identical name), so it holds identically on Linux.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/.." && pwd)"
OUT="$ROOT/out"
mkdir -p "$OUT"

TARGET=x86_64-unknown-linux-gnu
SYSROOT="$(rustc --print sysroot)"
HOST="$(rustc -vV | sed -n 's/host: //p')"
LLD="$SYSROOT/lib/rustlib/$HOST/bin/rust-lld.exe"
[ -x "$LLD" ] || LLD="$SYSROOT/lib/rustlib/$HOST/bin/rust-lld"

RFLAGS="--edition 2021 -O -C panic=abort -C debuginfo=0 -C force-unwind-tables=no -A unexpected_cfgs --cfg dc_os=\"linux\" --target $TARGET"

echo "== rustc: $(rustc --version)"

build_exe() { # <src> <extra-cfg> <out>
  local src="$1" extra="$2" out="$3"
  # shellcheck disable=SC2086
  rustc $RFLAGS $extra --emit=obj "$src" -o "$OUT/tmp.o"
  "$LLD" -flavor gnu -e _start -static --strip-all -o "$OUT/$out" "$OUT/tmp.o"
  rm -f "$OUT/tmp.o"
}
build_blob() { # <src> <out.bin> [optlevel]
  local src="$1" out="$2" opt="${3:-2}"
  # shellcheck disable=SC2086
  rustc $RFLAGS -C opt-level=$opt -C relocation-model=pic --emit=obj "$src" -o "$OUT/tmp.o"
  "$LLD" -flavor gnu --oformat binary -T "$HERE/flat.ld" -o "$OUT/$out" "$OUT/tmp.o"
  rm -f "$OUT/tmp.o"
}

echo "== baked baseline blobs =="
build_blob "$ROOT/pack/baked/rhp.rs"     baked_rhp_linux.bin
build_blob "$ROOT/pack/baked/readlen.rs" baked_readlen_linux.bin

echo "== content-addressed blobs =="
build_blob "$ROOT/pack/ca/adapter_v1.rs"      ca_adapter_v1_linux.bin
build_blob "$ROOT/pack/ca/adapter_v2.rs"      ca_adapter_v2_linux.bin
build_blob "$ROOT/pack/ca/payload_rhp.rs"     ca_payload_rhp_linux.bin
build_blob "$ROOT/pack/ca/payload_readlen.rs" ca_payload_readlen_linux.bin
build_blob "$ROOT/pack/ca/adapter_v1.rs"      ca_adapter_v1_opt1_linux.bin 1
build_blob "$ROOT/pack/ca/adapter_v1alt.rs"   ca_adapter_v1alt_linux.bin

echo "== CA loaders (mechanism under test) =="
build_exe "$ROOT/loader.rs" ""              loader_ca_linux
build_exe "$ROOT/loader.rs" '--cfg dc_verify' loader_ca_verify_linux

echo "== embed baseline loader (Q0 variant B; ③ reference) =="
export DC_BLOB="$OUT/baked_rhp_linux.bin"; build_exe "$ROOT/loader_embed.rs" "" loader_embed_rhp_linux
unset DC_BLOB

echo "== done. sizes (bytes): =="
( cd "$OUT" && ls -la *_linux *_linux.bin 2>/dev/null | awk '{print $5"\t"$9}' | sort -k2 )
