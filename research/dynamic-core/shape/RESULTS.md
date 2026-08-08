# Q20 — The shape boundary of ④ `call` (solution hunt for R6) — RESULTS

Decisive experiment for the residual **R6** named in
[`../SYNTHESIS.md`](../SYNTHESIS.md) §③ and first measured in
[`../primitives/RESULTS.md`](../primitives/RESULTS.md) (Q6 ①'s caveat): ④ `call`
only expresses the integer/pointer word subset — float/SIMD, struct-by-value,
varargs, `sret` are unreachable **at any arity**, a *shape* boundary, not an
*arity* slope. **Question:** can float and struct-by-value be pushed onto the
already-established **placement axis** (Q1's NON-leak: ABI mechanics are
deferrable to the lowerer, zero IR/kernel involvement) instead of requiring a
sixth primitive kind — checked, not assumed?

Clean-room. Reuses only the ①②③④ **contract** (`core/kernel.rs`, not copied)
and the neutral-IR "one value type: Word" discipline
(`ir/spec/ir.rs`, not copied). Real Windows Server 2022 / x86_64 box.

---

## Verdict — **float 走摆放轴可达（R6 收窄）；struct-by-value（≤ 寄存器宽）已经不需要扩展；varargs 部分可解、协议元数据仍是永久残留**

Three separate, checked findings, not one blanket "solved":

1. **Float IS a placement-axis extension, not a new primitive.** A `Kind`
   tag (`Int`/`Float`) added to the *signature description* — still one
   storage type (a `u64`), matching the IR's "one value type: Word" rule —
   lets the SAME dispatch shape (`transmute`-to-typed-fn, one match arm per
   tested signature shape) delegate XMM-vs-GPR placement entirely to the
   host's win64-ABI-aware codegen, **exactly the way register/spill/shadow-
   space placement was already deferred in Q1**. Four real Win32/CRT calls
   (`sqrt`, `pow`, `ldexp`, `PtInRect`) — including a **mixed float+int at
   different positions** case that is the one place a naive port of the
   SysV rule would silently corrupt the result — all **executed correctly
   on the real box** with **exact** expected values. **[真机执行] Q20 ①**

2. **Struct-by-value, when it fits in one register (≤ 8 B, all-integer
   fields), needs ZERO extension to ④.** `POINT` passed by value to
   `PtInRect` goes through the **unmodified** arity-only, all-word baseline
   — it is bit-indistinguishable from passing a `u64`. The only new fact
   the caller needs is the struct's **field order** (which field is the low
   32 bits) — that is an `offsetof`-class baked-layout fact, i.e. it drops
   into the **already-known R4/Q6/Q13 residue**, not a new call-primitive
   gap. **[真机执行] Q20 ①**

3. **Varargs and `sret`/struct-by-value-over-8B are NOT solved by this
   round, and the reasons are different for each — reported honestly, not
   forced to a uniform "closed".** See ④ below.

**The main criterion (②) verdict: NO fifth-call-class primitive was added.**
Every new capability entered as **data** consumed by the existing dispatch
shape. The one place a deeper problem was checked for and found — SysV's
per-register-class counters vs Win64's positional rule — **is real** (worked
by hand, `sysv_reasoned::sysv_register_plan`, structural only, no WSL) but
**resolves to a 6-line two-counter loop, not a new primitive or a new value
type.** Nothing forced a sixth primitive kind into existence.

---

## Measurement conditions

