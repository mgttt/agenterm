# Q14 — Behavioural verification of naming bindings — RESULTS

Decisive experiment for [`plan/archive/design-naming-verification-experiment.md`](../../../plan/archive/design-naming-verification-experiment.md).
Challenges the last residue Q4/Q13 declared irreducible: the **naming binding truth**
(「符号 index 1 是不是真的 `CreateFileA`」). Built and run on real Windows/x86_64 using
only ③ `sym` + ④ `call`. Clean-room; reuses Q13's four-primitive contract as the template.

---

## Verdict — **命名可行为式验证（Tier B / 需执行），对有契约符号收敛到 N≈0 残留**

Q4 declared naming binding-truth **unqueryable** — *"the call's effect IS the thing whose
correctness you are trying to establish; no independent probe exists."* **The strong form
is refuted.** For a **contract-bearing** symbol an independent probe *does* exist: a known
input whose correct output is **external ground truth** the DLL does not get to define
(string length, arithmetic, bytes-you-wrote). On this box, all four tested bindings both
**PASS on the correct binding and FIRE on a deliberate mis-bind** (① below) — the naming
binding truth is **behaviourally checkable**.

But the correction is precise, and it does **not** hand the orchestrator an unconditional
win:

- It is **Tier B (behavioural, requires EXECUTION)**, not Tier A (structural, execution-free).
  Q4's build-time structural guard **still cannot** verify naming without running the calls.
  Naming moves from **trust (Tier C)** to **runtime-checkable (Tier B)**, not into the
  no-execution column.
- The circularity **converges** (④, the main criterion): a resolver that lies about a
  contract-bearing symbol is **caught by that symbol's own check** (every mis-bind fired),
  so the resolver is **not** a residual root for those symbols. The check chain bottoms out
  at a **residue class**, not the symbol table: **{no-contract symbols, the Thompson
  trigger, the ambient kernel}**. For a payload using only contract-bearing symbols, the
  count of residual **trusted named-bindings N ≈ 0** (modulo Thompson + kernel).

Against Q13 (⑤): Q13 verified layout **modulo naming**. Q14 verifies naming itself, so that
caveat **largely dissolves** — layout and naming collapse together and bottom out at the
**same** residue Q13 already named for its own no-round-trip fields. **The trust set shrinks
from {naming} to {no-contract symbols + Thompson + kernel}.**

**This is the third outcome, not "Q4/Q13 were right".** Naming is not irreducible; it is
**runtime-verifiable-and-convergent, with a named residue** — a real, substantive
improvement for the north star (agent-produced OS naming becomes machine-checkable for the
contract-bearing majority).

---

## Measurement conditions

| | |
|---|---|
| Host | Windows Server 2022 Datacenter 10.0.20348 (**real machine**), x86_64 |
| Compiler | `rustc 1.97.0 (2d8144b78 2026-07-07)`, `-O` |
| Primitives | `sym`/`call` mirroring `core/kernel.rs` contract (clean-room, std harness) — byte-identical in role to Q13's; ④ ceiling raised 5→7 for `CreateFileA` |
| Symbols under test | pure: `lstrlenA`, `MulDiv`, `lstrcmpiA`; OS round-trip cluster: `CreateFileA`+`WriteFile`+`ReadFile`+`CloseHandle`; residue: resolver / `Sleep` / `ExitProcess` |
| Detection proof | per symbol: correct binding must PASS **and** a deliberate mis-bind must FIRE, on this box |
| ISA / other OS | x86_64 only (naming axis ⟂ ISA, Q5); no PDB, no symbol server, no PE parse (spec §6 / time box) |
| Execution status | **真机执行** — every PASS/FIRE below is a live call into kernel32 on this box |

---

## ① Boolean gate — THE MAIN JUDGEMENT — **VERIFIED for all 4 bindings**

A **mis-bind** = resolving a *different* export into the slot — exactly what a lying resolver
or a wrong binding table does. Live driver output (`out/q14.exe`, real Windows):

```
[A1] lstrlenA  (pure; oracle = string length is a MATH fact, no other symbol)
    [PASS] correct  lstrlenA
    [FIRE] mis-bind GetTickCount->lstrlenA-slot: =9425497968 , expected 8 (binding WRONG)
[A2] MulDiv  (pure; oracle = 7*191/1 = 1337 is ARITHMETIC, no other symbol)
    [PASS] correct  MulDiv
    [FIRE] mis-bind GetTickCount->MulDiv-slot: (7,191,1)=9425497968 , expected 1337 (WRONG)
[A3] lstrcmpiA  (pure; fine-grained: case-INSENSITIVE equal -> 0)
    [PASS] correct  lstrcmpiA
    [FIRE] mis-bind lstrcmpA->lstrcmpiA-slot: ("AgentTerm","agentterm")=1 , expected 0 (WRONG)
[B1] CreateFileA round-trip cluster (write known bytes, read them back)
    [PASS] correct  CreateFileA
    [FIRE] mis-bind DeleteFileA->CreateFileA-slot: returned no writable handle (0) — WRONG
```

