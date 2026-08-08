# Q16 — Integration validation (the seam audit) — RESULTS

**Question.** Sixteen experiments each measured one axis in isolation. Bolt the decided
parts into ONE binary, run the three canonical payloads through the combined pipeline,
toggle each part: **do the seams fight? Which parts cannot coexist?**

**The main product is ② (the seam list), not ① (the boolean gate).** ① passing is good
but not the point — the point is to find the seams.

**Host / build (one 口径 for everything here).** Windows Server 2022 x86_64 (real
machine), `rustc --edition 2021 -O -A dead_code`, MSVC host, std. Every part reused
**verbatim via `#[path]`** — no reimplementation, clean-room preserved. All three payloads
executed with **real OS effects** (real `input.txt` read, real stdout, a real `cmd.exe`
child spawned).

Reproduce:
```
rustc --edition 2021 -O -A dead_code research/dynamic-core/compose/main.rs \
  -o research/dynamic-core/compose/out/compose.exe
research/dynamic-core/compose/out/compose.exe
```

Parts assembled: Q1 IR+payloads · Q9 interpreter · Q15 policy interpreter · Q7 table
marshaller · Q4 structural equivalence guard (over Q1's sysv64/win64) · Q13 declare
detection · Q3 content addressing · Q6 five-primitive arity (observed via CreateProcessA=10).

---

## ① Combined boolean gate (measured, real machine)

| payload | interp Q9 | policy Q15 | table Q7 (win) | hand Q1/Q4 (win) |
|---|---|---|---|---|
| `pure_compute` | **163** | **163** | 281 B | 281 B |
| `read_hash_print` | **0** (prints `e78106feb44f1a27`) | **0** (same) | 1216 B | 1249 B |
| `spawn_echo` | **7** (prints `exit=07`) | **7** (same) | **REFUSED** | 1557 B |

Native execution proof (pure_compute, no ctx): table(Q7)→163, hand(Q1)→163, interp→163,
reference 163. **All backends agree.**

**Per-part toggle result.** The interpreter family (Q9, and Q15 with every
instruction-layer check ON) runs **all three** payloads end-to-end. The **codegen family
splits**: the Q1 hand-lowerer covers all three; the **Q7 table marshaller covers 2 of 3 —
it structurally refuses `spawn_echo`** (SpawnWait has no table row). So *"all parts enabled
for all three payloads"* is **impossible by construction** if Q7 is the chosen OS-seam part
— exactly Q7's own ⑤ boundary (orchestration is code, not data), reconfirmed under
composition, not a new break.

---

## ② Seam conflict list (the product)

Nature legend: **TRUE** = real conflict · **SURFACE** = looks like a fight, isn't ·
**DESIGN** = needs a design decision · **COST** = coexists but at an integration cost.

### S1 — Q7 marshaller × `spawn_echo` (SpawnWait) — **TRUE (already-known boundary)**
Measured: Q7 `marshal::lower(spawn_echo)` panics (`SpawnWait` → `None`); caught as REFUSED.
The composed system **cannot use a single OS-seam mechanism across the payload set**: spawn
must fall back to hand-code (Q1) or interpretation (Q9). This is Q7 ⑤ / SYNTHESIS **R1**
(L3b orchestration), so no SYNTHESIS change — but it is the concrete proof that *"the
payload set spans a seam Q7 cannot cross,"* i.e. the parts are **not uniformly composable**
across payloads.

### S2 — Q4 structural guard × the interpreter floor — **TRUE (the headline finding)**
Q4's guard (`VerifiedArtifact::build`) is a **construction gate that compares TWO native
lowerings region-by-region** (Tier A, no execution). Measured congruence over all three:

| payload | congruent | neutral (byte-verified) | intent (quarantined, unverified) | whole-image identical |
|---|---|---|---|---|
| `pure_compute` | OK | 88% | 0% | **true** |
| `read_hash_print` | OK | 54% | 41% | false |
| `spawn_echo` | OK | 40% | 56% | false |

The teeth are real (a mutated neutral byte → `Err`), but the guard **needs ≥2 codegen
backends to exist**. The **interpreter (Q9/Q15) emits no bytes — there is nothing to
region-compare.** So on exactly the platform where the whole track says you MUST interpret
(ACG-hardened process, iOS — Q8/Q12), Q4's build-time structural guard is **inapplicable**:
zero codegen lowerings to cross-check. It degrades to Tier B (differential execution:
run interp, compare to a lowering) — but **under ACG you cannot PRODUCE the lowering to
compare against** (gate 1 blocks codegen), so it collapses to Tier C.
**The crown-jewel positive result (Q4: execution-free, un-forgettable structural
equivalence) and the portability/hardening floor (interpretation) do NOT compose on the
hardened platform.** → SYNTHESIS change required (below).

### S3 — Q4 guard × Q7 table-driven marshalling — **SURFACE + COST**
Orchestrator's question: *does the region model still hold when intent content moves from
code into data, and who verifies the data?*
- **Region model holds.** Q7's non-call bytes are copied verbatim from Q1's `common.rs`,
  so **Neutral regions stay byte-identical**; the Intent region is quarantined by Q4
  whether its content came from code or a table. No conflict there.
- **Who verifies the data:** *not Q4.* The `OpSpec` table is checked by **Q7's own schema
  validator** (`marshal::validate` — arity/ctx/struct-offset well-formedness). So the two
  are **complementary**: Q4 quarantines the intent *bytes*; Q7 schema-checks the *table*
  that produced them. Together they cover more of the intent region than either alone.
- **COST:** Q4's guard rides on **region instrumentation living in `equiv_lower.rs`** (a
  modified copy of the Q1 lowerer). **Q7's `marshal.rs` has no region markers.** Running
  Q4 over Q7 output requires re-threading instrumentation through the marshaller — **the
  equivalence guard is lowerer-specific, not a drop-in over an arbitrary backend.**
