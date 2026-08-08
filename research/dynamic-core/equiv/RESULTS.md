# Equivalence-invariant experiment (Q4) — RESULTS

Decisive experiment for [`plan/design-equivalence-invariant-experiment.md`](../../../plan/design-equivalence-invariant-experiment.md):
**can "one neutral IR, lowered by two independent paths, must be behaviourally
equivalent" be made a STRUCTURAL invariant — one that cannot be forgotten because a
violation blocks producing runnable bytes — rather than an after-the-fact check? At what
cost, and with what coverage ceiling?** Measured on Q1's exact IR + two lowerers, reused
verbatim. Clean-room within the track; no external implementation consulted.

---

## Verdict — **bounded structural achievability (有边界可达)**

- **The invariant IS structural, and it bites.** `VerifiedArtifact` has private byte
  fields and a single construction entry that runs the congruence check; the only way to
  obtain runnable bytes (`win_bytes()`) is through it. A mutated Neutral byte makes
  `build` return `Err` and yields **no artifact** — proven by the negative test
  (`NeutralBytesDiffer`). It is *not* a skippable side script.
- **But it covers only the neutral core.** The guard structurally verifies everything
  **except the intent-call regions** — which are exactly Q1's OS-interface leaks (L1–L5).
  Coverage of the OS-interface content by any structural means is **0**.
- **The ceiling is quantified.** The unverifiable-by-structure fraction (intent bytes) is
  **0% for `pure_compute`, ~30–41% for `read_hash_print`, ~45–56% for `spawn_echo`.** For
  the struct-boundary payload, **more than half the Win64 image has no structural
  equivalence at all.**
- **Two ceilings stack.** Intent regions are (1) structurally unverifiable (no
  byte-identity; for `spawn`, zero shared structure) and (2) on this single-OS host,
  un-executable for SysV — so they cannot even reach behaviour-level (Tier B) checking.
  `spawn`'s SysV `SpawnWait` (fork/execve/wait4, 560 bytes) is pure **Tier C**: no
  structural anchor and no execution path.
- **A Q4-specific finding Q1 did not surface:** even the *neutral core* is **not** whole
  byte-identical for OS-touching payloads. Frame size (M1) and the entry ctx register
  (M2) are ABI facts baked into shared-path bytes; they are byte-*divergent* but
  behaviourally trivial (same opcode, ABI-mandated operand). **Byte-identity is strictly
  stronger than equivalence even inside the neutral core** — which is why the invariant
  needs the region model, not a whole-image `memcmp`.

**Net:** a structural, un-forgettable equivalence invariant is real but **bounded** — it
gives execution-free verification of agent-produced *compute* logic, and it hits a hard
wall exactly at the OS-interface glue. **It does not slide into a correctness proof:** for
zero-shared lowerings it declares "unverifiable" (Tier C) rather than proving fork/execve
≡ CreateProcessA. That restraint is the honest result and it is a coverage *ceiling*, not
a bug to be engineered away.

---

## Measurement conditions

| | |
|---|---|
| Host tool | `rustc`, `--edition 2021 -O -A dead_code`, MSVC host (same as Q1) |
| Base under test | Q1's `ir/` reused verbatim via `#[path]`: `spec/ir.rs`, `lower/asm.rs`, `lower/sysv64.rs`, `lower/win64.rs`, `payloads/payloads.rs`. **Unchanged.** |
| Only local copy | `equiv_lower.rs` = Q1 `common.rs` with region-boundary recording added; emit logic verbatim, so the emitted bytes match Q1 exactly (`pure_compute` 281, byte-identical). |
| New code | `verify.rs` (the invariant + gate + coverage), `main.rs` (driver + Q1 JIT harness). |
| Byte counts | raw emitted code-image bytes, per region kind, from the driver. |
| Execution | Win64 JIT-run against real `kernel32`, **only via `VerifiedArtifact::win_bytes()`**. SysV byte-measured, not executed (no WSL) — same posture as Q1. |

---

## ① Structural achievability — the boolean gate (PASS)

The mechanism (spec §1.1): `check_congruence` walks the two region sequences in lockstep
(both driven by the same IR walk → identical kind-sequence) and enforces
- **Neutral** regions byte-identical,
- **Control** regions target the same block (rel32 may differ from jump-offset drift),
- **Frame / CtxReg / Intent** regions quarantined (not byte-checked).

`VerifiedArtifact` can only be constructed by `build`/`build_from`, both of which run this
check and return `Err` on violation. Driver output:

```
-- criterion ① (gate ran, all passed) + ② (region coverage) --
  pure_compute     identical=true   | sysv total= 281  win total= 281
  read_hash_print  identical=false  | sysv total=1046  win total=1249
  spawn_echo       identical=false  | sysv total=1251  win total=1557

-- criterion ① (negative): mutate a Neutral byte -> build MUST refuse --
  OK: build refused — NeutralBytesDiffer { idx: 0 } — no artifact, no bytes to run

-- execution evidence (Win64; bytes obtained only via VerifiedArtifact) --
  pure_compute  -> 163 (expect 163)  OK
  read_hash_print -> "a49d2cbecc13994f" (expect "a49d2cbecc13994f")  OK
  spawn_echo    -> printed "exit=07" ret=7 (expect "exit=07", 7)  OK
```

