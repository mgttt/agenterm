# Q3 — Lowering cost: how big is the minimal usable IR→native lowerer, and where does it go?

Spec + pinned criteria + verdict: [`plan/design-lowering-cost-experiment.md`](../../../plan/design-lowering-cost-experiment.md).
Clean-room; no prior implementation was consulted. Parallel to and independent of Q1 (`../ir/`).

## What this is

Q0 (layer count) measured a kernel that **cannot run a neutral IR** — its payloads were
**precompiled native blobs**; the kernel only map+jumps. Q3 adds the missing piece: a
runtime **IR→native x86_64 lowerer** (`lower.rs`), and measures **X = its byte size** and
whether it belongs **in-kernel** (smallest total delivery) or **out-of-kernel** (smallest TCB).

- `ir.rs` — the IR: a 24-opcode register-machine bytecode (data, not the lowerer; not in X).
- `lower.rs` — **the lowerer = X**. Single emit pass + one rel32 back-patch. Split into
  `[X86_64]` encoders (139 lines, per-ISA) and `[SHARED]` driver (162 lines, ISA-independent).
- `runner.rs` — builds the env table (kernel primitives + Q0 adapters via sysv64 shims). Not in X.
- `tools/ir_gen.rs` — author-time tool emitting the three payloads' IR `.bin` (not shipped, not in X).
- `pack/in_kernel.rs` — variant A: lowerer statically linked (TCB = whole binary).
- `pack/out_kernel_blob.rs` — variant B: lowerer as a flat PIC blob loaded by Q0's minimal kernel.
- `pack/measure_{lower,driver}_flat.rs` — two flat blobs differing only by the lowerer → X.

The lowered payloads reproduce Q0's three semantics exactly: `pure_compute` (exit 163),
`read_hash_print` (FNV-1a/64 = `a49d2cbecc13994f`), `spawn_echo` (`exit=07`, exit 7).

## Results (strip release; x86_64; rustc 1.97; Linux cross-measured, Windows executed)

| # | criterion | Linux (unpadded) | Windows (PE-aligned) |
|---|---|--:|--:|
| ① | **X = minimal usable lowerer** | **3003 B** (flat-safe; in-kernel jump-table version ~2777) | 3003 (ELF flat) |
| ② | shared / ISA-specific split | 162 shared / **139 x86-specific** lines (46% per-ISA) | same |
| ③ | total delivery (run rhp) | in-kernel **6360** vs out-of-kernel **7816** | 7680 vs 9216 |
| ④ | TCB | in-kernel ~6.2–6.4 KB vs out-of-kernel **~2.93 KB** (frozen) | 7680 vs ~3.96 KB |

Minimal kernel baseline (no lowerer) ≈ 2.93 KB (L) / ~4 KB (W). **X ≈ the whole kernel.**

**Verdict:** a real no-free-lunch tradeoff. ③ favors **in-kernel** (~1.5 KB smaller total,
constant); ④ favors **out-of-kernel** (half the frozen TCB). X≈kernel-size makes it real
(in-kernel doubles TCB). Implementation surfaced a decisive extra constraint: an
out-of-kernel lowerer, run as a flat non-relocated blob, must be **memset-free and
jump-table-free** (a code generator, unlike Q0's precompiled payloads) — measured cost
+8% on X and a more fragile build. See spec §8 for the full decision trace and the
untested copy-and-patch alternative that could dissolve the tradeoff.

## Reproduce

```powershell
# Windows (executes all six variants; needs MSVC target + llvm-tools for flat blobs):
pwsh research/dynamic-core/lowering/build/build_lowering.ps1
cd research/dynamic-core/lowering/out
[IO.File]::WriteAllText("$PWD\input.txt","dynamic-core experiment 2026-08-08`n")
.\A_lower_pure_windows.exe;  $LASTEXITCODE   # 163   (in-kernel)
.\A_lower_rhp_windows.exe                     # a49d2cbecc13994f
.\A_lower_spawn_windows.exe; $LASTEXITCODE    # prints exit=07, exits 7
.\B_lower_pure_windows.exe;  $LASTEXITCODE    # 163   (out-of-kernel, 2-stage)
.\B_lower_rhp_windows.exe                      # a49d2cbecc13994f
.\B_lower_spawn_windows.exe; $LASTEXITCODE     # prints exit=07, exits 7
```

```sh
# Linux artifacts (cross-compiled from any host; byte-measured, not run here — no WSL):
bash research/dynamic-core/lowering/build/build_lowering_linux.sh   # prints X + sizes
```

Independent reference hash (FNV-1a/64 of the 35-byte input) = `a49d2cbecc13994f`
(Python: offset basis `0xcbf29ce484222325`, prime `0x100000001b3`) — same as Q0, proving
the lowered payload is not co-wrong with the kernel.
