# ⚠️ 已归档：dynamic-core 已判决实验规格

> **归档于 2026-08-10。** Q0–Q23 研究轨已经封闭，综合结论与重新开启条件由
> `research/dynamic-core/SYNTHESIS.md` 和 `research/dynamic-core/README.md` 拥有。
> 本文件只保存该问题的实验前判据与历史结果，不是活跃版本任务。

# Q5 — The ISA axis: what does a second ISA cost?（历史规格）

> ⚠️ **Not AgenTerm product scope.** Independent research track, implementation in
> `research/dynamic-core/isa/`. Continues the dynamic-core track (Q0/Q1). Does not
> enter any version plan, does not change `PRD.md` capability state.

| field | value |
|------|-----|
| **date** | 2026-08-08 |
| **purpose** | measure, not argue, what adding a SECOND ISA (aarch64) costs: is the neutral IR still neutral on the ISA axis, and does "N kernels, one per ISA" stay bounded? |
| **prereqs** | Q0 `RESULTS.md` (§0 assertion 2), Q1 `ir/RESULTS.md` (ABI-axis leak L1–L5, the two-way LOC split) |
| **provenance** | **clean-room.** The aarch64 encoder is written from the published ARM ARM A64 encodings; no prior backend consulted. Q1's IR + payloads are reused verbatim (mandated — same-track completed artifact). |

---

## 0. The untested assertion this attacks

Q0's spec §0.2 states, and never tested:

> The kernel only generates code for its own machine; cross-compilation is an
> ordinary payload capability, **so kernel size does not grow with ISA count — you
> just build N copies.**

The whole "N×M collapses to N" cost model rests on **each of those N builds being
cheap**, yet Q0/Q1 pinned x86_64 throughout — not one ISA was ever added. Q1 varied
only the **ABI axis** (SysV vs Win64, same ISA, deliberately isolated). The ISA axis
is a different axis: register count/naming, instruction encoding, addressing modes,
alignment, immediate ranges all differ. **Q1's "neutrality has a boundary" result
cannot be extrapolated to the ISA axis.**

### Already settled, not re-opened here
1. The IR shape (typed three-address, single `Word`) — reused from Q1 unchanged.
2. The three payloads' semantics — reused from Q1 unchanged.
3. Whether the lowerer lives in or out of the kernel — that is Q2. This experiment
   measures the per-ISA lowerer size but does not decide its placement.

---

## 1. Hard constraints (violation = invalid)
1. **IR unchanged.** `spec/ir.rs` and `payloads/payloads.rs` must be **byte-identical**
   to Q1. If the IR must change to admit aarch64, ISA-neutrality has failed → record
   as the headline leak. (Detector: any urge to add an ISA hint / second value type /
   escape hatch is the disease to *detect*, not satisfy.)
2. **Clean-room encoder.** The A64 encoder is written from published encodings, and
   every instruction word is cross-checked against LLVM ground truth
   (`rustc --emit obj --target aarch64-*`), since no aarch64 host is available.
3. **Minimal, not good.** The encoder emits only the ~20 instructions the three
   payloads need, naive stack-slot (no register allocation). Building a competent
   aarch64 backend is the failure mode; a *minimal usable* one is the goal.

## 2. Minimal experiment: same IR, second ISA
| dim | choice | why |
|---|---|---|
| ISA | add **aarch64** beside Q1's x86-64 | second real ISA; maximally different from x86 |
| targets | aarch64-Linux (SVC syscalls) + aarch64-Windows (symbol reach) | fills the 2×2 (ISA × OS) matrix so the three-way split is measurable |
| payloads | Q1's three, verbatim | continuity + comparability with Q1's numbers |
| execution | byte-measured + encoder-validated only | no aarch64 host / no qemu on this Windows-x86 box; same honest posture as Q1's SysV side |

