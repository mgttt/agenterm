# tinyvm optional SIMD game-kernel profile

Owner: [PRD 02.35](../prd/PRD_02_35_agenterm_tinyvm.md)

Status: implemented, optional workload profile

The Cargo feature `simd` enables a bounded standard WebAssembly SIMD subset
inside the unified `agenterm-tinyvm` crate. It is deliberately driven by three
game-runtime jobs: saturated signed PCM mixing, whole-vector masks for packed
flags or pixel composition, and wrapping integer lanes for coordinates,
counters and deterministic fixed-point state. It is not a claim that the
complete SIMD proposal is implemented.

## Accepted standard surface

```text
v128 value type
├── function parameters/results
├── locals and zero initialization
├── immutable/mutable globals
├── block signatures and typed select
└── typed host boundaries

0xfd instructions
├── v128.load
├── v128.store
├── v128.const
├── v128.not
├── v128.and
├── v128.andnot
├── v128.or
├── v128.xor
├── v128.bitselect
├── v128.any_true
├── i8x16.add / i8x16.sub
├── i16x8.add / i16x8.sub / i16x8.mul
├── i16x8.add_sat_s
├── i16x8.sub_sat_s
├── i32x4.add / i32x4.sub / i32x4.mul
└── i64x2.add / i64x2.sub / i64x2.mul
```

All vector bytes use standard little-endian lane order and portable scalar Rust
semantics. The interpreter does not emit host vector instructions and does not
depend on the host ISA. Whole-vector logic operates on the canonical 16 bytes;
`bitselect(a, b, mask)` chooses each bit from `a` where the mask bit is one and
from `b` otherwise. `v128.load` and `v128.store` validate natural alignment
immediates and preflight the complete 16-byte memory range. Signed lanes use
Rust's defined `i16::saturating_add` and `i16::saturating_sub`, including both
overflow boundaries. Integer lane arithmetic uses the corresponding wrapping
operation at each lane width, so overflow is deterministic in debug and
release builds and independent of host ISA.

Any other `0xfd` instruction is rejected during module decoding with a typed
unsupported-opcode error. When the Cargo feature is absent, the first SIMD
instruction fails explicitly as `SIMD feature is disabled`; default and
`staticcore` builds therefore retain their existing semantics and size.

## Evidence

`smoke-wabt-simd-audio.sh` compiles the 515-byte workload independently with
WABT, validates it with `wasm-validate`, then runs the same lane vectors through
tinyvm, macOS JavaScriptCore and an actual headless H5 browser. The three
runtimes produce the same saturated lanes, six nontrivial mask vectors and
`any_true` results for both nonzero and zero inputs, plus eleven wrapping
add/subtract/multiply vectors across 8-, 16-, 32- and 64-bit lanes. The audio
lanes are:

```text
add: 32767,-32768,300,-300,32767,-32768,-5000,5000
sub: 20000,-20000,-100,100,32766,-32767,32767,-32768
```

The Rust black box also covers v128 function/local/global/constant values,
rejects scalar lane operands, scalar or missing mask operands and an
over-aligned load, and proves an out-of-bounds store leaves the destination tail
unchanged. The optional profile stores v128 inline and keeps `Val` at 24 bytes;
there is no heap allocation or native handle per vector.

The default stripped static core remains 101,256 bytes under its unchanged
100 KiB gate. The optional profile is 117,768 bytes under its separate 120 KiB
gate. With the current complete iOS host owners, the SIMD build links at
1,799,160 bytes arm64 and 1,903,568 bytes x86_64, under separate explicit
opt-in ceilings; those budgets do not weaken the default product boundaries.

A separate manifest-bearing TinyArcade cartridge performs the same add and
subtract operations during `game_init`, checks both saturation extremes,
renders one indexed frame, and round-trips its 16-byte state. With an
`ios-c-api,simd` XCFramework, the Swift/C ABI opens and executes that cartridge
on the booted iPhone 17 Pro Simulator. The focused linked consumer is 1,622,376
bytes with the current game-kernel profile.
