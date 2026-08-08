# Q21 — Can control-flow orchestration (R1/L3b) be tabified? — RESULTS

Targets R1 in `research/dynamic-core/SYNTHESIS.md` §③, grounded in Q7's `spawn_boundary()`
(`research/dynamic-core/tables/RESULTS.md` ⑤). Reuses Q1's problem statement
(`research/dynamic-core/ir/RESULTS.md` L3) and is read against Q17's posture on this seam
(`research/dynamic-core/recursive/RESULTS.md`: recursion moves seams, doesn't shrink them).

---

## Verdict — **R1 has a real, executed, tabifiable sub-boundary — and a real, precisely-named
hard wall next to it.** Q7's "5 irreducibly code" facts are not one thing; they split 2 / 3.

---

## ① Precise restatement — what Q7's "irreducibly code" actually names

Q7's `spawn_boundary()` (`tables/marshal.rs` ⑤) classifies 8 facts of `SpawnWait`. The 5
marked `code`, quoted verbatim:

| # | Q7's fact | Q7's own reason (verbatim) |
|---|---|---|
| 4 | extract hProcess from PROCESS_INFORMATION output | "a runtime pointer read whose result feeds the NEXT call — **dataflow between calls**" |
| 5 | sequence CreateProcess→Wait→GetExitCode→Close | "multi-call **ORCHESTRATION** with data dependencies" |
| 6 | SysV fork() then BRANCH on pid | "two divergent **control-flow paths**... the hardest wall" |
| 7 | SysV argv[]/envp[] pointer-array construction | "pointer arithmetic, not constant fields" (runtime-**variable length**) |
| 8 | error/sentinel handling (L5) | "recording the sentinel value is data; **ACTING on it needs a branch**" |

Reading these five side by side, they are **not one failure mode**. Two (#4, #5) never
involve a runtime value selecting *which* operations execute — every SpawnWait call runs
exactly once, in a fixed order, and the only thing that varies step-to-step is *which value*
flows into the next call's argument (a pointer read out of a struct). The other three (#6,
#7, #8) all involve a runtime value determining *whether/how many times* something runs:
fork's pid decides which of two disjoint code paths executes; argv's length is
runtime-determined; acting on a sentinel means skipping steps conditionally.

**Precise boundary, stated as a rule, not a vibe:**

> A multi-call sequence tabifies as inert DATA **iff every step always executes, in a fixed
> author-time order, with arguments drawn from constants or earlier steps' results**. It
> stops being DATA the instant any step's *execution* (not just its *arguments*) is
> contingent on a runtime value the table's author could not know in advance.

So Q7's "irreducibly code" verdict, read carefully, was never really about "orchestration"
as a monolith — Q7 lumped #4/#5 in with #6/#7/#8 because its marshaller is a **single-call**
engine by design (spec forbids multi-call inside one lowering) and so **never attempted**
the linear subset; `SpawnWait` was simply absent from both its tables (Q7 ⑤ deviation 1: "no
spawn bytes are emitted"). Q21 attempts exactly that missing case.

---

## ② Can the fork-style branch be tabified without inventing a new language? — Executed test

### The tabifiable half: executed, on real `kernel32`

`research/dynamic-core/orchestration/step_table.rs` defines a schema (`ArgSrc`, `Step`,
`StepTable`) and **one fixed control loop**, `run()`, that walks `table.steps` with
`for step in table.steps` — Rust's own linear iteration is the only "program counter", and
it is not itself table-driven or data (it cannot be pointed anywhere else; there is no field
in `Step` that names a "next step").

`research/dynamic-core/orchestration/main.rs` supplies, as **DATA**:
- `STARTUPINFOA_BYTES` / `PROCESS_INFORMATION_BYTES` — the struct *content*, a constant blob
  (Q7's L3a: once the layout facts are known via query, the content is data — reused here).
- A 4-step `StepTable`: `CreateProcessA → WaitForSingleObject → GetExitCodeProcess →
  CloseHandle`. Q7 item 4 ("extract hProcess") is **not a separate step** — it is folded
  into `ArgSrc::SlotPtrOff(pi_slot, 0, 8)`, an *argument source* for steps 2 and 3. That
  fold is the concrete demonstration that #4 is dataflow, not control flow: it never needed
  a branch, only a field read.

**Executed result** (Windows/x86_64 real machine, `out/driver.exe`):

```
CreateProcessA success = true
WaitForSingleObject raw = 0x0
exit code (read purely from the step table's DATA-driven field-read) = 7
PASS: linear 4-step SpawnWait orchestration executed entirely from DATA, zero per-op branch in the control loop.
```

`cmd.exe /c exit 7` was spawned, waited on, and its exit code (7) extracted — end to end
through the step table, with **zero per-op or per-step branches** in the engine. Discipline
check (third-party reproducible):

```
grep -nE 'match|if ' step_table.rs
```
→ the only `match` sites are `match a { ArgSrc::... }` (a bounded 4-variant data-shape
dispatch, the same class Q1/Q7 already treat as legitimate "fixed engine", not per-op code)
and `match width { 1|4|8 => ... }` (bounded width dispatch). **No `if`, no `match` on step
identity or `reach_id`, no jump.** `step_table.rs` is also host-agnostic by construction —
`grep -c 'CreateProcess\|kernel32\|Win' step_table.rs` → 3 hits, all three in doc comments,
zero in code.

**This is Q21's positive result: Q7 items #4 and #5 — "extract hProcess" and "sequence
Create→Wait→GetExitCode→Close" — DO tabify, executed, without inventing any new language.**
The reason they tabify is exactly the rule in ①: no runtime value ever selects *whether* a
step runs, only *what value* feeds its arguments.

### The non-tabifiable half: executed failure-path demonstration

A second table (`spawn_table_failure_demo`, same shape, only the DATA differs: the command
line points at a nonexistent executable) makes the missing capability concrete rather than
asserted:

```
CreateProcessA success = false (GetLastError = 6, expect 2 = ERROR_FILE_NOT_FOUND)
WaitForSingleObject raw = 0xffffffff (expect 0xffffffff = WAIT_FAILED, NULL handle)
exit code the table reports anyway = 0
FINDING: CreateProcessA failed (no process ever ran), yet the linear step table
has NO FIELD that can express "skip Wait/GetExitCode when step 0 failed" — it
reports exit code 0 as if a process had run and exited cleanly.
```

(The `GetLastError` value reported is 6/`ERROR_INVALID_HANDLE`, not the 2/`ERROR_FILE_NOT_FOUND`
that `CreateProcessA` itself set — because the table's blind continuation into
`WaitForSingleObject(NULL, ...)` **overwrote** the original error before the driver ever
read it. This is a second, independent illustration of the same failure mode: continuing a
fixed sequence past a point that should have branched doesn't just produce a wrong *answer*,
it corrupts diagnostic *state* too.)

**Was a conditional-jump field invented and smuggled in as "data"? No — checked explicitly.**
`Step` has no `next_on_fail`, `cond`, or any field naming another step. The failure-demo
table is *structurally identical* to the success table; only the *byte content* of one
`Rodata` argument differs. That is the honest point: **the schema, as designed, cannot
express "conditionally skip" at all** — not "expresses it clumsily", **cannot**, by
construction. Making it able to would require adding exactly the field just named, and that
field is analyzed, not built, in ③.

---

## ③ Why the branch/loop half is not tabifiable — the exact missing capability

Precise diagnosis, not "too complex":

**What would be needed.** To make step 1/2 conditional on step 0's outcome, `Step` would
need a field like `next_on_fail: Option<usize>` (index of the step to jump to), and `run()`
would have to change from:

```rust
for step in table.steps { execute(step); }
```
to
```rust
let mut pc = 0;
while pc < table.steps.len() {
    let step = &table.steps[pc];
    let ret = execute(step);
    pc = if fails(ret) { step.next_on_fail.unwrap_or(pc + 1) } else { pc + 1 };
}
```

**What that change actually is.** The loop has gained two things it did not have before: a
**mutable program counter** (`pc`, no longer just Rust's own linear iterator) and a
**conditional transfer of control keyed on a runtime value** (`fails(ret)`, unknown at
table-authoring time). A `Step` array with those two properties is — structurally, not
metaphorically — a **finite-state program**: state = `pc`, transition = data-dependent. This
is definitionally what a bytecode instruction stream is. The `next_on_fail` field is a goto.

**The exact capability missing from the DATA schema:** *conditional transfer of control
based on a value not known until execution.* A flat/tree data structure (the `StepTable` as
built) has no execution semantics beyond "here are values, walk them in this fixed order" —
it cannot select among **behaviors**, only supply **arguments**. The moment a table needs to
choose *which subsequent operations run* based on a runtime fact, it has stopped being a
description of values and become a description of *behavior contingent on unknown-at-authoring-time
data* — which is the definition of a program, not a data structure. `fork`'s parent/child
divergence (Q7 #6), argv's runtime-determined length (#7, a bounded-*by-runtime-value*
repeat — the same missing capability in a different shape: iteration count is itself a
runtime value the table can't pre-commit to), and acting on a sentinel (#8, demonstrated
above) are three instances of the **same one missing primitive**: conditional/data-dependent
control transfer. There are not three separate walls; there is one wall wearing three
costumes.

**Diagnostic test for self-deception, applied honestly (per the task's own instruction):**
would giving the table this field let me call it "data, not code"? No — and the reason is
principled, not aesthetic: **once a representation can, by itself, select among sequences of
future effects based on values it did not have at authoring time, it has the defining
property of an executable program** (this is the same boundary Turing/Rice-adjacent
arguments draw between "data" and "code": data has no operational semantics of its own,
programs do). Adding `next_on_fail` gives the schema exactly that property. So the honest
classification is: **a `StepTable` extended with conditional transfer is not "a slightly
richer data format" — it is a minimal bytecode VM**, indistinguishable in kind from the IR
this whole track already builds (`research/dynamic-core/ir/`). Building it would be
reinventing the IR one field at a time, which is precisely the "IDL slide" Q7's spec (and
this task's own §1.2) was designed to catch. **This experiment does not build that field.**

---

## ④ The tabifiable subset's boundary — what it buys, precisely

| capability | in the executed subset? | why |
|---|---|---|
| fixed-order multi-call sequencing | **yes — executed** | no runtime value picks the next step; `for` loop suffices |
| cross-call dataflow (struct field → next call's argument) | **yes — executed** | `ArgSrc::SlotPtrOff`, a bounded field-read, not a branch |
| struct-content-as-constant-blob (L3a content, given L3a layout via query) | **yes — executed** | reuses Q7's finding that content is data once layout facts are known |
| out-parameter read-back (`GetExitCodeProcess`'s `LPDWORD`) | **yes — executed** | `read_out`, a bounded post-call field-read, mirrors Q7's `Ret::OutParam` |
| conditional skip / error-path divergence (Q7 #8) | **no** | needs data-dependent control transfer (③) |
| `fork`-style parent/child divergence (Q7 #6) | **no** (not executed — no `fork` on Windows; same missing capability as #8, argued not reproduced cross-platform) | same |
| runtime-length argv/envp construction (Q7 #7) | **no** (not executed) | same missing capability, in "how many times" form rather than "which branch" form |

**The subset's real-world size.** Of Windows' 4-step `SpawnWait` (`CreateProcessA → Wait →
GetExitCode → CloseHandle`), **all 4 steps and 100% of the happy-path dataflow tabify** —
this experiment executed exactly that. What does **not** tabify is not "the orchestration"
in general, it is specifically: (a) branching on `CreateProcessA`'s own success/failure
(needed for a *robust* implementation — the happy-path-only table above is honest about
skipping it), and (b) anything with the same shape (SysV `fork`+branch, runtime-length
arrays). The Windows-vs-Linux step-count difference the task asked about (4 steps vs 3 +
fork-branch) is **not itself** the wall — a 3-step SysV-style sequence with no branch (if
one existed) would tabify exactly like the Windows 4-step one did. **The wall is the branch,
not the step count or the platform.**

---

## Judgement, walked against the task's own criteria

1. **①** Q7's "irreducibly code" bucket (5 items) is not homogeneous. It splits into
   **2 items that are linear dataflow** (tabifiable, now executed) and **3 items that are
   data-dependent control transfer** (not tabifiable without becoming a bytecode VM). This
   split was implicit in Q7's own per-item reasons (quoted in ①) but never stated as a
   boundary — Q21's contribution is naming and testing it.
2. **②** The fork-style branch (and its two siblings, sentinel-acting and runtime-length
   arrays) **cannot** be represented without adding data-dependent conditional transfer to
   the schema — checked by concrete attempt (a working linear table + an explicit failure
   demo showing exactly where it breaks), not by assumption. No jump/branch syntax was
   invented and relabeled as "data" — the temptation was named and explicitly not taken (③).
3. **③** The missing capability is precise: **conditional transfer of control keyed on a
   runtime value unknown at table-authoring time** (a program counter + data-dependent
   branch). A table gaining that capability gains the definitional property of an executable
   program, indistinguishable in kind from this track's own IR.
4. **④** The tabifiable subset is **the entire happy-path multi-call dataflow chain**
   (100% of Windows' 4-step SpawnWait, executed) — not a token sliver. The non-tabifiable
   remainder is exactly Q7's original hardest wall (#6/#7/#8), unchanged and now more
   precisely characterized rather than narrowed.

**Verdict: R1 has a bounded tabifiable subset (④) — linear multi-call dataflow, now proven
by execution — and a permanent, precisely-named residue: data-dependent conditional control
transfer (③), which cannot be added to a data schema without that schema becoming a
programming language.** This is a refinement of R1, not a reversal: Q7's "not any primitive
fixes it" stands for the control-transfer half; it was simply over-broad in also lumping in
the dataflow half, which Q7 never attempted (its own spec forbade multi-call lowering).
R1 remains a real, permanent residue of this architecture — the honest conclusion the task
asked to accept if that is what the evidence says, and it is.

---

## Reproduce (third-party runnable)

```powershell
cd research/dynamic-core/orchestration
mkdir out 2>$null
rustc --edition 2021 -O main.rs -o out/driver.exe
./out/driver.exe
```

Discipline checks:
```
grep -nE 'match|if ' step_table.rs      # only ArgSrc/width dispatch, no per-step/per-op branch
grep -c 'CreateProcess\|kernel32\|Win' step_table.rs   # 3, all in doc comments — engine is host-agnostic
```

Expected output: `exit code ... = 7` on the success path; `CreateProcessA success = false`
and `exit code the table reports anyway = 0` on the failure-demo path.

---

## Deviations from plan / honesty notes

1. **SysV `fork`/branch (Q7 #6) and argv/envp construction (#7) are analyzed, not executed**
   — no Linux host available (same posture as Q5/Q7/C6 in `SYNTHESIS.md`). The Windows
   `CreateProcessA` failure-path demo substitutes as an **executed** instance of the same
   underlying missing capability (data-dependent control transfer), argued in ③ to be the
   same wall in a different costume, not a proxy dressed up as equivalent without argument.
2. **`CloseHandle` omitted from the failure-demo table**, calling it on a `NULL` handle from
   a failed `CreateProcessA` risks undefined/version-dependent behavior under handle
   validation; the point (③) is already fully made by steps 0–2, so the demo table is 3
   steps instead of 4. Recorded, not hidden.
3. **`GetLastError` value in the failure demo (6, not the expected 2)** — not adjusted or
   hidden; explained in ② as itself a second demonstration of the same failure mode
   (blind continuation corrupts diagnostic state, not just the reported answer).
4. **No conditional-jump field was implemented**, by design (task §"硬纪律": stop at ①②③,
   do not build a second ISA/mini-language). ③'s argument for why it would be a VM is
   structural (program counter + data-dependent transfer = executable-program semantics),
   not a claim resting on an implementation that was then hidden.
5. **Rodata allocation in `step_table.rs::resolve` leaks memory** (`Box::leak` per
   resolution) — acceptable for a short-lived measurement driver; noted so it is not mistaken
   for a design recommendation.

**No metric was adjusted to flatter the result.** The reported split (2 tabifiable / 3 not)
keeps Q7's original hard wall exactly where Q7 put it; it does not claim R1 is closed.
