# ⚠️ 已归档：dynamic-core 已判决实验规格

> **归档于 2026-08-10。** Q0–Q23 研究轨已经封闭，综合结论与重新开启条件由
> `research/dynamic-core/SYNTHESIS.md` 和 `research/dynamic-core/README.md` 拥有。
> 本文件只保存该问题的实验前判据与历史结果，不是活跃版本任务。

# Q10 — Copy-and-patch stencil lowering: does it shrink X and dissolve Q2's tradeoff?（历史规格）

> ⚠️ **Not AgenTerm product scope.** Independent research track; implementation in
> `research/dynamic-core/stencil/`. Succeeds [`design-lowering-cost-experiment.md`](design-lowering-cost-experiment.md)
> (Q2), which measured X=3003 B and named copy-and-patch as its untested alternative.
> Does not enter any version plan, does not change `PRD.md` capability state.
> **Parallel to and independent of Q9/Q6 — this track does not touch those directories.**

| field | value |
|---|---|
| **Date** | 2026-08-08 |
| **Goal** | Measure a minimal copy-and-patch backend: how big is the runtime part (memcpy + relocation applier)? Where does the cost go? Does Q2's in/out-kernel tradeoff dissolve? |
| **Prior reading** | Q2 (X=3003 B, the tradeoff), Q1 IR (the input), Q5 (per-ISA cost), `reference-cross-target-execution.md` §7.4/§8.1/§10.1 (copy-and-patch as a KB-scale Futamura 1st projection) |
| **Provenance** | **Clean-room.** Built from the public *description* of copy-and-patch only; **no prior implementation (incl. CPython's `Tools/jit/`) was read.** |

---

## 0. Why this is the pivot after Q2

Q2 measured the minimal usable hand-written IR→x86_64 encoder at **X ≈ 3003 B ≈ the whole
kernel**, which is exactly what makes "lowerer in-kernel vs out-of-kernel" a **real
no-free-lunch tradeoff** (in-kernel = smaller total delivery; out-of-kernel = smaller
frozen TCB). Q2 then flagged copy-and-patch: emit machine-code templates (stencils) with a
**real optimizing compiler at build time**, so the runtime "lowerer" is only **memcpy + a
relocation applier**. If the reviewer's hypothesis holds, **X's code part collapses to a
few hundred bytes**, the ISA cost moves to **stencil data** (a payload, shippable
out-of-kernel), and the tradeoff **dissolves** into "small applier in-kernel + stencils as
data." It also sits squarely on this track's core principle — **mechanism fixed, capability
via data** — so Q10 tests that principle on the **code-generation** axis (as Q7 tested it
on the OS-interface axis: +1 intent = 0 marshaller code).

### Already settled, out of scope here

1. **IR neutrality** (Q1) — assumed; the IR is Q2's, byte-identical, unchanged.
2. **The lowerer only generates code for its own machine** (Q2 §0.2) — carried over.
3. **Four primitives unchanged.** The applier is a pure user of ① (`mem_alloc`/
   `mem_protect`) + ② (jump). Any urge to add a 5th primitive is a **finding**, not a fix.

---

## 1. Hard constraints (violation ⇒ invalid)

1. **Real compiler produces the stencils at BUILD time** (`rustc -O2`/LLVM). The runtime
   must contain **no instruction encoder and no register allocator** — only memcpy + hole
   patching + whatever irreducible glue the mechanism forces (that glue is a measurand,
   not a licence).
2. **Runs the three Q1/Q2 payloads with byte-identical output** (`pure`→163,
   `rhp`→`a49d2cbecc13994f`, `spawn`→`exit=07`/7). Not a mock-up.
3. **Same byte口径 as Q2** (flat-PIC blob subtraction; `-min-jump-table-entries=200`,
   `relocation-model=pic`, ELF, `flat.ld`) so the headline X is directly comparable to
   3003 B.
4. **Disease detector:** any urge to grow this into a *stencil compiler / toolchain* is the
   pathology this experiment must **detect**, not satisfy. Minimal viable form only.
5. **No metric tuning after the fact.** Criteria below are pinned before writing the
   applier.

---

## 2. Minimal experiment

| axis | choice | why |
|---|---|---|
| ISA | x86_64 only | time-box; the ISA axis is *projected* in ⑤, not built twice |
| OS | Windows executed; Linux byte-measured | no WSL, same as Q1/Q2 |
| register model | **memory register file** (vreg id = slot) | the minimal 1-stencil-per-op form; no register-carrying convention (that needs `preserve_none`/`ghccc` = a heavier toolchain and variant explosion) |
| stencils | only the opcodes the 3 payloads use | time-box; no float, no full coverage |
| holes | `R_X86_64_PC32` (via `-C relocation-model=static`) | direct rip-relative, patchable to regfile/env/pool/label; immediates via a constant pool |

---

## 3. Criteria (pinned before the applier was written; not changed after)

| # | criterion | metric | nature |
|---|---|---|---|
| **①** | **X's new value** — the runtime memcpy+relocation-applier size | bytes + LOC, flat subtraction (Q2口径) **and** `llvm-size` code/data split; **direct compare to Q2's 3003 B** | **main (intercept)** |
| **②** | **Where the cost went** — stencil-data size and its growth law | bytes; scales with opcodes? ISAs? the product? **total (code+data) vs Q2's X** | slope |
| **③** | **Is the tradeoff dissolved** — in/out-kernel total delivery + TCB, recomputed | Q2口径; does the "X negligible ⇒ obviously in-kernel" collapse trigger? | **main (decision)** |
| **④** | **Cost & boundary** — build-time compiler dependency; which IR constructs won't stencilize | prose + bytes; **measure, don't assume** (esp. control flow) | list / boundary |
| **⑤** | **Relation to Q5** — is +1 ISA cheaper as stencils, or just a different form? | prose vs Q5's +307 LOC | slope |

**Main criteria are ①+③.** Honesty clause: if the stencil total is *bigger*, report it —
refuting the reviewer's expected value is worth more than confirming it.

---

## 4. Decision tree, kill criterion, time-box (pinned)

1. **① first.** If the whole runtime footprint (applier code + stencil data) **≥ Q2's
   3003 B**, the headline promise ("X collapses") is **false** — record it and do not let a
   small applier-*core* number paper over a larger total.
2. **③ decides the tradeoff.** Dissolved **only if** X becomes negligible vs the ~2.93 KB
   kernel so the frozen-TCB penalty of in-kernel vanishes. If X grew, the collapse does not
   trigger.
3. **kill criterion:** if control flow / calls **cannot** be made to run at all under the
   stencil model, report "copy-and-patch does not cover the payloads" and stop.
4. **time-box:** produce ①②③ then stop. **No second ISA** (unless ⑤ needs it and it's
   cheap), **no optimization, no full opcode coverage.**

---

## 5. Directory

```
research/dynamic-core/stencil/       ← NOT wired into the root workspace
├─ stencils.rs        ← stencil TEMPLATES (the real compiler's input)
├─ stencilize.rs      ← BUILD tool: ELF obj -> bytes+holes -> stencils_gen.rs (not shipped, not X)
├─ patch.rs           ← the APPLIER = X_new (memcpy + PC32 applier + control-flow residual)
├─ ir.rs              ← Q2 IR opcode/env constants (copied verbatim; data, not X)
├─ runner.rs          ← env table (Q2 adapters via sysv64 shims); not X
├─ pack/{in_kernel,measure_patch_flat,measure_driver_flat}.rs
├─ build/build_stencil.ps1
└─ RESULTS.md
```

---

## 6. Excluded options (do not re-propose)

| option | why excluded |
|---|---|
| register-carrying stencils (`preserve_none`/`ghccc`) | variant explosion + exotic toolchain; the memory-regfile form is the minimal one |
| a second ISA | time-box; ⑤ projects it |
| reading CPython's `Tools/jit/` | clean-room |
| full opcode coverage / float | beyond the 3 payloads |
| tightening the applier for a smaller number | metric tuning |

## 7. What this does NOT answer

- Runtime *speed* of stencil code vs Q2/Q9 (only size here).
- A real two-point per-ISA slope (⑤ is a projection).
- Whether a register-carrying variant would be faster/bigger.

---

## 8. Conclusion backfill (2026-08-08, measured)

Implementation + all numbers + third-party reproduce:
[`research/dynamic-core/stencil/RESULTS.md`](../../research/dynamic-core/stencil/RESULTS.md).

**Verdict: copy-and-patch is NOT worth it at this scale; the tradeoff is NOT dissolved.**

- **① (main):** the pure memcpy+relocation applier is **651 B** (tiny, as hypothesized) —
  but that was never the costly part. Whole runtime applier **code = 4515 B**; whole
  footprint (code+data, flat, Q2口径) = **5826 B ≈ 1.94× Q2's 3003 B**. X **grew**.
- **② :** stencil data = 406 code bytes + 55 holes (tight **571 B**; Rust-materialized
  ~1210 B), scaling as **opcodes × ISAs** (same multiplicative law as any per-ISA lowerer)
  plus a per-`argc` `CALL` variant factor. Total is **bigger** than Q2's X — worse than
  merely moving bytes to data, because the **opcode decode/dispatch never moved to data**
  and dominates.
- **③ :** recomputed, the tradeoff **shifts both endpoints up** and keeps its shape; the
  "X negligible ⇒ obviously in-kernel" collapse **does not trigger** (X grew). Net loss vs
  Q2's hand lowerer at both placements.
- **④ :** control flow **confirmed un-stencilable** (a stencil can't leave CPU flags live
  across its boundary; the branch target is a layout-time offset) → ~20 bytes of
  applier-emitted residual per branch. Nuance: with a *memory* regfile the *data-flow* side
  of branching is trivial, so control flow is hard **only** at the flags/target seam.
  `CALL` arity forces per-`argc` variants. The **±2 GB PC32 placement constraint is real
  and was hit** (co-locate code/regfile/pool/env in one arena). Build-time compiler
  dependency is real; cross-compiler reproducibility is by **contract (hole symbols)**, not
  by bytes.
- **⑤ :** +1 ISA becomes **per-ISA data (stencils, distribution-time strippable)** + an
  **irreducible per-ISA code residual** (reloc kinds + control-flow emit). A **form change
  toward data** (matches the track principle) but **not a size win** and not free.

**Refuted the reviewer's expected value** (the highest-value outcome): the Futamura
first-projection win is real but small and localized to the encoder sub-component, which
was already cheap; the dominant decode/dispatch survives and the stencil data pushes the
total *above* both Q2's compiler (3003 B) and Q9's interpreter (3177 B). **Not tuned for a
prettier result.**

### Deviations
See RESULTS.md "Deviations" — X reported at three levels (applier-core / whole-code /
code+data) because the single "X" the reviewer imagined was only the smallest of the
three; `emit` mildly inflated by an operand-context struct (verdict shown robust to it);
no second ISA (⑤ projected); Windows executed, Linux byte-measured.

---

*Research-track projection. No version ownership, no PRD capability change.*
