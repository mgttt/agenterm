# Q19 — IR structural verification for the interpreter path — RESULTS

**Question (from Q16's S2).** Q4's Tier-A guard is a *produce-time, execution-free,
un-forgettable* gate that works by **comparing two codegen lowerings** region-by-region. A
pure interpreter emits **zero** lowerings, so on the ACG/iOS floor where the track mandates
interpretation there is nothing to cross-check — Q16 concluded the produce-time structural
axis is **absent** there ("the strongest verification result and the strongest portability
result do not compose on the hardened platform").

Candidate solution tested here: don't compare two **outputs** — verify the one thing an
interpreter has, the **IR itself**. Can the interpreter path carry a **Tier-A-class**
(produce-time / no-execution / un-forgettable) **structural verifier over the IR**? Minimal
size? What does it verify / not verify? **Does it patch S2's hole, or an adjacent one?**

**The main criterion is ② — the honest verdict on which hole it actually closes.**

---

## Verdict — **it patches an ADJACENT hole (well-formedness); S2's equivalence hole stays open, but S2 was OVER-STATED**

Two-part result, and the split IS the finding:

1. **S2 as literally worded is REFUTED.** S2 says the produce-time structural axis is
   *absent* on the interpreter floor. It is **not**: a genuine Tier-A gate exists there —
   the IR verifier has **all four** Tier-A properties (produce-time, no-execution,
   construction-gate, un-forgettable), measured at **98 LOC / 634 B**. The interpreter floor
   is **not** barren of produce-time structural verification.

2. **S2's headline STANDS, re-scoped.** Q4's *specific* guarantee is **behavioural
   equivalence of two independent lowerings** — a **relational** property of a **pair** of
   artifacts. The IR verifier guarantees **well-formedness of one IR** — a **unary** property
   of a **single** artifact. These are **different guarantees**. On an interpreter-only floor
   there is exactly **one** artifact, so the equivalence guarantee is **structurally
   unreachable** (you cannot manufacture a second, independently-derived executor to disagree
   with the first). No unary well-formedness check can synthesise a relational equivalence
   check. **So the crown-jewel result (Q4 execution-free EQUIVALENCE) still does not reach the
   interpreter floor** — exactly where interpretation is forced.

**Net (honest, per the honesty clause):** Q19 does **not** dress well-formedness up as
equivalence. It gives the interpreter path a **real Tier-A gate**, but that gate verifies
**"this IR is well-formed"**, not **"this execution is equivalent to an independent one"**.
The hole S2 named — produce-time *behavioural-equivalence* verification — is **structural**
and **stays open** on the interpreter floor. What Q19 closes is the weaker, adjacent claim
that the floor has **no** produce-time gate at all. **S2 goes from "the produce-time axis is
absent" (false) → "the produce-time EQUIVALENCE guarantee is absent; a produce-time
WELL-FORMEDNESS guarantee is present and is Tier-A" (true).**

---

## Measurement conditions

| | |
|---|---|
| Machine | Windows Server 2022 Datacenter 10.0.20348 (real box) |
| ISA / target | x86_64 / `x86_64-pc-windows-msvc` |
| Compiler | `rustc -O` (release), edition 2021 |
| Reuse | `ir/spec/ir.rs`, `ir/payloads/payloads.rs`, `interp/interp.rs` reused **verbatim** via `#[path]`. New code = `verify.rs` only. No Cargo, never touches root workspace. |
| Byte 口径 | **Q9's 口径, reused verbatim** (COMPARABILITY best practice): `rustc -O --crate-type=lib --emit=obj` → `llvm-size` Berkeley **`.text`**, **std + default panic**, unstripped. Cross-calibration: Q9 eval-core recomputed here = **1908 B**, matches Q9's reported 1908 B → same ruler. |
| LOC 口径 | non-blank, non-comment lines (same as Q4/Q9). |
| Execution status | `真机执行` for ① positive/negative gate and ② compose (pure_compute run through the gate → 163); `结构推断` for the equivalence-unreachability argument (a structural fact, not a measurement); B1 oob_store **deliberately NOT executed** (real OOB write) — verify PASSES it without execution, which is itself the point. |

## Reproduce

```powershell
cd research/dynamic-core/verify; mkdir out 2>$null
rustc --edition 2021 -O -A dead_code main.rs -o out/driver.exe
./out/driver.exe
# bytes (Q9 口径) + cross-calibration against Q9's 1908 B
$BIN = "$(rustc --print sysroot)/lib/rustlib/x86_64-pc-windows-msvc/bin"
rustc --edition 2021 -O --crate-type=lib --emit=obj -A dead_code measure_core.rs -o out/verify_core.o
rustc --edition 2021 -O --cfg interp_measure_core --crate-type=lib --emit=obj -A dead_code ../interp/measure_core.rs -o out/q9_core.o
& "$BIN/llvm-size.exe" out/verify_core.o out/q9_core.o
```

---

## ① Boolean gate — negative-probe posture (PASS)

Driver output (`out/driver.exe`):

**Positive — the three real payloads are well-formed → must PASS:**
```
  pure_compute     PASS (well-formed)
  read_hash_print  PASS (well-formed)
  spawn_echo       PASS (well-formed)
```
**Negative — injected bad IR MUST FIRE** (each rebuilds a fresh payload and corrupts one thing):
```
  P1 越界索引 (out-of-range value index)          FIRE  ValOutOfRange { block: 0, val: 9999 }
  P2 未定义 opcode/callee (undefined extern id)   FIRE  ExternIdOutOfRange { block: 0, id: 99 }
  P3 类型/arity 不匹配 (arity mismatch)           FIRE  ArityMismatch { block: 0, id: 0, got: 0, want: 1 }
  P4 控制流跳到非法目标 (CFI: illegal jump target) FIRE  BlockTargetOutOfRange { block: 0, target: 999 }
  P5 rodata 偏移越界 (data-side OOB index)        FIRE  RodataOffsetOutOfRange { block: 0, off: 99999 }
```

All four fault classes the task named are covered: **越界索引** (P1, plus P5 on the data
side), **未定义 opcode** (P2 — the extern-id-past-table is the IR's "undefined opcode"
analog; note the *enum* opcodes are unrepresentable-if-undefined for free via Rust's closed
enums, so the only openable "opcode" hole is the callee id), **类型不匹配** (P3, arity), and
**控制流跳到非法目标** (P4, plus entry-out-of-range). The gate is **not vacuous** — every
probe fires with the correct fault; the three real payloads pass.

## ② Is this Tier-A, and is it the SAME thing Q4 verifies? (main criterion)

**Tier-A checklist (Q4's definition), item by item:**

| Tier-A property | IR verifier | met? |
|---|---|---|
| **produce-time** | runs before `interp::run`, on the IR as data | **yes** |
| **no execution** | one structural walk; no IR executed, no OS touched (proven: B1 oob_store is *verified* without being *run*) | **yes** |
| **construction gate** | `VerifiedModule` has a private field; the only constructor is `verify` | **yes** |
| **un-forgettable** | `run_verified(&VerifiedModule)` cannot be called without a token only `verify` mints (demonstrated composing with Q9's `interp::run`; in production `interp::run`'s signature would take `&VerifiedModule`) | **yes** |

So **it IS Tier-A by every property.** But **it is NOT the same guarantee as Q4:**

| | Q4 guard | Q19 IR verifier |
|---|---|---|
| property shape | **relational** (a pair) | **unary** (one artifact) |
| what it proves | "two independent lowerings of this IR are behaviourally **equivalent**" | "this one IR is **well-formed**" |
| catches | a lowerer bug that makes one path diverge | malformed IR |
| needs | **≥2** artifacts to cross-check | **1** artifact |
| failure it can't see | — | a well-formed IR that the interpreter mis-executes (nothing to cross-check the executor against) |

**Why the difference is decisive for S2.** S2's hole is the *equivalence* guarantee. That
guarantee is a property of a **pair** of independently-derived executors. The interpreter-only
floor produces **one** executor. You cannot get a relational guarantee from a unary artifact:

- Running the interpreter twice compares a thing to itself — catches nothing (Q4's teeth come
  from two *independent* lowerings that *could* disagree).
- Producing a second, codegen lowering to compare against is exactly what ACG forbids (Q16
  gate 1) — and doing so leaves the "interpreter-only floor".
- The IR verifier has only the IR; it proves the **input** is valid, never that the
  **executor** is valid. Q4 proves the executor is valid *by having a second one disagree*.

**Therefore Q19 补的是相邻但不同的洞 (well-formedness), 不是 S2 那个洞 (equivalence).** But
it materially corrects S2's over-statement: the interpreter floor is **not** "产出时轴缺席"
— it carries a Tier-A well-formedness gate. The hole that remains is **narrower** than S2
claimed: not "no produce-time structural verification", but "no produce-time
**behavioural-equivalence** verification".

## ③ Coverage boundary (measured)

**Verifies (produce-time, no execution):** value indices in range; block targets in range
(CFI); entry in range; extern id resolves (undefined-callee); call arity == declared nargs;
rodata offset in range; ≥1 block. In one sentence: **"the IR is a well-formed graph the
interpreter can walk without indexing outside its own arrays, and every call names a real
intent with the right arity."**

**Cannot verify (measured — all three PASS the verifier despite being wrong):**

| boundary | demo | verify result | why it's out of reach |
|---|---|---|---|
| **memory safety** | B1 `oob_store`: Alloc(8) then Store8 at base+64 | **PASS** | runtime pointer values (Alloc/Rodata + arithmetic) are not statically known; catching an OOB deref needs **value-range / abstract interpretation = eBPF's 20k lines**, OR Q15's **run-time** bounds check. The verifier stops **below** that line by design — that is why it is 98 LOC, not 20k. |
| **semantics** | B2 `wrong_result`: Exit 999 | **PASS** | the verifier is not a correctness proof; **well-formed ≠ computes the intended value**. (Undefined-value reads fall here too: the interpreter zero-inits all vals, so an undefined read is memory-safe but possibly-wrong — a semantic, not structural, fault.) |
| **OS seam L1–L5** | B3 `spawn_echo`: rodata=8B, SpawnWait nargs=0 | (payload passes) | the dangerous content (`cmd.exe /c exit 7`, injected FFI constants, `STARTUPINFOA=104` layout) **is not an IR value** — it lives in `do_intent` **below** the IR (Q15 ②, Q9 ⑤). There is nothing structural to check. **Same seam Q4/Q9/Q15 all stop at.** |
| **termination** | (not built) | would PASS | an infinite loop is well-formed; halting is Q15's **execution-time** step limit. |

**The boundary, plainly:** the verifier covers **well-formedness of the IR graph** and
**nothing past it**. Memory-safety, semantics, and termination are **execution-time**
properties (Q15) or need **abstract interpretation** (eBPF); the OS-seam content is the
**L1–L5 crack** no execution method touches (Q9 ⑤). My prediction going in — "it guarantees
no semantics, and can't touch the OS seam" — **held**; the sharper measured line is that it
also **can't reach memory safety** (B1), which is the exact place eBPF spends its 20k lines.

## ④ Cost

| thing | size | 口径 |
|---|--:|---|
| **IR verifier (whole `verify.rs`)** | **98 LOC / 634 B** | Q9 口径 (object `.text`, msvc, std+default-panic) |
| — of which the guard proper (`verify` + `chk`/`tgt`/`each_read`) | ~79 LOC | — |
| — fault taxonomy (`IrFault`) | 9 LOC | — |
| — un-forgettable gate (`VerifiedModule` + impl) | ~8 LOC | — |
| Q4 guard (`check_congruence` + `VerifiedArtifact`) | ~55 LOC | LOC only; emits 0 bytes into any artifact |
| Q9 eval-core (the interpreter it guards) | 1908 B | same 口径 (cross-calibrated here) |
| Q1 lowerer | 14819 B | same 口径 |
| **eBPF kernel verifier** (`verifier.c`) | **20,065 LINES** | reference §4.1f (一手查证) |

Readings:
- **It is small and effective.** 98 LOC / 634 B — **~1/3 of the interpreter's own eval-core
  (1908 B)**, ~4.3% of the Q1 lowerer, and **~0.5% of eBPF's verifier in LOC**. Like Q4, it
  emits **0 bytes into any produced artifact** — it is the verifier's own `.text`, comparable
  to Q9's own `.text`, not a tax on the payload.
- **The size gap to eBPF IS the capability gap (B1).** eBPF's 20,065 lines buy **runtime
  memory-safety** (value-range/abstract interpretation). Our 98 lines buy **well-formedness
  only** and **explicitly do not** get memory safety (oob_store passes). The survey's
  "要 eBPF 的安全就得不到它的体积" is **confirmed from the small end**: the minimal structural
  gate is affordable **precisely because it stops at well-formedness**, below eBPF's
  memory-safety line.
- **So, on the ④ framing:** S2 does **not** cleanly downgrade to "两条路各有各的结构门 (=
  same kind, done differently)". It downgrades to **"两条路各有一个 Tier-A 产出时门，但保证不
  同级"**: the codegen route's gate proves **equivalence** (relational); the interpreter
  route's gate proves **well-formedness** (unary). Both routes can *also* run the
  well-formedness gate; **only the codegen route can run the equivalence gate**, and only when
  ACG permits producing two lowerings.

## ⑤ Net effect on SYNTHESIS (change proposals — NOT applied here; left for the orchestrator)

Q16's SYNTHESIS edits about S2 assume the interpreter floor has **no** produce-time
structural verification. Q19 shows that is too strong. Proposed corrections (the orchestrator
should reconcile against the live SYNTHESIS/README, which this run did **not** touch):

1. **§5.1 trust-graph / the "收敛" line.** Q16's fix #1 said that on the interpreter floor
   "only Q13+Q15 remain, both Tier B / execution-time … a convergence of two execution-time
   checks." **Amend:** the interpreter floor **does** have a produce-time axis — a Tier-A IR
   **well-formedness** verifier (Q19: 98 LOC / 634 B, all four Tier-A properties). So the
   floor's convergence is **produce-time well-formedness (Q19) + execution-time layout (Q13) +
   execution-time closure (Q15)** — a produce-time member **is** present. **But** its
   guarantee is **well-formedness, strictly weaker than the codegen route's Q4 equivalence**;
   the produce-time *equivalence* guarantee remains codegen-only.

2. **The S2 headline ("strongest verification ⟂ strongest portability").** **Keep it, but
   re-scope:** it is true for **behavioural-equivalence** verification (Q4), which is
   relational and structurally needs ≥2 independently-derived executors — unavailable on a
   one-executor interpreter floor. It is **false** as stated for produce-time structural
   verification **in general**: the interpreter floor carries a Tier-A well-formedness gate.
   Replace "产出时轴在解释器地板上缺席" with "产出时**等价**验证在解释器地板上结构性缺席；
   产出时**良构**验证（Q19, Tier-A）在两条路上都在。"

3. **§5.3 point 1 / §附 evidence table (Q4 row).** Where Q16 added "Tier A is codegen-only",
   **qualify:** *Tier-A **equivalence** is codegen-only; a Tier-A **well-formedness** gate
   (Q19) is available on the interpreter route too, but it verifies the IR is well-formed, not
   that its execution equals an independent one.*

4. **README question board.** Add **Q19 = decided**, conclusion = this verdict: an IR
   structural verifier gives the interpreter path a genuine Tier-A produce-time gate (98 LOC /
   634 B, all probes fire), but it verifies **well-formedness (unary)**, not Q4's
   **equivalence (relational)** — so it **corrects S2's over-statement** ("产出时轴缺席" is
   false) **without closing S2's actual hole** (produce-time equivalence is structurally
   unreachable on a one-executor floor). Coverage stops at well-formedness: memory-safety
   (needs eBPF-scale value-range or Q15 run-time), semantics, termination, and the L1–L5 OS
   seam all remain unverifiable structurally.