| | |
|---|---|
| Host | Windows Server 2022 Datacenter 10.0.20348 (**real machine**), x86_64 |
| Compiler | `rustc 1.97.0 (2d8144b78 2026-07-07)`, `-O`, MSVC host, raw `rustc` (no Cargo) |
| Real native targets called | `msvcrt.dll!sqrt`, `msvcrt.dll!pow`, `msvcrt.dll!ldexp` (all present, verified), `user32.dll!PtInRect` (present, verified) |
| Reach mechanism | ③ unchanged: `LoadLibraryA` + `GetProcAddress`, mirroring `core/kernel.rs::sym` (not copied) |
| Baseline for LOC comparison | `core/kernel.rs::call` Windows arm (lines 237–268, **32 LOC**, arity-only 0..=10 match) + Q1's own reference figure "ABI placement ~20–30 LOC/target, fixed" (`ir/RESULTS.md` ⑤) |
| ISA / other OS | x86_64 only. SysV register-class rule is **structural only** (`#[cfg(target_os = "linux")]`, never type-checked on this host — same posture `core/kernel.rs`'s own Linux arms already have when built for Windows) — **no WSL, consistent with full-track posture** |

---

## ① Boolean gate — real float/struct-by-value calls, exact values, real machine

```
[PASS] sqrt(144.0) == 12.0  got 12
[PASS] pow(2.0, 10.0) == 1024.0  got 1024
[PASS] ldexp(1.5, 4) == 24.0  got 24
[PASS] PtInRect(rect[0,0,100,100], (50,50)) == true  got true
[PASS] PtInRect(rect[0,0,100,100], (200,200)) == false  got false
[PASS] baseline call_word still works (GetCurrentProcessId)  pid=93364

== Q20 RESULT: ALL PASS ==
```

- **`sqrt`** — 1 float arg, float return. Simplest case: XMM0 in, XMM0 out.
- **`pow`** — 2 float args (XMM0 **and** XMM1). Confirms multi-float-register
  placement, not just single-register.
- **`ldexp(double, int)`** — **the decisive case**. Float at position 0, int
  at position 1. Win64's rule is **positional**: argument *i* uses GPR-list[i]
  or XMM-list[i] purely by index, so the `int` here must land in **RDX**
  (the 2nd position's GPR), not RCX (the 1st GPR overall). Nothing in this
  file names a register — Rust's own win64 codegen made that placement
  decision from the `Kind` data alone, and the exact result (`24.0`, not a
  garbage value from reading the wrong register) is the proof it landed
  correctly. **This is the specific risk the task flagged ("混合整数/浮点参数
  排序规则的复杂度") and it is empirically closed for Win64.**
- **`PtInRect`** — struct-by-value. Both a positive (`true`) and a negative
  (`false`) case, ruling out a test that would pass by accident (e.g. always
  returning nonzero).
- **Regression check** — the pre-existing all-word baseline
  (`GetCurrentProcessId`, arity 0) still runs through `baseline::call_word`
  unmodified, confirming the extension is additive, not a replacement.

**Gate: PASS.** All four real-API tests, all exact match, on the real box.

---

## ② Is float support a new primitive, or a data extension? — **DATA (main criterion)**

Judged by construction, per class:

| capability | mechanism used | verdict |
|---|---|---|
| **1 float arg/ret** (`sqrt`) | new `(&[Kind::Float], Kind::Float)` match arm in `call_shaped`; same `transmute`-to-typed-fn dispatch shape as baseline | **DATA** — a `Kind` tag consumed by the existing shape |
| **2 float args** (`pow`) | same shape, wider arm | **DATA** |
| **mixed float+int, positional** (`ldexp`) | same shape; the positional rule is enforced by the **host codegen**, not by anything this file writes | **DATA** — no register logic added by us at all |
| **struct-by-value ≤ 8 B, all-integer fields** (`PtInRect`'s `POINT`) | **unmodified baseline** `call_word` (arity 2, both Int) — the struct is bit-packed into a `u64` by the *caller*, before the call primitive ever sees it | **NOT EVEN AN EXTENSION** — pre-existing ④ already covers it once the caller supplies field order (a baked layout fact, Q6/Q13's residue, not a call-primitive gap) |

**No sixth primitive was introduced anywhere in this file.** `call_shaped`
is still ④: "invoke a native address given a data description of the
arguments" — the description just grew one more field per argument (a
`Kind` tag) alongside the existing word value. This is structurally the
same move Q1 made for ABI placement (register/spill/shadow-space — all
derived from the semantic signature, zero IR change) and the move Q7 made
for OS-interface content (renumbering as data) — **it is the SAME axis
(摆放/placement, not 机制/mechanism), now shown to also cover the
*type-class* of a word, not just its numeric value.**

**Where the candidate's own honesty check (§2 point 1) is answered:**
"does float exposing a deeper problem (float return register, mixed
ordering complexity)?" — **checked, found real, and resolved cheaply**:

- **Float return register**: tested directly (`sqrt`/`pow`/`ldexp` all
  return `f64` via XMM0) — **no separate handling needed**, the same
  `Kind` tag on the *return* position picks the right register class via
  the same host-codegen delegation. No extra code beyond the `ret: Kind`
  parameter already in the function signature.
- **Mixed ordering complexity**: **real**, but only across ISA-ABI
  *families*, not within one. Win64's rule is positional (cheap: index by
  position). SysV's rule needs two separate running counters (int-class,
  float-class) — coded structurally in `sysv_reasoned::sysv_register_plan`
  (18 LOC including two 6/8-entry register-name tables; the counter loop
  itself is ~8 LOC). **This is exactly the same shape Q5 already found for
  the ABI-placement axis in general** ("I5 — the ABI placement axis's shape
  itself varies by ISA/OS", `ir/RESULTS.md` / `SYNTHESIS.md` §1.4) — Q20
  adds one more instance of a fact already established, it does not
  discover a new category of problem.

**Struct-by-value's honest boundary (>8 B, or non-uniform field classes):**
Q20 did **not** find and execute a real Win32/POSIX system API that
*requires* a struct-by-value parameter larger than one register (searched,
by literature/memory, kernel32/user32/gdi32/msvcrt/common POSIX libc
signatures — RECT/LOGFONT/STARTUPINFO-class structures are, without
exception among the ones checked, passed **by pointer** in the real OS
APIs surveyed). Reasoned (not executed) extrapolation from the
already-established **L2 mechanism** (Q1: "the lowerer injects
target-only constant args, semantic arity ≠ native arity" — already
executed and proven, `CreateProcessA` 10 native args from 0 semantic args)
plus **Q6/Q13's baked-`sizeof`** class: a >8 B struct-by-value C call
compiles (per the real Win64 ABI) to "caller copies the bytes to a fresh
stack/heap buffer, passes ITS ADDRESS instead" — which is the **same
transformation shape** as L2's constant-arg injection, needing one more
baked fact (**total size**, not per-field offsets) supplied to the
lowerer. **This is argued, not measured — flagged honestly as the gap this
round did not close empirically**, because no real target API was found
to execute it against inside the time box.

---

## ③ Cost — LOC delta against the established baselines

| component | LOC (non-blank, non-comment) | note |
|---|--:|---|
| **established baseline** — `core/kernel.rs::call`, Windows arm | **32** | cited, not re-measured (unchanged file, lines 237–268) |
| this file's local re-anchor of the same baseline shape (`baseline::call_word`, arity 0–2 only) | 15 | present in this file so the delta below is measured against a real, compiled, present artifact, not a citation across files |
| `call_shaped` — **whole function** (baseline-equivalent arm + 3 new float arms + panic fallback) | 23 | |
| — of which, **NEW capability only** (3 float-involving match arms) | **~12** (4 lines × 3 arms: pattern, `let` transmute, call, closing brace) | **this is the number to compare** |
| `Kind` enum definition (2 variants, incl. derive attribute line) | 4 | one-time, not per-capability |
| `pt_in_rect` wrapper (struct-by-value marshalling) | 5 | **0** of which are inside `call_shaped` — struct-by-value added nothing to the call primitive itself |
| `sysv_reasoned::sysv_register_plan` (structural, unexecuted) | 18 (incl. 2 register-name tables, ~8 LOC of tables + ~8 LOC counter loop) | shows the SysV-vs-Win64 divergence costs a **two-counter loop**, not a new mechanism |

**Reading:** the float capability's marginal cost is **~12–16 LOC** (3 match
arms × 4 lines + the `Kind` enum's 4 lines) — **well inside** Q1's own "~20–30 LOC/target, fixed"
figure for the ABI-placement axis, and **smaller** than the pre-existing
32-LOC arity-only baseline it extends. Struct-by-value (≤ register width)
costs **0 LOC inside ④** — its only cost (5 LOC) is *caller-side* byte
packing, not primitive machinery. This is consistent with Q1's central
finding that ABI-placement work is cheap and fixed per target; Q20 shows
the *type-class* dimension of that same work is equally cheap.

**Honest caveat on generality (does not change the verdict, qualifies its
scope):** `call_shaped` here is a **hand-picked, finite set of shapes** (4
signatures), the same way `core/kernel.rs::call`'s baseline is a
**hand-picked, finite set of arities** (0..=11). Both are static Rust
`match` tables. Scaling either one to **fully arbitrary** arity×kind
combinations (a real libffi `ffi_prep_cif`-equivalent) would require
either (a) a combinatorially large match table (impractical past a few
positions), or (b) a genuine small in-primitive code generator that writes
register-move instructions from the `sig` data at call time — i.e. ④'s
*own* implementation becomes a tiny JIT built out of ①②, which is still
**not a sixth primitive** (④ built from ①② is exactly the architecture's
own composition rule) but **is more machinery** (a rough estimate,
**not implemented, not measured**: on the order of 60–150 LOC for a
minimal x86-64 register-mover, an **estimate, unimplemented**) than the
few-arm match table demonstrated here. **This experiment proves the
boolean gate and the no-new-primitive verdict for the tested shapes; it
does not claim the finite-match-table approach scales to unbounded arity
without further (still primitive-preserving, but real) work.**

---

## ④ Residual — R6 revised

| construct | pre-Q20 status (R6) | post-Q20 status |
|---|---|---|
| **float / double args + return** | shape boundary, unreachable at any arity | **CLOSED for the tested cases** — data extension on the placement axis, ~12 LOC, real-machine executed (①②③ above). Generalizing to *unbounded* arity/kind combinations is an **estimated, unimplemented** cost (60–150 LOC mini-codegen), not a primitive gap. |
| **SIMD (vector register) args** | shape boundary | **NOT tested — genuinely out of scope** (task explicitly excludes SIMD). No claim made either way. |
| **struct-by-value, ≤ 1 register (≤ 8 B, uniform-class fields)** | shape boundary | **CLOSED, and it turns out to need no extension at all** — degenerates to the pre-existing all-word baseline once the caller bakes field order (an already-known R4/Q6/Q13 residue). |
| **struct-by-value, > 1 register (or mixed field classes needing eightbyte/HFA classification)** | shape boundary | **NOT closed empirically this round.** Reasoned (not executed) that it reduces to L2's already-proven "lowerer injects a marshaled arg" mechanism plus a baked `sizeof` fact — **no fifth primitive predicted**, but **no real system API was found and run against it inside the time box**. Honest gap, not claimed solved. |
| **`sret`** (large aggregate return via hidden pointer) | shape boundary | **Same reasoning as struct-by-value > 1 register** (it is the same ABI mechanism, mirrored: callee writes through a caller-supplied pointer instead of a register). **Not executed.** Argued to reduce to the same L2-class transform; **not measured**. |
| **varargs** | shape boundary, assumed likely permanent | **SPLIT, and the split is the finding.** A *finite, enumerable* set of call shapes chosen by a **static** branch (e.g. "this call site sometimes has 1 int arg, sometimes 2" — each shape gets its own `Inst::Call` site in the IR, selected by an ordinary `BrCond`) is **already fully expressible** by this round's mechanism — every concrete shape is just another data-classified match arm, exactly like `sqrt`/`pow`/`ldexp` were. **What remains permanent**: (a) genuinely **runtime-determined arity/kind** (e.g. parsing a `printf`-style format string at runtime and constructing a call no `Inst::Call` site anticipated) is not expressible — the neutral IR's `Call` arity is fixed at IR-authoring time (`Vec<Val>` length is static), so an unbounded family of shapes needs either an unbounded family of static call sites (impractical) or real runtime codegen; (b) the **protocol metadata** some varargs ABIs require independent of any argument (SysV: `AL` register = count of SSE registers used, so the *callee* can save the right ones) is a fact **about the call site's own shape**, supplied as an extra out-of-band value — expressible as one more `Kind`-adjacent datum if the shape is static, but **not derivable** if the shape itself is runtime-chosen. **Verdict: varargs over a closed, finite, compile-time-known shape set — solved by this round's mechanism. Varargs over a genuinely open/runtime-determined shape set — confirmed permanent, for the reason stated, not by assumption.** |

**R6, revised for `SYNTHESIS.md` (if adopted upstream):**
> ~~float/SIMD, struct-by-value, varargs, `sret` — in any arity, unreachable~~
> → **float (non-SIMD): closed, data extension, ~12 LOC (Q20).**
> **Struct-by-value ≤ register width: closed, needs no extension (Q20).**
> **Struct-by-value > register width / `sret`: reasoned closeable via the
> existing L2 marshalling mechanism, not executed this round — open, not
> permanent (Q20, honest gap).**
> **Varargs over a closed/finite shape set: closed (Q20). Varargs over a
> genuinely runtime-determined shape set: permanent (Q20, reasoned from the
> IR's static-arity `Call` instruction, not assumed).**
> **SIMD: untouched, out of scope by design (task exclusion), no claim.**

---

## Decision trace (spec §4 tree, walked)

1. **① boolean gate.** 4 real Win32/CRT calls requiring float and/or
   struct-by-value, exact-value checked, real machine. **PASS** — 6/6
   checks green including the positional-ordering and both-branches
   struct-by-value tests. → continue.
2. **② main criterion.** Every new capability classified: float (all 3
   shapes) = data extension of the existing dispatch, delegated placement
   to host codegen, **no new primitive**; struct-by-value ≤ register width
   = **zero extension**, pre-existing baseline suffices. The one place a
   deeper problem was actively checked for (mixed-order register class
   rule) was found real and resolved at 6–8 LOC, not by adding machinery
   of a new kind. **Verdict: DATA, not a new primitive class**, for every
   tested case.
3. **③ cost.** ~12–16 LOC for float (under Q1's 20–30 LOC/target reference
   figure); 0 LOC inside ④ for struct-by-value ≤ register width; 18 LOC
   structural (unexecuted) for the SysV counter-loop variant.
4. **④ residual.** R6 narrows: float (non-SIMD) and small struct-by-value
   exit the residual list. Large struct-by-value/`sret` stay **open but
   reasoned-closeable** (honest gap, not executed). Varargs splits into a
   closed finite-shape case and a permanent runtime-shape case, with the
   permanence argued from the IR's static call arity, not assumed.

**No kill criterion was tripped.** The float-as-placement-axis hypothesis
was checked, not assumed, and held on all four real-API tests including
the one case (`ldexp`, mixed ordering) most likely to expose a hidden
mechanism gap.

---

## Reproduce (third-party runnable)

```powershell
cd research/dynamic-core/shape
mkdir out 2>$null
rustc --edition 2021 -O -A dead_code main.rs -o out/harness.exe
cd out
./harness.exe          # runs all 6 checks on real Windows, prints PASS/FAIL, exits 0/1
```

`sysv_reasoned` is behind `#[cfg(target_os = "linux")]` and is never
type-checked by the command above (matches `core/kernel.rs`'s own posture
for its Linux arms when built on Windows) — it is source-present for LOC
counting and manual inspection, not compiled or executed here.

---

## Deviations from the pinned criteria

1. **No real Win32/POSIX system API >8 B struct-by-value was found and
   executed.** The pinned criteria asked to "如实判定" whether such an API
   exists and whether the avoidance strategy holds; the honest answer after
   a literature/memory survey (no internet access, clean-room) is "none
   found among kernel32/user32/gdi32/msvcrt/common POSIX libc signatures
   checked" — reported as a **gap**, not stretched into either "proven
   impossible" or "proven solved."
2. **SysV register-class counter code is structural only** (never
   type-checked on this host, no WSL) — flagged inline with the track's
   standard `[结构推断，未编译于本机]` tag, same posture as every other Q's
   SysV-side artifacts in this track.
3. **`call_shaped`'s shape table is a hand-picked finite set (4 signatures),
   not a general `(arity, [Kind]) -> dispatch` scheme.** This is recorded
   explicitly in ③ as a scope caveat on the "cheap" cost number — it is not
   claimed to be a general solution for arbitrary arity/kind combinations
   without further (estimated, unimplemented) work.
4. **`Kind::Display` (the `fmt::Display` impl) is cosmetic** (used only in
   the `call_shaped` panic message) and is not counted in any of the "new
   capability" LOC figures in ③.

**Honesty clause:** no metric was adjusted to flatter the result. The
struct-by-value-over-8B and `sret` gaps are reported as **open, reasoned,
not executed** rather than silently folded into "R6 solved" — the task's
explicit warning against "为了消灭残留而假装它消失了" is taken at face
value: three of R6's four items narrow with real evidence, one narrows
only by argument, and varargs is split rather than declared uniformly
closed or uniformly permanent.