Pass conditions (spec §3①): (a) the gate refuses the mutant and yields no artifact — **met**;
(b) the guarded region is non-empty and equals all non-OS logic — **met** (neutral bytes
are identical across targets: 246/246, 676/676, 623/623 — see ②). The gate is not vacuous.

**A note on why a whole-image `memcmp` would NOT be a usable invariant:** it passes only
for `pure_compute`. For OS-touching payloads the images differ (mechanics + intent), so a
naive `bytes_sysv == bytes_win` gate would reject every real payload. The region model is
what lets the structural guard verify the shared core while quarantining the rest.

---

## ② Coverage — the boundary shape (main product)

Per target, bytes by region kind (`n`=neutral, `c`=control, `f`=frame, `x`=ctx-reg,
`i`=intent), and the derived fractions. `struct(A′)` = (neutral+control)/total;
`intent(leak)` = intent/total = **the unverifiable-by-structure fraction**.
"Structurally accounted" = 1 − intent = neutral+control+mechanics.

| payload | target | total | n | c | f | x | i | neutral(A) | struct(A′) | **intent(leak)** |
|---|---|--:|--:|--:|--:|--:|--:|--:|--:|--:|
| pure_compute | sysv | 281 | 246 | 21 | 14 | 0 | 0 | 87.5% | 95.0% | **0.0%** |
| pure_compute | win | 281 | 246 | 21 | 14 | 0 | 0 | 87.5% | 95.0% | **0.0%** |
| read_hash_print | sysv | 1046 | 676 | 42 | 14 | 7 | 307 | 64.6% | 68.6% | **29.3%** |
| read_hash_print | win | 1249 | 676 | 42 | 14 | 7 | 510 | 54.1% | 57.5% | **40.8%** |
| spawn_echo | sysv | 1251 | 623 | 47 | 14 | 7 | 560 | 49.8% | 53.6% | **44.8%** |
| spawn_echo | win | 1557 | 623 | 47 | 14 | 7 | 866 | 40.0% | 43.0% | **55.6%** |

Readings:
- **`pure_compute` is fully structural.** intent=0; the whole image is byte-identical
  (`identical=true`). Everything is verified with no execution.
- **The neutral byte count is identical across targets for every payload** (246, 676,
  623). This is the invariant's positive content: the shared core genuinely matches
  byte-for-byte, so the un-runnable SysV lowering **inherits the Win64 side's execution
  evidence for its neutral core**. Verification transfers across the un-executable target
  — but *only* for the neutral core.
- **The ceiling grows with OS-content.** intent(leak) climbs 0% → ~30–41% → ~45–56% as
  payloads go pure → file-I/O → process-spawn. The Win64 side is always worse (its OS
  calls emit more bytes: `CreateProcessA`'s struct build vs SysV's fork sequence).
- **`spawn_echo` win64: 55.6% of the image is unverifiable by any structural means.** This
  is the L3 struct-layout wall measured as a fraction of the artifact.

---

## ③ Cost of the invariant

**Runtime cost: essentially zero, and — the key point — NO execution required.** The
structural layer is an O(total-bytes) `memcmp` of the neutral regions plus label
comparison, run once at build. It does not run the code, does not need a second OS, and
adds **0 bytes** to the emitted artifact (the guard emits nothing).

**Code cost (LOC, non-comment/non-blank):**

| component | LOC | role |
|---|--:|---|
| `verify.rs` total | 118 | invariant + gate + coverage accounting |
| — of which: `check_congruence` + `VerifiedArtifact` gate | ~55 | **the structural guard itself** |
| — of which: `Coverage` accounting | ~40 | measurement for ② (not needed by the guard) |
| region model + instrumentation in `equiv_lower.rs` | ~60 | `RegionKind`/`Region`/`Rec` + boundary calls (rest is verbatim Q1) |

So the invariant that makes equivalence structural costs **~55 LOC of guard riding on ~60
LOC of region instrumentation that is coextensive with the lowerer's own control flow** —
the boundaries it records (prologue / each intent-call / terminators) are the points the
lowerer already branches on. It slows down nothing measurable; it needs no runtime.

---

## ④ The unverifiable zone — Tier × Leak map (main product)

Highest verification tier reachable for each divergence source. Tiers:
**A** byte-identity (structural, no execution) · **A′** structural congruence
(control-by-label) · **A−** mechanics (same opcode, ABI-mandated operand) ·
**B** observable-behaviour equivalence (needs BOTH targets executed) ·
**C** unverifiable / assert-only.