---

## Decision trace

1. ① gate built, negative-probe posture: 3 payloads PASS, 5 injected faults FIRE → **the gate is real and non-vacuous.**
2. ② Tier-A checklist: all four properties met → **it IS a Tier-A gate.** But guarantee shape is **unary well-formedness**, not Q4's **relational equivalence** → **different guarantee.**
3. Equivalence is structurally a pair-property; interpreter floor = one executor → **equivalence unreachable there** (结构推断, airtight: doubling one artifact catches nothing; a second lowering needs codegen, which ACG blocks and which leaves the floor).
4. → **Verdict: Q19 补相邻的良构洞；S2 的等价洞仍开，但 S2 "产出时轴缺席" 被证伪 → 改为 "产出时等价缺席，产出时良构在场".**
5. ③ boundary measured: memory-safety / semantics / OS-seam all pass the verifier → well-formed ≠ safe/correct.
6. ④ cost: 98 LOC / 634 B, small; the gap to eBPF's 20k lines is exactly the memory-safety capability it deliberately omits.
7. kill criterion (would have fired if the verifier could NOT distinguish good IR from any injected bad IR) — **not triggered**.

## Deviations from an idealised spec (honesty clause)

- **No pre-written spec doc** (the task pinned ①–⑤ before code; criteria were fixed before
  writing `verify.rs` and not changed after).