| symbol | class | oracle (external ground truth) | mis-bind → signal | strength |
|---|---|---|---|---|
| **A1** `lstrlenA` | pure | `len("agenterm")==8` (math) | GetTickCount returns a tick ≠ 8 | **strong** (known in → known out) |
| **A2** `MulDiv` | pure | `7*191/1==1337` (arithmetic) | GetTickCount ≠ 1337 | **strong** |
| **A3** `lstrcmpiA` | pure | case-insensitive equal ⇒ 0 | `lstrcmpA` (case-sensitive) ⇒ 1 | **strong**, fine-grained (catches a near-miss sibling) |
| **B1** `CreateFileA` | round-trip cluster | bytes written come back | `DeleteFileA` yields no writable handle → read-back fails | **strong jointly**, modulo cluster partners |

**Boolean gate (spec §4): PASSES for all four** — each binding passed on the correct symbol
and fired on the mis-bind. Kill criterion ("mis-bind fires for NO symbol") **not** triggered.
The A3 probe is worth singling out: mis-binding *case-insensitive* compare to its
*case-sensitive* sibling — a one-character-behaviour difference — is caught. The check is not
"is it roughly a compare function", it is "does it honour `lstrcmpiA`'s exact contract".

---

## ② Coverage classification — verifiable iff a constructible KNOWN-in → KNOWN-out contract

| class | example | why (un)verifiable | residual naming trust |
|---|---|---|---|
| **pure, external oracle** | `lstrlenA`, `MulDiv`, `lstrcmpiA` | output is a **math fact** external to the DLL; **zero** other-symbol trust | **none** (modulo Thompson) |
| **OS round-trip cluster** | `CreateFileA`/`WriteFile`/`ReadFile` | OS's own write-then-read-back is the oracle; verified **jointly, modulo partners** (each itself verifiable) | none for the cluster; a behaviour-preserving *internal permutation* is undetected (shrinks with more distinct tests) |
| **the resolver** | `GetProcAddress`/`LoadLibraryA` | used to obtain **every** pointer incl. the one you'd test it against — no probe not routed through it | **irreducible root** — but see ④: its lies about *contract-bearing* symbols are caught downstream |
| **no observable effect** | `OutputDebugStringA`, `ExitProcess` | effect invisible / unrecoverable → **no known-out** to compare | **irreducible residue** |
| **weak / indirect-only** | `Sleep` | only observable via **another** symbol (`GetTickCount`), non-deterministically | verifiable only *modulo that symbol*, and flakily |

Live evidence for the weak tier: `Sleep(1)` measured `elapsed ~0 ms` via `GetTickCount`
(GetTickCount's ~15.6 ms resolution swallows a 1 ms sleep) — the "oracle" is another symbol's
non-deterministic reading. Reported as-is: this is the boundary, not an implementation gap.

**The boundary, plainly:** a naming binding is behaviourally verifiable **iff the symbol has a
constructible known-in→known-out contract or an OS round-trip**. That is — empirically — the
*majority* of symbols a payload actually calls (they exist to transform inputs or produce
observable OS effects). The residue is: symbols with no observable contract, the resolver root,
and the Thompson trigger. This mirrors Q13's layout boundary (round-trippable fields
detectable; no-round-trip fields residual) **almost exactly** — the two are the same shape.

---

## ③ Cost — pure checks 8 LOC/symbol, cluster 40 LOC, +0 kernel bytes

Detection-layer LOC (non-comment/non-blank), measured from `main.rs`:

| self-check | LOC | note |
|---|--:|---|
| `strlen_selfcheck` (A1) | 8 | resolve + call + compare; most is the FIRE message (core ~4) |
| `muldiv_selfcheck` (A2) | 8 | " |
| `strcmp_i_selfcheck` (A3) | 8 | " |
| `roundtrip_selfcheck` (B1) | 40 | create+write+close+reopen+read+cmp + cleanup |
| **marginal per symbol** | **8 (pure) / 40 (cluster)** | payload-side only |

