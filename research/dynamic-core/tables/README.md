# tables/ — Q7: OS-interface content as DATA

Decisive experiment for
[`plan/archive/design-os-interface-as-data-experiment.md`](../../../plan/archive/design-os-interface-as-data-experiment.md).

**Question:** Q1 found the architecture has exactly one seam — the OS-interface content
(leaks L1–L5) — that is per-target, hand-written, and unverifiable. **Can that content
stop being code and become DATA (tables) interpreted by one fixed marshaller?** If so, the
same seam improves on three fronts at once: growth (mechanism fixed, capability by data),
verifiability (data validates against a schema; code does not), reuse (data shares finer
than code).

**One-line conclusion:** **bounded reachable.** The single-native-call family
(Alloc/Open/Read/Close/Write) becomes data over a fixed marshaller and **executes** — for
it the marginal *code* cost of +1 intent and +1 same-ISA target is **0**. The seam stays
**code** at (a) orchestration / control flow (L3b — multi-call dataflow, SysV fork/branch)
and (b) cross-ISA intent restructuring (I2 — `openat`/`clone`). L3's layout half (L3a)
tablifies only **in query form + a host oracle = the missing 5th primitive (Declare)**. The
numbers, the boundary list, and the decision trace are in [`RESULTS.md`](./RESULTS.md).

## Layout

```
table.rs    the DATA: AbiDesc (per-target ABI+reach+intent-table) + per-(target,intent)
            OpSpec rows + StructSpec (L3a layout as data). No executable logic.
marshal.rs  the fixed generic MARSHALLER (one mechanism, all intents/targets; zero
            per-intent / per-target branch) + schema validator (④) + boundary map (⑤).
            Non-Call lowering copied verbatim from Q1 common.rs.
main.rs     driver: #[path]-reuses Q1 ir/asm/payloads, lowers via the table, JIT-runs
            on Win64 against real kernel32.
out/        build output (git-ignored elsewhere; *.bin dumps + driver.exe)
```

## Build & run

```powershell
cd research/dynamic-core/tables
mkdir out 2>$null
rustc --edition 2021 -O -A dead_code main.rs -o out/driver.exe
cd out; ./driver.exe
```

Compiled as a single `rustc` invocation via `#[path]` modules — **does not touch the root
workspace**. Reuses Q1's `spec/ir.rs`, `lower/asm.rs`, `payloads/payloads.rs` verbatim.

## What Q7 does NOT do

No second ISA, no optimization, **no IDL / call-sequencing bytecode** (inventing one is the
failure mode the experiment detects), no full Declare primitive (only a stub oracle to test
the CO-RE reframe's shape). See `RESULTS.md` §"Deviations" and the spec §7.
