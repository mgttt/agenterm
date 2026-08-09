# Q23 — Runtime-declared native intents — RESULTS

Targets `plan/design-dynacore-native-core.md` §8. Reads against the F1 finding
(`research/dynamic-core/assembled/RESULTS.md` §② F1), the bake-and-detect
discipline (Q13, `declare/RESULTS.md`), and behavioural naming verification
(Q14, `naming/RESULTS.md` / SYNTHESIS R3). Clean-room per
`prd/PRD_02_14_research_provenance.md`: borrows the *shape* of
`crates/agenterm-nativecore/src/verify.rs` (`IntentArityMismatch`) and
`seam.rs` (`Arg::Sem(k)`, the uniform `REACH: &[fn(&[i64])->i64]` trampoline)
by having read them; imports nothing from that crate.

---

## Verdict — **decisive split, not a single yes/no.**

**The verifiable half of F1 moves to load time cleanly and must be *derived, not
declared*; the half people assume `contract_arity()` covers was never a machine
check even in the compiled path — it is a second-party signature assertion, and
moving to load time forces that assertion to come from behavioural probing
(contract-bearing symbols, Q14 residue) or an independent curated registry
(still human-gated). A pure data-from-one-author pack cannot reach F1-strength on
the ABI half — not because runtime is special, but because single-authorship
destroys the independence F1's fix relied on.**

Mapping to §8's three candidate directions: they are **not three alternatives —
they decompose the question.** Direction 1 handles the internal-arity /
memory-safety half (with "derive, don't declare" as the real finding); direction
2 recovers the ABI/naming half at run time for contract-bearing symbols;
direction 3 is correct **only** for the narrow no-contract residue and for a
sharper reason than "runtime extension is unsafe."

---

## §0 Background & what is NOT in question here

Settled, not re-litigated (see the cited RESULTS):
- Interpretation is the floor; ③ reach + ④ call reach any existing native code
  over the integer/pointer word subset (Q6/Q9/Q12).
- The seam content (naming, layout, sentinels) is not neutralisable — it is
  Q1's L1–L5 and stays trust past the intent boundary (Q15 R5).
- Naming binding truth is structurally unqueryable at produce time but
  behaviourally (Tier B, run-time) verifiable for contract-bearing symbols,
  converging to residue {no-contract symbols, Thompson trigger, kernel} (Q14).
- `contract_arity()` in `ir.rs` is a **compile-time match arm**; `verify.rs`
  cross-checks every `ExternDecl.nargs` against it, which is what makes the F1
  check real rather than IR-internal-only.

Q23's one question: can that contract come from the **pack at load time**
instead of the match arm, keeping the F1-class strength `verify.rs` has today?

## §1 Hard constraints (violate → the experiment does not answer Q23)

1. **No codegen / no executable-memory generation.** The extension mechanism
   must be ③ reach + ④ call only — same posture as the whole nativecore design.
   (Met: `call_n` is a transmute-dispatch over resolved symbols; zero bytes
   emitted.)
2. **The new symbol must be one the interpreter did not ship knowing about.**
   No compile-time `match symbol` anywhere in the mechanism. (Met: `grep -nE
   'match .*(symbol|MulDiv|lstrlen)' main.rs` → none; the three symbols appear
   only as *strings* in the runtime-parsed pack text.)
2b. **病灶探测器 (disease detector).** Any urge to make the answer "yes" by
   letting the interpreter carry a hidden trusted table of the new symbol's true
   signature is the compile-time knowledge Q23 is trying to remove — if it turns
   out to be necessary, that is a **finding**, not a thing to smuggle in. (It
   turned out necessary for exactly one half — recorded as the verdict, §S5.)
3. Real Win32 exports, real calls, on the real box — measured, not argued.

## §2 Minimal experiment

| dimension | choice | why |
|---|---|---|
| new APIs | `MulDiv`, `lstrlenA`, `GetTickCount` (kernel32) | all **contract-bearing** (external oracle exists), none among nativecore's seven intents — Q14 used the first two for the same reason |
| pack form | line-based text parsed at runtime | makes "declared at load time" literal — the symbol is a string, not compiled dispatch |
| contract source | measured **both ways**: a separate `declared_arity` field vs. one **derived** from the recipe | this is the axis the finding lives on |
| the lie | constructed packs that lie about arity (internal, S3/S4) and about the real ABI (S5) and about naming (S6) | the F1 reproduction pattern: does the mechanism catch a lie *before* execution? |