- **Residual:** nobody verifies the marshaller *faithfully emits what the table says*
  except by executing it (Tier B).

### S4 — Q3 content addressing × Q4 equivalence — **TRUE divergence → DESIGN decision**
Measured (`read_hash_print`, win64): hand(Q1) 1249 B id=`ce0df597cbebdeaa` vs table(Q7)
1216 B id=`63168b4169f523e6` — **behaviourally equivalent, byte-different**.
- Q3 identity = **byte hash over any two blobs** → **sees TWO adapters**.
- Q4 identity = **structural congruence between (sysv, win) of ONE IR+backend** → it
  **cannot even pair** hand-win vs table-win (wrong pair shape).
They define "same?" over **different relations AND different pairs** — *"who is right"* is a
category error. The actionable finding: **CA distributes the blob stripped of the IR, and
Q4 needs the IR to re-lower and compare.** Once an adapter is a content-addressed blob,
Q4 is inapplicable — the two sit at **incompatible lifecycle points** (Q4 = build-time on
IR; Q3 = distribution-time on bytes). And the interpreter backend has **no bytes at all**
→ content-id is undefined. Design decision: canonicalise (pick one lowering) *before* CA
storage, or accept byte-level fragmentation (Q3's own R7). Sharpens SYNTHESIS **R7**.

### S5 — Q15 policy × Q7 tablification — **SURFACE (no conflict; confirms Q15 ④)**
Measured: deny-`SpawnWait` → `Err(IntentDenied)`. The allowlist **keys on `Intent` in the
IR extern table**, which Q7 keeps as the table KEY (`win_table(intent)`), so **the gate
point survives tablification**. BUT it is an *interpreter-loop* chokepoint; Q7-lowered
native code has no loop, so to keep the gate under Q7 the allow/deny must **move to
bind-time** (marshaller refuses to emit a disallowed Call — O(1)/intent-type). The
per-instruction Q15 checks (bounds/step/taint) do **not** survive into native code (they'd
need O(ops) emitted guards). This is exactly **Q15 ④** — composition **re-confirms** the
convergence, it does not break it.

### S6 — Q13 detection × Q9 interpreter — **SURFACE (same result, different trust target)**
Measured: SYSTEM_INFO.dwPageSize detection pass_on_correct=**true**, fire_on_corrupt=**true**
— and this is **backend-independent** (the self-check is just ③+④ API round-trips). BUT
*what it guards* differs: the interpreter's seam bakes layout via Rust `#[repr(C)]`
(compiler-computed offsets, `assert_eq!(size_of::<StartupInfoA>(),104)`); the Q7 table /
Q1 lowerer bake **numeric literals** (104, @0, @4). Q13 detects a wrong **numeric** bake →
**load-bearing for codegen, near-vacuous for the interpreter** (nothing numeric to get
wrong; the trust moved to "the host Rust compiler's repr(C) for this target"). So the
detection *conclusion* is the same, the *thing it protects* is codegen-specific. Refines **R4**.

### S7 — the two "Win64" backends are not interchangeable — **COST (found during assembly)**
Q7's WIN64 uses `WIN_SYMBOLS` (5 symbols) and resolves stdout at **bind time** into
`ctx[2]`; Q1's `win64.rs` uses `SYMBOLS` (9 symbols) and calls `GetStdHandle` at **runtime**
(index 4). Same target name "win64", **different ctx contract and symbol table**. A composed
loader **cannot treat "the win64 lowerer" as one swappable part** — the ctx builder must
know which backend produced the bytes. (Concretely: pure_compute, needing no ctx, ran
uniformly through both; `read_hash_print` would need a backend-specific `Ctx`.)

### S8 — parts are standalone crate roots assuming they own `mod ir/asm/common` — **COST**
Assembly forced choosing **one** `crate::common` (used `equiv_lower.rs`, byte-identical to
Q1's) because `sysv64.rs`/`win64.rs` hard-code `use crate::common`. It worked only because
`equiv_lower` is a verbatim superset. Absolute crate-paths are a real composability tax; two
components wanting *different* `crate::common` could not co-exist without renaming.

---

## ③ Composed cost (one ruler; different from the per-experiment numbers, by design)

- **Composed binary:** **254,976 B on disk** (whole file, std, msvc, `-O`, unstripped). This
  is the union: three backends + the guard + both tables + detection.
  **NOT comparable to any single-Q byte number** — each experiment measured only its own
  slice under a *different* 口径 (some include the OS seam, some exclude it; object `.text`
  vs flat-subtraction vs whole-file). Per COMPARABILITY, these do not divide.
- **The only shared ruler is LOC** (non-blank, non-comment): reused parts total **1884 LOC**
  (+190 for the compose harness). Breakdown: ir 123 · asm 202 · common(equiv_lower) 204 ·
  sysv64 109 · win64 137 · payloads 115 · verify 118 · table 176 · marshal 316 · interp 142 ·
  interp_policy 242.
- **Why the composed total is far less than summing each experiment's self-reported total:**
  the **shared substrate (ir 123 + asm 202 + lowering-frame/common 204 ≈ 529 LOC) is counted
  ONCE in the union** but was re-counted inside every isolated experiment's own figure. The
  isolation that made each Q measurable also **double-counts the substrate N times** — the
  composed system pays for it once. That is itself a composition finding.

---

## ④ Which parts genuinely cannot coexist

1. **Q4's Tier-A structural guard ⟂ an interpreter-only deployment** (S2). This is the one
   *genuine* non-coexistence, and it bites **exactly on the hardened platform (ACG/iOS)
   where the track mandates interpretation.** There is no glue that fixes it: Tier A needs
   two codegen lowerings; the interpreter floor has zero. "The architecture's strongest
   verification result and its strongest portability result do not hold at the same time."
2. **Q7 marshaller ⟂ `spawn_echo`** (S1) — a declared boundary (R1), not new, but it means
   no single OS-seam mechanism spans the payload set.

Everything else **coexists**, at the costs/decisions in S3–S8.

---

## SYNTHESIS conclusions that need modifying (with the fix)

1. **§5.1 trust-graph, row "中立核 / 产出时 (Q4 Tier A/A′)"** and the **"收敛 (全轨最强的一条)"**
   line: the three-axis convergence (Q4 produce-time / Q13 layout / Q15 exec-time) is stated
   as if all three coexist. **Fix:** add — *"Tier A requires ≥2 codegen lowerings to
   region-compare. On the interpreter-only floor (the ACG/iOS deployment from Q8/Q12) there
   are no two lowerings, so Q4's produce-time structural axis is **absent**; only Q13+Q15
   remain, and both are Tier B / execution-time. The convergence therefore holds on the
   **codegen** route; on the interpreter floor it is a convergence of two execution-time
   checks, not the produce-time + execution-time pair the headline implies."*
2. **§5.3 point 1** (*"IR 内的东西，用构造门（Q4 形态）验。便宜、无需执行、不可遗忘"*): **Fix:**
   qualify — *"…on the codegen route. Where the platform forces interpretation, no second
   lowering exists, so the construction gate cannot run; verification there is Tier B
   (differential execution), not Tier A — and under ACG the codegen side cannot be produced
   to differ against, so it is Tier C."*
3. **§附 evidence-strength table, Q4 row "最弱的一环"**: currently *"单 OS 主机 → L1/L2/L4 只能到
   Tier C"*. **Fix:** add *"+ Tier A is codegen-only: an interpreter-only deployment has no
   two lowerings, so the guard is inapplicable exactly where interpretation is forced (Q16)."*
4. **§发现的矛盾**: add **X6 — Q3 content-addressing and Q4 equivalence diverge on adapter
   identity and sit at incompatible lifecycle points** (S4 above); this sharpens R7.
5. **README question board**: add a **Q16 = decided** row (conclusion = the ④ summary above:
   parts compose for the interpreter family end-to-end; the single genuine non-coexistence is
   Q4 Tier-A ⟂ interpreter floor; Q7 excludes spawn (=R1); Q3⟂Q4 identity divergence needs a
   design decision; Q15 allowlist survives tablification (→ bind-time); Q13 detection is
   codegen-specific).

*(These edits are left to the orchestrator's reconciliation — this run did not touch the
shared README / SYNTHESIS / COMPARABILITY files.)*

---

## Deviations from an idealised spec (honesty clause)

- **No spec doc was pre-written** for Q16 (the task itself pinned the criteria before code;
  criteria ①–④ were fixed before writing `main.rs` and not changed after).
- **Execution scope:** all three payloads were executed on the **interpreter family** (real
  OS effects). On the **codegen family**, only `pure_compute` was executed natively (no ctx
  needed); `read_hash_print`/`spawn_echo` codegen was **lowered + byte-measured, not
  executed** in this harness — tables/RESULTS (Q7) and ir/RESULTS (Q1) already executed those
  Win64 products, and re-running them here adds no seam information. The seam findings do not
  depend on executing them.
- **Not for let-me-make-it-look-good:** the most valuable result (S2 / ④.1) makes the
  architecture *worse*, not better — the two flagship results don't hold together on the
  hardened platform. Recorded as the headline, not buried.
- **Time-boxed:** stopped at ① + ②. No optimisation, no second ISA, no product, no attempt
  to "fix" any seam by editing a part's semantics.