| source | where | structural? | highest tier (in principle) | on THIS host (Win-only) |
|---|---|---|---|---|
| neutral compute/mem/control | shared path | yes | **A / A′** | **A** (SysV core inherits Win64 execution via byte-identity) |
| **M1** frame-size divergence *(Q4-new)* | Frame region | same opcode, ABI immediate | **A−** | A− |
| **M2** ctx-register divergence *(Q4-new)* | CtxReg region | same opcode, ABI register | **A−** | A− |
| **L1** reach (symbol vs syscall#) | Intent region | no | **B** (differential) | **C** for SysV (unrunnable) |
| **L2** arity / injected const args | Intent region | no | **B** | **C** for SysV |
| **L3** OS struct layout (`spawn`) | Intent region | **no shared structure** | **B** as black-box only | **C** (zero-shared **and** SysV unrunnable) |
| **L4** out-param width | Intent region | no | **B** | **C** for SysV |
| **L5** error/sentinel convention | Intent region | no — the *spec* of "did it fail" is target-dependent | **C** (equivalence cannot even be stated neutrally) | **C** |

**Which leaks make equivalence undecidable (below Tier B):**
- **L5** — undecidable *in principle*: a neutral "did it fail?" predicate cannot be
  written, so the equivalence to check cannot be stated.
- **L3 + this host** — undecidable *here*: zero shared structure (no A/A′) and SysV cannot
  execute (no B). `spawn`'s SysV realization is pure Tier C.
- **L1 / L2 / L4** — decidable at **Tier B** (run both OSes, compare exit/stdout/effects),
  but never structural and never available on a single-OS host. They are after-the-fact
  differential tests — exactly the forgettable check the structural invariant was meant to
  replace, and they only shrink the unverifiable zone if a second OS is on hand.

**The natural coverage ceiling, stated plainly:** the structural invariant covers the
neutral core (Tier A) + control (A′) + mechanics (A−) = **everything except the intent
regions**. The intent regions ARE the OS interface, and their size (② ) is the ceiling:
0% of a compute payload is unverifiable, but ~45–56% of a spawn payload is. No structural
mechanism can close that gap without encapsulating the OS call — which relocates, not
removes, the trust (Q1's core dilemma).

---

## ⑤ Equivalence layering — final definitions (calibrated to the measurements)

| tier | definition | applies where | verification cost | forgettable? |
|---|---|---|---|---|
| **A — byte-identity** | two lowerings byte-identical in a region | straight-line neutral code (no ABI/OS divergence) | static `memcmp`, no execution | **no** (constructor-gated) |
| **A′ — structural congruence** | control transfers target the same block; rel32 may drift | jumps/branches over neutral code | static, compare label ids | **no** |
| **A− — mechanics** | same opcode, ABI-mandated operand (frame size, ctx reg) | prologue/epilogue frame + ctx store | static, lockstep kind match | **no** |
| **B — observable behaviour** | exit code + stdout + observable side effects agree | intent regions where BOTH targets can execute against a shared contract | run both targets (differential test) | **yes** (separable) |
| **C — unverifiable** | assert-only | zero-shared lowerings (L3), target unrunnable on host, or equivalence unstateable (L5) | none possible | n/a |

The structural invariant lives entirely in **A / A′ / A−** (un-forgettable, execution-free).
**B is inherently after-the-fact** (it is differential testing, which can be skipped and,
on one OS, cannot run for the other target). **C is the documented ceiling.** The boundary
between {A,A′,A−} and {B,C} is precisely the neutral-core / OS-interface boundary Q1 found —
Q4 measures it as a *fraction of the artifact* and shows it can be enforced structurally up
to that line and no further.

---

## Reproduce (third-party runnable)

```powershell
cd research/dynamic-core/equiv
mkdir out 2>$null
rustc --edition 2021 -O -A dead_code main.rs -o out/driver.exe
cd out; ./driver.exe
```

The driver builds each payload through the congruence gate, prints per-region coverage,
runs the negative mutant test (must print `build refused`), dumps
`out/<payload>.<abi>.bin`, and — on Windows — JIT-executes each Win64 lowering **only via
the gated `win_bytes()`**, verifying `pure_compute`→163, `read_hash_print`→
`a49d2cbecc13994f`, `spawn_echo`→`exit=07`/ret 7. Independent reference values match Q1.

---

## Deviations from the spec

1. **SysV byte-measured, not executed** (no WSL) — as the cover note permits. Its neutral
   core is verified by byte-identity against the *executed* Win64 core (evidence transfer);
   its intent regions are Tier C on this host (④).
2. **`equiv_lower.rs` is a copy of Q1 `common.rs`**, not a `#[path]` reuse — Q1's
   `lower_inst`/`lower_op` are private and cannot be driven from outside to observe region
   boundaries. Emit logic is verbatim (spec §5); the copy adds only the region sink and
   emits no extra bytes (confirmed: `pure_compute` still 281 and byte-identical).
3. **`Coverage` (~40 LOC) is measurement, not guard** — separated in ③ so the invariant's
   own cost (~55 LOC) is not overstated.
4. Directory compiled as one `rustc` invocation via `#[path]` modules (no Cargo), never
   touching the root workspace — same discipline as Q1.