## §3 Judging criteria (fixed before the run)

| # | criterion | type |
|---|---|---|
| C1 | A never-shipped API executes correctly from pure load-time data | boolean gate (extensibility) |
| C2 | An IR that under-provides args to a recipe is **caught before execution** | boolean gate (F1 reproduction) |
| C3 | The catch survives without re-introducing F1 (i.e. the contract cannot be a second author statement that silently disagrees with what the seam reads) | boolean + evidence |
| C4 | Whether an *internally-consistent-but-ABI-wrong* pack is machine-catchable at load time from data alone | boolean (this is the honest kill line) |

## §4 Decision tree / kill criterion / time-box

```
1. C1 fails (cannot call a new API from data at all) -> Q23 is moot, stop. 
2. C1 passes, C2 fails (derived-verify does not catch under-provision)
      -> direction 1 does not even reach F1-strength; report negative.
3. C1+C2 pass -> examine C3 (declare vs derive) and C4 (ABI lie).
      C4 catchable from data alone  -> "yes, runtime extension is F1-safe."
      C4 NOT catchable from data    -> decompose: which half moves, which needs
                                       an independent party (direction 2 or 3).
kill: if C2 passes ONLY under the declared field (not derived) -> F1 is not
      actually closed; that is a negative on direction-1-naive.
time-box: stop when C1..C4 each have a real-machine data point. No second ISA,
      no full IR, no libffi generality beyond arity 0..4 integer/pointer.
```

## §5 Non-goals (explicit)

- Not building a production pack loader, signing, or a wire format.
- Not extending ④ beyond the integer/pointer word subset (Q20's float `Kind`
  tag would ride the same axis; not needed to answer Q23).
- Not touching `crates/agenterm-nativecore/**` or any product `src/` file.
- Not re-measuring SYNTHESIS numbers; not editing the design doc (routed back
  to the orchestrating session).
- Not resolving Q14's Thompson-trigger residue — inherited, not re-opened.

---

## Measurement conditions

| | |
|---|---|
| Machine | Windows Server 2022 Datacenter 10.0.20348 (**real box**) |
| ISA / target | x86_64 / `x86_64-pc-windows-msvc` |
| Compiler | `rustc 1.97.0 (2d8144b78 2026-07-07)`, `-O`, edition 2021, no Cargo |
| Execution | **[实测·真机执行]** — every S1–S6 number below came out of a real run; MulDiv/lstrlenA/lstrcmpiA/GetTickCount are real kernel32 exports resolved via `LoadLibraryA`/`GetProcAddress` and invoked through a real `extern "system"` trampoline |
| Prototype | `main.rs`, 254 non-blank-non-comment LOC, single file |

Reproduce:
```powershell
cd research/dynamic-core/runtime-intent
rustc --edition 2021 -O main.rs -o out/rti.exe
./out/rti.exe
```

---

## The run (pasted verbatim)

```
== Q23 runtime-declared native intents — real box ==

pack parsed at runtime: 3 intents, symbols = ["MulDiv", "lstrlenA", "GetTickCount"]
(the interpreter source contains NONE of these symbol names as code)

[S1 extensibility] MulDiv(7,11,5): verify_derived=Pass  native_call=15  oracle=15  PASS — a never-shipped API ran from load-time data
[S2 extensibility] lstrlenA("HELLO"): verify_derived=Pass  native_call=5  oracle=5  PASS

[S3 F1/Lie-A, derived] MulDiv recipe needs 3, IR passes 2: verify_derived=IntentArityMismatch { provided: 2, contract: 3 }  CAUGHT before execution — F1 reproduced and closed at LOAD TIME
[S4 derive-vs-declare] recipe derefs Sem(2) but author declares arity 2, IR passes 2:
        verify_DECLARED = Pass  -> PASS = F1 REOPENED (seam would read Sem(2) OOB at runtime)
        verify_DERIVED  = IntentArityMismatch { provided: 2, contract: 3 }  -> CAUGHT (contract derived from recipe = 3 != 2)

[S5 Lie-B, ABI under-declaration] pack declares MulDiv as 1-arg, IR passes 1:
        verify_derived = Pass  -> PASS (recipe is internally consistent — NO machine check fires)
        native MulDiv(7,<garbage RDX>,<garbage R8>) returned 1 (a true 3-arg call would be 15); result is UB from an under-declared ABI —
        NOTHING in the single-author pack lets verify() know MulDiv's TRUE arity is 3.

[S6 direction-2/Q14 naming probe] intent 'len' binds lstrcmpiA but claims strlen:
        arity check: PASS (1==1) — arity cannot see a naming lie
        Tier-B probe strlen("HELLO")==5 : got -1 -> FIRE (behavioural probe caught the mis-bind — Q14, contract-bearing)
        residue: a NO-contract symbol (e.g. OutputDebugStringA) has no oracle -> unprobeable (Q14 residue).
```