**Kernel-in vs kernel-out split:** **kernel bytes added by verification = 0.** The four
primitives are unchanged; every check is ordinary ③+④ payload code. Detection is **pure
kernel-out**, identical posture to Q13. Against Q13's **~18–38 LOC/fact +0 kernel bytes** for
layout: same envelope; **pure-function naming checks are cheaper** (no constructive file setup),
round-trip naming checks land at the top of Q13's range because they *are* the same round-trip.

---

## ④ Circularity / root trust set — THE MAIN CRITERION — **CONVERGES to N ≈ 0 residual bindings**

Dependency of each check (the symbols it must itself call), from the driver:

```
A1 lstrlenA  -> {resolver}                                   (pure: NO other symbol)
A2 MulDiv    -> {resolver}                                   (pure: NO other symbol)
A3 lstrcmpiA -> {resolver}                                   (pure: NO other symbol)
B1 create    -> {WriteFile, ReadFile, CloseHandle, resolver} (each itself Class-A/B verifiable)
```

**Is this the vicious circle Q4 named? No — it converges, and further than "a small root
set of symbols".** Three findings, in order of force:

1. **A resolver lie about a contract-bearing symbol is CAUGHT by that symbol's own check.**
   The negative probes *are* the proof: mis-binding a slot = simulating a resolver that hands
   back the wrong pointer for that name, and the behavioural check **fired every time**. So the
   resolver's honesty *about a contract-bearing symbol* is verified **together with** the
   binding — the resolver is **not** a residual root for those symbols. (This is stronger than
   my going-in hypothesis, which expected the resolver to survive as an N≈1 root.)

2. **Pure functions verify with dependency depth 1** — `{resolver}` only, and by (1) even that
   is not trusted, because a wrong pointer produces a wrong number. Their oracle is *external*
   (math). So a large class of bindings is verifiable against **no** in-process trust at all,
   modulo Thompson.