## 3. Criteria (fixed BEFORE writing the encoder; not changed after)
| # | criterion | how measured | nature |
|---|---|---|---|
| **①** | **ISA-neutrality gate** | IR + payloads compile to aarch64 **unchanged** (byte-identical files); pure_compute lowers identically for the two aarch64 targets | **boolean** |
| **②** | **three-way LOC/byte split** | reclassify all lowerer code into {shared / per-ISA / per-target}; Q1's two-way split fails on the ISA axis | **list + quant** |
| **③** | **per-ISA share size & growth (MAIN)** | size of the per-ISA bucket (encoder + generic lowering), LOC; does it grow with intents or only with ISA count? | **slope** |
| **④** | **kernel per-ISA delta** | the four-primitive kernel's `.text` size, x86 vs aarch64 (Q0 baseline ~2.7 KB) | quant |
| **⑤** | **ISA-axis leak list** | new leaks vs Q1's ABI-axis L1–L5 (immediate range, alignment, register pressure, reach-set shifts) | **list** |
| **⑥** | **cost of neutrality** | emitted aarch64 bytes ÷ baseline, Q1 口径 (% of prior blob) | quant |

## 4. Decision rule + kill criterion (pre-registered)
1. **① is the gate.** If the IR must be edited to admit aarch64 → ISA-neutrality
   falsified for the payload subset; stop and report.
2. ① passes → **③ is decisive**: if the per-ISA share (a) grows with intents/OS
   count (multiplicative), OR (b) is large *and* linear in ISA count such that "build
   N kernels" is not cheap → **§0.2 assertion falsified**, the N×M→N collapse fails,
   the track's cost model must be rebuilt.
3. If the per-ISA share is **bounded and constant-in-intents** (linear in ISA count
   only) and the kernel per-ISA delta is small → assertion **holds**, collapse stands,
   with a newly-quantified per-ISA line item.
4. ⑥ > 200% → cost-too-high flag (same ceiling as Q1).
5. **Time box:** stop when ①②③④⑤⑥ have numbers for aarch64. No third ISA, no
   register allocator, no optimization, no execution engine.

## 5. Directory
```
research/dynamic-core/isa/
├─ spec/ir.rs            reused from Q1 (byte-identical)
├─ payloads/payloads.rs  reused from Q1 (byte-identical)
├─ lower/a64.rs          aarch64 encoder — PER-ISA
├─ lower/common_a64.rs   aarch64 generic lowering + AAPCS64 placement — PER-ISA
├─ lower/a64_linux.rs    aarch64 Linux (SVC) — PER-TARGET
├─ lower/a64_win.rs      aarch64 Windows (symbol) — PER-TARGET
├─ kernel/prim.rs        four primitives, both ISAs — for ④
├─ kernel/textsize.rs    ELF .text reader (no objdump on host)
├─ main.rs               driver: lower 3 payloads ×2 targets, validate encoder
└─ RESULTS.md            ①–⑥ + leak list + reproduce
```

## 6. Excluded options
| option | why excluded |
|---|---|
| a third ISA (riscv) | time box; two ISAs already give the slope |
| register allocator / optimizer | would confound the size measurement; naive is comparable to Q1 |
| standing up qemu-aarch64 | not available; would consume the whole budget for marginal gain over LLVM-validated encoding |
| a new payload | breaks continuity with Q1's numbers |

## 7. Not answered here
- lowerer placement (in/out of kernel) — Q2.
- a third ISA's marginal cost (this gives the first ISA-axis slope point; a fourth
  data point would confirm linearity).
- behavioural equivalence of the two ISAs' lowerings as a structural invariant — Q4.

## 8. Conclusion — see `research/dynamic-core/isa/RESULTS.md`
Verdict: **§0.2 assertion holds; the N×M→N collapse stands**, with a newly-quantified
per-ISA line item (encoder + lowering ~300 LOC, kernel +13%) that is **bounded and
constant-in-intents, linear in ISA count only**. Full numbers, leak list, and the
three-way split in RESULTS.