---

## Reading the results against §3

### C1 — extensibility PASS (S1, S2). [实测·真机执行]
`MulDiv` and `lstrlenA` — neither shipped in nativecore's seven intents — ran
correctly (15, 5) driven entirely by a text pack parsed at runtime, over a
generic ④ trampoline, with **zero recompilation and zero codegen**. An agent
*can* teach the interpreter a Win32 API it did not ship knowing about. The
extensibility half of Q23 is a clean yes.

### C2 + C3 — the F1 half moves to load time, IFF derived (S3, S4). [实测·真机执行]
S3 is the F1 pattern reproduced one level up: an IR that under-provides args to
a recipe that dereferences `Sem(2)`. `verify_derived` **caught it before
execution** (`IntentArityMismatch { provided: 2, contract: 3 }`) — the exact
class Q22's F1 demo let panic inside the interpreter, now a produce-time reject
at load time.

S4 is the **actual finding of direction 1**: a separately-**declared** arity
field re-opens F1 verbatim. When the pack author declares arity 2 for a recipe
that reads `Sem(2)`, `verify_declared` **passes** it (2 == 2) and the seam would
read `Sem(2)` out of a 2-element slice at run time — F1 reborn, because the
author now controls *both* the IR and the declaration (single-author
circularity, exactly what F1's compile-time `contract_arity()` broke by being a
second party). `verify_derived` still catches it, because the contract is
computed from the same recipe the seam dereferences — the two cannot disagree by
construction. **So direction 1 works, but only if the contract is derived from
the executable recipe, never trusted as a parallel author declaration.** This is
strictly *stronger* than the compiled path: `contract_arity()` and the seam's
`Sem(k)` accesses are two hand-written constants a human keeps in sync (guarded
by a test); derivation removes the second constant and the possibility of drift.

### C4 — the ABI half does NOT move (S5). [实测·真机执行]
S5 is the kill line. A pack declares the real `MulDiv` as a **1-arg** function
(recipe `[Sem(0)]`), and the IR honestly passes 1 arg. The recipe is internally
consistent, so `verify_derived` **passes** — and then the trampoline calls the
real 3-arg `MulDiv` with one arg, reading garbage from RDX/R8, returning `1`
where a true call would return `15`. **Nothing in the single-author pack lets
`verify()` know `MulDiv`'s true arity is 3.** On Windows x64 a native export
carries no queryable arity/type metadata — the calling convention is uniform and
reflection does not exist — so the true ABI signature is *not machine-derivable
from the pack.* This is not a new hole opened by going to load time; it is
Q1's L2 (semantic arity ≠ native arity) meeting Q14's naming residue. The
compiled path did not verify this either: `contract_arity(FileWrite) == 3` is a
human who read the WriteFile contract and wrote `3`. **What recompilation
actually buys is not an F1-class machine check — it is an independent
second-party assertion of the true native signature, routed through git +
human review.**

### Direction 2 recovers the contract-bearing majority (S6). [实测·真机执行]
S6 shows the run-time escape hatch: a Q14 behavioural probe against an external
oracle (`strlen("HELLO") == 5`) **fires** on a mis-bound symbol (`lstrcmpiA`
masquerading as strlen) that every arity check waves through — got `-1`, FIRE.
For a symbol with a constructible known-in→known-out contract, direction 2
verifies the binding at run time (Tier B), inheriting Q14's exact residue: a
no-contract symbol (`OutputDebugStringA`) has no oracle and stays trust.