3. **The chain therefore bottoms out at a RESIDUE CLASS, not the symbol table:**
   - **(1) the Thompson trigger** — a symbol/resolver honest during the check, malicious later.
     **Irreducible for all behavioural testing** (this is where Q4's Thompson genuinely survives).
   - **(2) no-contract symbols** (② residue) — no probe, so the resolver *can* lie about these
     undetectably. This is the only place a residual *named* trust survives, and it equals ②'s
     unverifiable set.
   - **(3) the ambient OS kernel** — outside the process, = Q15's beyond-the-intent-boundary
     trust; not an agent product, not in scope to verify here.

**N (residual trusted named-bindings) for a payload using only contract-bearing symbols ≈ 0**,
modulo (1) + (3). **"命名不可约" is therefore wrong; the correct statement is "命名可行为式验证，
收敛到 {Thompson trigger, 无契约符号, 内核} 这个残留类"** — and Thompson + kernel are residues
that *every* verification story on this platform already carries. The architecture implication
the orchestrator flagged **does** hold: this is "only trust the residue", not "naming is a hole".

---

## ⑤ Relation to Q4 / Q13 — the {naming} hole collapses into the shared residue

Q4/Q7-L1: *"is symbol index 1 really `CreateFileA`?"* declared **unqueryable** (circular:
the only way to test the call is to make it). Q13 verified **layout** *modulo naming*, shrinking
the trust set **{naming + layout} → {naming}** and leaving **{naming}** as the last irreducible
hole (SYNTHESIS R3).

Q14 shows **{naming} itself is not irreducible.** The asymmetry Q4 drew ("layout's consequence
can be cross-checked without trusting layout; naming's consequence *is* the thing you're
verifying, no independent probe") is **real but smaller than stated**:

- Layout is checkable **modulo naming** (Q13).
- Naming is checkable **modulo the Thompson trigger + no-contract residue + the kernel** (Q14)
  — for any contract-bearing symbol, via an oracle *external* to the symbol.

So Q13's "modulo naming" caveat **dissolves** for contract-bearing symbols, and the two holes
**collapse into one residue**: not "naming = trust", but the same *no-round-trip / no-contract*
residue Q13 already documented for layout, **plus** the universal Thompson trigger. The trust
set shrinks **{naming} → {no-contract symbols, Thompson trigger, ambient kernel}**.

**Caveat kept front and center (honesty clause):** this verification is **Tier B — it requires
executing the symbol**. Q4's structural, execution-free guard still cannot reach naming; and
the executed checks have side effects (files created, sockets bound). Naming is *runtime-*
verifiable-and-convergent, not *structurally* verifiable. Q4/Q13 were not wrong that naming is
**unqueryable structurally**; they overreached in reading "unqueryable" as "irreducible trust".

---

## Decision trace (spec §4 tree, walked)

1. **① (main, boolean gate):** all four bindings PASS-on-correct **and** FIRE-on-mis-bind on
   the real box → behavioural verification exists for every tested symbol → kill criterion
   (① fires for no symbol) **not** tripped. → continue.
2. **④ (main, convergence):** the check chain converges — a resolver lie about a contract-
   bearing symbol is caught downstream; pure functions need no in-process trust; residue =
   {Thompson, no-contract, kernel}; N ≈ 0 residual named-bindings for contract-bearing payloads.
   → **falls to §4 branch 3: naming is behaviourally verifiable and convergent.**
3. **② :** verifiable iff constructible known-in→known-out contract or OS round-trip; residue =
   no-contract symbols + resolver-root (itself reduced by ④) + weak indirect-only.
4. **③ :** 8 LOC/pure symbol, 40 LOC/cluster, **+0 kernel bytes** (payload-side), Q13 envelope.
5. **⑤ :** {naming} → {no-contract + Thompson + kernel}; Q13's "modulo naming" dissolves; but
   Tier B (needs execution), so structural (Tier A) naming verification remains impossible.

**Verdict: 命名可行为式验证 / 收敛到 N≈0 残留 (modulo Thompson+kernel) / NOT irreducible** —
the orchestrator's challenge to R3 is **upheld**, with the precise limits that it is runtime
(Tier B) not structural, and that the Thompson trigger is a genuine surviving residue.

---

## Reproduce (third-party runnable)

```powershell
cd research/dynamic-core/naming
mkdir out 2>$null
rustc --edition 2021 -O -A nonstandard_style -A dead_code -A unused_mut main.rs -o out/q14.exe
cd out ; ./q14.exe
```

The driver, on real Windows: runs each of the four self-checks with the correct binding (must
print `[PASS]`) and with a deliberate mis-bind (must print `[FIRE]`), prints the ① per-symbol
VERIFIED verdict, the ④ dependency graph + convergence reading, the ② coverage classification
(with a live `Sleep`/`GetTickCount` weak-tier demonstration), the ③ cost, and the ⑤ Q4/Q13
relation. Deterministic across runs except the mis-bind's `GetTickCount` value and the `Sleep`
elapsed reading.

---

## Independent reference values (proof it is not self-certifying)

- `lstrlenA("agenterm")` → **8**: `len("agenterm")` is 8 by inspection, external to kernel32.
- `MulDiv(7,191,1)` → **1337**: `7*191 = 1337`, `/1 = 1337`, external arithmetic.
- `lstrcmpiA("AgentTerm","agentterm")` → **0** (equal) vs `lstrcmpA` → **1** (differ): the
  case-insensitivity is `lstrcmpiA`'s documented contract; the sibling's divergence is the check.
- `CreateFileA` round-trip returns the bytes `"agenterm-q14-naming"` written — same value in and
  out, independent of any single symbol's self-report.

---

## Deviations from the spec / honesty clause

1. **The result challenges two existing RESULTS and the challenge is UPHELD — reported as such.**
   The orchestrator hypothesised the naming verdict was too pessimistic; the experiment confirms
   it. Per the honesty clause I checked hard for the opposite: the kill criterion (no mis-bind
   fires) did **not** trip, and I could not construct a contract-bearing symbol whose mis-bind
   went undetected. Where Q4/Q13 **do** survive is stated plainly (Tier B not Tier A; Thompson
   trigger; no-contract residue) rather than buried.
2. **Convergence was STRONGER than my going-in hypothesis.** I expected the resolver to survive
   as an N≈1 root. The negative probes showed it does not, for contract-bearing symbols — its
   lies about them are caught. I did not soften this to match the prediction; N ≈ 0 is reported.
3. **Class-B verifies the cluster JOINTLY, not each symbol individually.** A behaviour-preserving
   internal permutation of `{CreateFileA, WriteFile, ReadFile}` would pass. Stated as a coverage
   limit (②), not engineered away; it shrinks with additional distinct round-trip tests.
4. **No slide into symbol-integrity infrastructure (spec §1.3 pathology).** No PDB, no symbol
   server, no PE export parsing, no signature/hash whitelist, no full Win32 coverage. The four
   primitives are unchanged from Q13; verification is added purely as payload-side ③+④ usage.
   The `Sleep` weak-tier probe is a *classification* demonstration, not a verification attempt.
5. **④ ceiling raised 5→7** in the local `call` shim (identical to Q13's 7-arg ceiling) so
   `CreateFileA` (7 args) fits. No kernel-contract change; the ceiling matches Q6/Q13.
6. **x86_64 / Windows only, four primitives only** (spec §6 / time box). Naming axis is
   ISA-orthogonal (Q5), so a second ISA was not built.