- **Un-forgettability is demonstrated via a wrapper** (`run_verified`), not by editing Q9's
  `interp::run` signature (that file belongs to Q9 and is reused verbatim). The
  `VerifiedModule` private-field construction gate is the real mechanism; wiring it into
  `interp::run` in production is a one-line signature change, noted not done.
- **The equivalence-unreachability claim is a structural argument (`结构推断`), not a
  measurement** — you cannot measure the absence of a second artifact. It is stated as
  reasoning and labelled as such.
- **B1 oob_store is not executed** (it is a real out-of-bounds write). That it *verifies
  without executing* is the criterion-③ point, so not executing it is correct, not a gap.
- **Did NOT slide into an eBPF-scale verifier / formal verification / a second ISA** (the
  named failure mode). The verifier is deliberately capped at structural well-formedness; B1
  documents exactly where it stops and why going further costs eBPF's 20k lines.
- **Did not dress well-formedness up as equivalence** (the honesty clause's central demand):
  the verdict explicitly reports that S2's equivalence hole stays open.

## Independent reference values

- Cross-calibration: Q9 eval-core recomputed under this toolchain = **1908 B** (matches Q9's
  reported 1908 B) → the 634 B verifier number is on the same ruler, not a drift.
- pure_compute run **through the verified gate** → **163** (Q1/Q9 independent value) → the
  verifier does not alter correct execution; it is a produce-time gate, not a semantics change.