**One honest sharpening of direction 2 (S5 ∩ S6):** behavioural probing's
strength is bounded by the *declared* signature. An **under-declared** arity
(S5) yields a probe too weak to reproduce the oracle deterministically — you
cannot form a 3-input→known-output probe for an intent the pack declares as
1-arg. So direction 2 catches a **wrong binding of a fully-declared contract**;
it does **not**, by itself, catch an **under-declaration** of a real symbol's
arity. Closing S5 requires an independent statement of the true arity — the
role `contract_arity()` plays, and the thing a single-author data pack lacks.

---

## Decision-tree trace (per §4)

C1 passes (S1/S2) → C2 passes **but only under derived, not declared** (S3 vs
S4) → the §4 kill note fires on direction-1-*naive* (declared) and clears on
direction-1-*derived* → C4 is **not** catchable from data alone (S5) → decompose:
the internal-arity/memory-safety half moves to load time (derived); the ABI/
signature half needs an independent party — behavioural probe for
contract-bearing symbols (direction 2, S6), otherwise a human-reviewed assertion
(direction 3, narrow residue). **Terminal verdict: decisive split, stated at the
top.**

## Numbers

| criterion | result | tag |
|---|---|---|
| C1 new-API-from-data (MulDiv 15, lstrlenA 5) | PASS | [实测·真机执行] |
| C2 under-provision caught pre-exec (derived) | PASS (`contract:3 != provided:2`) | [实测·真机执行] |
| C3 declared re-opens F1 / derived closes it | declared=Pass(hole), derived=Caught | [实测·真机执行] |
| C4 ABI under-declaration catchable from data | **NO** (MulDiv returns 1 not 15, verify passes) | [实测·真机执行] |
| direction-2 naming probe on contract-bearing symbol | FIRE (got -1 ≠ 5) | [实测·真机执行] |
| prototype size | 254 LOC, one file, no codegen, no exec-memory | [实测·结构推断] |

---

## Deviations / honesty clause

1. **No度量 was changed to make the verdict look cleaner.** The answer is a
   split, and both halves are reported with their own evidence — the ABI half is
   an honest negative (S5), not softened.
2. **S5 relies on UB deliberately.** Transmuting to a shorter signature and
   reading uninitialised arg registers is undefined behaviour; that *is* the
   demonstration (an under-declared ABI is a memory-safety hole at the FFI
   boundary), not a mechanism the prototype depends on for correctness. The
   returned `1` is one observation of that UB on this box; the point is that
   `verify()` passed and the result is wrong/undefined — not the specific value.
2b. **S6's probe is Tier B (needs execution)**, exactly Q14's posture — it is
   not a produce-time check and does not upgrade the produce-time axis.
3. **The prototype models an IR call site as a single instruction**, not the
   full block/verify machinery of `verify.rs` — the F1 property under test
   (arg-count vs contract) does not need the rest, and adding it would only
   restate Q19/Q22. Recorded so the 254 LOC is not read as "a full interpreter."
4. **Only arity 0..4, integer/pointer only.** Q20's float `Kind` tag and larger
   arities ride the same axis; not needed to settle Q23 and deliberately out of
   scope (§5). The verdict is about the *contract-declaration* axis, not the ④
   shape axis Q20 already mapped.
5. **`native_arity` (recipe length) is kept but unused in dispatch** — retained
   only to document that semantic arity (the contract) and native arity (L2) are
   distinct facts even when they coincide for these three APIs.

## What would change the verdict

- If a future Windows exposed queryable per-export ABI metadata (it does not
  today), C4 would become machine-catchable and the ABI half would move to load
  time too — collapsing direction 3's residue. **[结构推断]** — no such channel
  exists on this box (same shape as Q13's "no `offsetof` oracle" finding).
- A **curated, interpreter-shipped signature registry** (an allowlist of
  extendable symbols with their true arities, human-reviewed once) would restore
  the second-party assertion without a per-intent recompile — this is direction
  3 in a cheaper form (sign the registry, not the binary), still human-gated at
  the point a *new* symbol is admitted. Not built here; named as the concrete
  middle path the evidence points to.

---

*Research-track experiment. Does not change PRD capability status; does not edit
the design doc (verdict routed back to the orchestrating session).*
*Built from public technical knowledge only; no nativecore source imported.*
