# Q7 — OS-interface content as DATA — RESULTS

Decisive experiment for [`plan/design-os-interface-as-data-experiment.md`](../../../plan/design-os-interface-as-data-experiment.md):
**can Q1's one seam — the OS-interface content (leaks L1–L5) — stop being per-target
hand-written code and become DATA (tables) interpreted by one fixed marshaller?**
Measured on Q1's exact IR + payloads, reused verbatim. Clean-room within the track; no
external implementation consulted — only the published CO-RE/BTF and ANDF lessons from R1.

---

## Verdict — **bounded reachable (有边界可达)**

The seam **narrows but does not close.** Table-driving converts the OS-interface content
from per-target code to data **for the single-native-call family** — and there it *fully*
succeeds: the file-I/O family (Alloc/FileOpen/FileRead/FileClose/WriteStdout) is lowered
**entirely from data by one fixed marshaller** and **executes correctly** (pure→163,
read_hash_print→`a49d2cbecc13994f`). For that family the marginal **code** cost of +1
intent and +1 (same-ISA) target is **0** — all growth is data, and the data is
schema-checkable.

It does **not** close for two residues, and the honest finding is *which* two:

1. **L3b — orchestration / control flow.** Multi-call dataflow (extract `hProcess` →
   feed Wait → GetExitCode), and above all **control-flow branches** (SysV
   `fork` then branch on pid; child `execve`, parent `wait4`). No flat table expresses a
   conditional branch. Forcing it into data means inventing a call-sequencing bytecode —
   **the IDL slide the experiment was built to detect** (spec §1.2). This is code, and no
   host oracle fixes it.
2. **I2 — cross-ISA intent restructuring** (surfaced by Q5, analyzed here, not executed).
   Across ISA an intent doesn't just **renumber**, it **restructures**: aarch64-Linux has
   **no `open`** (must `openat`+`AT_FDCWD`), **no `fork`** (must `clone`). Renumbering
   tablifies; restructuring changes arity/semantics/sequence — it is L3b wearing the L1
   mask. This is the concrete mechanism of ANDF's "long tail you can't cover."

**On the L3 reframe (CO-RE / 5th primitive):** L3 **splits**. Its layout half (**L3a** —
`STARTUPINFOA.cb=104`, `hProcess@0`) **does** tablify — but only in **query form**: the
table carries the *name* `("STARTUPINFOA","cb")`, a host **layout oracle** answers it, the
number is folded in at bind time (BTF/CO-RE, R1 §4.1e). That is **not payload-side data
alone — it needs a query channel = the missing 5th primitive Declare** (R1 §10.5). So the
answer to "is L3 tablable?" is: **the layout facts need Declare; the orchestration needs
nothing that exists** — it is control flow, permanent residue of this architecture.

---

## Measurement conditions

| | |
|---|---|
| Host tool | `rustc 1.97.0 (2d8144b78 2026-07-07)`, MSVC host (same as Q1/Q4) |
| Base under test | Q1's `ir/` reused verbatim via `#[path]`: `spec/ir.rs`, `lower/asm.rs`, `payloads/payloads.rs`. **Unchanged.** |
| New code | `table.rs` (DATA), `marshal.rs` (fixed engine + validator + boundary), `main.rs` (driver + JIT harness). Non-Call lowering copied verbatim from Q1 `common.rs`. |
| ISA | x86_64 only (spec §2; ISA axis is Q5's, folded in as findings). |
| Targets | **Win64** (ABI + symbol reach) and **SysV x86_64** (ABI + syscall reach). |
| Execution | Win64 JIT-run against real `kernel32`, from **table-driven bytes**. SysV byte-measured, not executed (no WSL) — same posture as Q1/Q4. |
| Byte counts | raw emitted code-image bytes (`out/*.bin`). |

---

## ① Expressiveness — the boolean gate (per leak class)

| leak | what it is | verdict | evidence |
|---|---|---|---|
| **L1** naming (symbol string vs syscall#) | `CreateFileA` vs `open`=2 | **DATA** (renumber) | `OpSpec.reach_id` — symbol index (Win) / syscall number (SysV). Executed. |
| **L1/I2** naming that **restructures** across ISA | aarch64 `openat`/`clone`, no `open`/`fork` | **NOT DATA (code)** | Q5. A different *set*, not a different number — restructures arity/sequence. Analyzed, not executed (single-ISA). |
| **L2** semantic arity ≠ native arity + const injection | IR `FileOpen(path)` → 7-arg `CreateFileA` | **DATA** | `OpSpec.args = [Sem(0), Const(0x8000_0000), …]`. Executed (1 sem arg → 7 native, all constants in the table). |
| **L4** out-param width + per-target result divergence | Win `ReadFile`→&DWORD; SysV `read`→rax | **DATA** | `Ret::OutParam{width:4}` (Win) vs `Ret::Direct` (SysV) — same intent, different rows. Executed. |
| **L3a** struct **layout** facts | `cb=104`, `hProcess@0` | **DATA, but only in QUERY form** | `StructSpec` with `FieldSrc::Queried{struct_name,field,…}`; validated by schema. Needs a host oracle (Declare) — not payload-side alone. |
| **L3b** struct **orchestration** / control flow | build→call→extract→wait; SysV fork/branch | **NOT DATA (code)** | see ⑤. SpawnWait is absent from both tables → the single-call marshaller **cannot lower it**. |
| **L5** error/sentinel | `(HANDLE)-1`, negative return | **PARTIAL** | the sentinel *value* is data; *acting* on it needs a branch = code (Q4: undecidable). |

**Gate result:** L1(renumber)/L2/L4 pass as data and **execute** → the kill criterion
(spec §4: "if L1 or L2 can't be data, thesis falsified") is **not** triggered; the thesis
holds on its simplest classes. The boundary is L3b + I2, quantified in ⑤.

---

## ② Growth curve — THE MAIN JUDGEMENT (slope)

Discipline enforced structurally (spec §1.1): the engine contains **zero** `match intent`
and **zero** target-name branch (`grep -c 'match intent' marshal.rs` → only a doc comment;
`grep -c 'abi.name ==' marshal.rs` → 0; the intent table is a **data field** `AbiDesc.table`).
Target behaviour is selected only through data fields (`abi.arg_regs`, `abi.shadow`,
`abi.reach`, `abi.table`).

| marginal action | **code** increment (engine LOC) | **data** increment (table LOC) |
|---|--:|--:|
| **+1 intent** (single-call family) | **0** | ~5–13 (one `OpSpec` row per target; 5 shape-diverse intents live in ~46 LOC) |
| **+1 target** (same ISA, existing reach mechanism) | **0** | ~57–58 (one `AbiDesc` + one intent table) |
| +1 target with a **novel reach mechanism** (e.g. ARM `svc`) | **+1 `match abi.reach` arm** (fixed set = 2 known) | as above |
| **+1 intent that restructures across ISA (I2)** | **> 0 (code)** | — — thesis fails here |

**Reading:** for the single-call family the marginal **code** cost is **0** on both axes —
*all* growth is data. This is the positive result: **mechanism fixed, capability by data**,
demonstrated on the exact seam Q1 said regrows at O(targets × intents). The slope judgement
(spec §4 step 1) **passes** for this family. It **fails** at I2 (cross-ISA restructuring)
and at any orchestration intent (⑤) — those still cost engine code.

Win64 and SysV share **100%** of the engine (70 LOC single-call core + 42 LOC L3a) and
**100%** of the 116-LOC copied non-Call lowering; they differ *only* in data.

---

## ③ Cost — the fixed marshaller vs Q1's per-target code (LOC, non-comment/blank)

| component | LOC | scaling |
|---|--:|---|
| **engine: single-call core** (`max_outgoing`+`emit_intent`+`load_arg`+`alloc_spec`) | **70** | **fixed** — serves all intents, all same-ISA targets |
| engine: L3a struct-building (`build_struct`) | 42 | fixed |
| schema types (`AbiDesc`/`Arg`/`OpSpec`/`StructSpec`…) | 47 | fixed (shared) |
| copied non-Call lowering (verbatim Q1 `common.rs`) | 116 | shared, **not new** |
| `validate` (④, measurement) | 41 | fixed |
| `spawn_boundary` (⑤, measurement) | 23 | fixed |
| **per-target DATA** (win / sysv) | 57 / 58 | **per target (data)** |
| **per-intent DATA** | ~5–13 | **per intent (data)** |

**Q1 baseline — and the one comparison that must be stated carefully.** `win64.rs`=137,
`sysv64.rs`=109 total; the OS-interface-content portion was **~90–110 LOC/target and grows
per intent AND per target** (Q1 ⑤).

> **⚠️ The two sides do not cover the same capability set.** Q7's engine handles the
> **single-native-call family only** (Alloc/Open/Read/Close/Write); **`SpawnWait` is absent
> from both tables and the marshaller cannot lower it** (⑤). Q1's ~90–110 LOC/target
> **includes exactly the part Q7 excludes** — the spawn struct-building and the SysV
> fork/branch sequence. So "**70 LOC fixed vs 90–110 LOC/target**" as a bare head-to-head
> is **not like-for-like**, and it also silently dropped Q7's own **57–58 LOC/target of
> data**. Stated correctly, **inside the single-call family**:
>
> | | fixed cost | per same-ISA target | per intent |
> |---|--:|--:|--:|
> | **Q7** (single-call family) | 70–112 LOC **engine code** | **57–58 LOC of DATA**, 0 LOC code | ~5–13 LOC of DATA, 0 LOC code |
> | **Q1** (same family **+ spawn**) | — | **90–110 LOC of CODE** | grows |
>
> **The slope conclusion is untouched by this correction** — it never rested on the
> intercept: +1 intent and +1 same-ISA target cost the engine **0 lines of code**, verified
> structurally (`grep -c 'abi.name ==' marshal.rs` → 0; `match .*intent` hits only a doc
> comment), and *that* is Q7's product. See [`../COMPARABILITY.md`](../COMPARABILITY.md) §2 U4.

Q7 replaces the growing term with a **fixed ~70–112 LOC engine + ~9 LOC/intent of data +
~57–58 LOC/target of data**. Crossover is near two targets; beyond it Q1 grows as
targets × intents in **code** while Q7's **code** stays flat and only its **data** grows.

**Emitted bytes: the marshaller adds 0 bytes** and matches/*beats* Q1:

| payload | Q7 sysv | Q1 sysv | Q7 win | Q1 win |
|---|--:|--:|--:|--:|
| pure_compute | 281 | 281 | 281 | 281 |
| read_hash_print | **1046** | 1046 | **1216** | 1249 |

`pure_compute` is byte-identical to Q1 (non-Call path verbatim). `read_hash_print` SysV is
byte-identical (1046); Win64 is **33 bytes smaller** because WriteStdout is now a single
`WriteFile` (the stdout HANDLE is resolved into `ctx[2]` at bind time) instead of Q1's
inline `GetStdHandle`+`WriteFile` — see the deviation note.

---

## ④ Verifiability gain — what a schema can check that code could not

The `validate` pass (41 LOC) walks each `OpSpec` and checks: reach index within the symbol
table; every `Sem(k)` within the intent's semantic arity; `CtxWord`/`StructPtr` in range;
out-param width ∈ {4,8}; struct field offsets fit inside the struct. Result: **5 rows/target,
0 errors**; a **negative probe** (declare FileOpen arity 0) is **rejected** → non-vacuous.

- **What Q1 could not have:** hand-written `emit_call` is opaque code — nothing to validate.
  The table is a **structural object**, so every OS-interface recipe now has a
  well-formedness check *before* a byte is emitted.
- **Against Q4's ceiling:** Q4 measured the file-I/O intent region at **~30–41%** of the
  artifact and **0% structurally verifiable**. Q7 gives that region an **input-validation**
  anchor: the recipe is schema-checked. The unverifiable fraction is **not** driven to 0 —
  it is **reclassified** from "opaque bytes" to "bytes generated from a validated recipe."
- **What the schema still CANNOT check (honest):** whether symbol index 1 *really means*
  `CreateFileA`, whether syscall 2 *really means* `open`, whether `0x8000_0000` *really is*
  `GENERIC_READ`. **The schema validates the recipe's shape, never its naming truth (L1).**
  So Q7 narrows the *structural* hole (malformed recipes are caught) but not the *binding*
  hole (the name→number truth is still trust — Thompson survives here). Narrowing is real
  and bounded.

---

## ⑤ The boundary — THE MAIN PRODUCT (where table-driving fails)

`spawn_boundary()` classifies each fact of `SpawnWait` as {data | query | code}. Driver output:

```
[query] STARTUPINFOA.cb size/offset            L3a: bounded field-offset relocation; CO-RE host oracle
[query] PROCESS_INFORMATION layout             L3a: field offsets tablify the same way
[ data] command-line byte string               a rodata blob — already data
[ code] extract hProcess from PROCESS_INFO out L3b: runtime pointer read feeding the NEXT call — cross-call dataflow
[ code] sequence CreateProcess->Wait->GetExit  L3b: multi-call ORCHESTRATION = a call-sequencing bytecode = the IDL slide
[ code] SysV fork() then BRANCH on pid         L3b: two divergent control-flow paths — the hardest wall
[ code] SysV argv[]/envp[] pointer-array build L3b: runtime pointer arithmetic, not constant fields
[ code] error/sentinel (L5)                     recording is data; ACTING needs a branch = control flow
--> tablable-as-data=1  needs-query-channel=2  IRREDUCIBLY-code=5
```

**The boundary, stated as the general shape (beyond the 3 payloads — the ANDF long tail):**

| OS-interface fact | table-drivable? | why |
|---|---|---|
| symbol name / syscall number (**L1 renumber**) | **yes (data)** | a leaf name→index |
| semantic→native arity + const injection (**L2**) | **yes (data)** | a fixed-length arg recipe |
| out-param width / result shape (**L4**) | **yes (data)** | an enum |
| struct field offset/size/const field (**L3a**) | **yes, via QUERY + host oracle (Declare)** | bounded CO-RE relocation kinds |
| multi-call orchestration + dataflow (**L3b**) | **no (code)** | needs a call-sequencing language = IDL |
| conditional control flow — `fork`/branch (**L3b**) | **no (code)** | no flat table has a branch |
| **cross-ISA intent restructuring (I2)** | **no (code)** | `openat`/`clone`: the syscall *set* changes shape, not just number |
| varargs (`printf`) | **no** | arg count is runtime-varying; recipe is fixed-length |
| callback / function pointer to OS | **no** | the callee is generated code, not a constant |
| ioctl-shaped inout structs, long tail | **no** | unbounded per-request shapes — ANDF drowned here |

**Dimensional correction (Q5, folded in):** the seam is **not one flat per-target table**.
Its components have **different dimensions**: **layout = per-OS** (L3 identical on x64 &
ARM64 Windows), **reach = per-(ISA, OS)** (aarch64 numbers all differ), **ABI placement =
per-ISA** (and on aarch64 Linux/Windows share AAPCS64, so Q1's "ABI is per-target" is itself
ISA-relative). Q7's single-ISA table **collapses** these axes; a correct multi-ISA design
must factor reach (ISA×OS) from layout (OS) from ABI (ISA). This experiment measures only
the OS/ABI axis at fixed ISA; the ISA axis is where **I2** bites, flagged not executed.

---

## Decision trace (spec §4 tree, walked)

1. **Main = ② (slope).** Single-call family: +1 intent = 0 engine LOC, +1 same-ISA target
   = 0 engine LOC. **Does not judge negative.** → continue.
2. **② holds for the family → ① :** L1(renumber)/L2/L4 = data (executed); L3a = data-via-query;
   L3b + I2 = code. Kill門 (L1/L2 as data) **not** triggered.
3. **⑤ = main product:** boundary is **orchestration/control-flow (L3b) + cross-ISA
   restructuring (I2)**, not layout. L3 verdict: **layout needs the 5th primitive (Declare);
   orchestration needs control flow no primitive supplies.**
4. **③④ quantify:** engine is a fixed ~70–112 LOC + ~57–58 LOC/target of **data**, against
   Q1's growing ~90–110 LOC/target of **code** — **like-for-like only inside the single-call
   family**, since Q1's figure includes the spawn sequence Q7's engine cannot lower at all
   (see the ⚠️ box under ③). What the verdict rests on is the **slope** (+1 intent / +1 same-ISA
   target = 0 engine code), not that intercept pair. Schema validation is real but checks
   shape, not naming truth.

**Verdict: bounded reachable.** The one-seam narrows to data for single native calls and
stays code for control flow and cross-ISA restructuring.

---

## Reproduce (third-party runnable)

```powershell
cd research/dynamic-core/tables
mkdir out 2>$null
rustc --edition 2021 -O -A dead_code main.rs -o out/driver.exe
cd out; ./driver.exe
```

The driver lowers `pure_compute` and `read_hash_print` through the **table-driven**
marshaller for both ABIs, prints emitted sizes, runs schema validation (+ a negative probe
that must be rejected), and — on Windows — JIT-executes the Win64 bytes against real
`kernel32`, verifying `pure_compute`→163 and `read_hash_print`→`a49d2cbecc13994f` (FNV-1a/64
of the fixed 35-byte input). It then prints the SpawnWait boundary classification.
Independent reference hash: FNV-1a/64 of `"dynamic-core experiment 2026-08-08\n"` =
`a49d2cbecc13994f` (matches Q1/Q4).

**Discipline checks (third-party):**
```
grep -c 'abi.name ==' marshal.rs      # -> 0  (no per-target branch)
grep -nE 'match .*intent' marshal.rs  # -> only the doc comment on line 4
```

---

## Deviations from the spec

1. **SpawnWait is not executed** — it is *unlowerable* by the single-call marshaller by
   design; that unlowerability IS the ⑤ result. Its layout half is written as `StructSpec`
   data and schema-validated to prove L3a tablifies, but no spawn bytes are emitted (spec
   §1.4 forbids a hand-written escape hatch, so there is none).
2. **WriteStdout made single-call via `ctx[2]`.** Q1 emitted `GetStdHandle`+`WriteFile`
   (a 2-call sequence). To keep it in the single-call family the stdout HANDLE is resolved
   by the host into `ctx[2]` (modelling the kernel's ③ reach resolving handles at bind
   time). This is a **legitimate relocation** (handle acquisition is binding, not the call)
   but it is recorded, not hidden: it is *why* Win64 rhp is 33 bytes smaller than Q1, and
   it shows that "single-call tablability" sometimes requires pushing handle-acquisition
   into binding — a mild form of the same encapsulation-relocation Q1 found.
3. **Layout oracle is a stub.** `FieldSrc::Queried` carries the answer inline rather than
   calling a real BTF-equivalent; it exists only to *test the CO-RE reframe's shape* (spec
   §7 explicitly defers the oracle's source). The finding is "L3a needs a query channel,"
   not "here is the oracle."
4. **I2 (cross-ISA restructuring) analyzed, not executed** — single-ISA host per spec §2;
   folded in from Q5 as a boundary finding.
5. **Non-Call lowering copied, not `#[path]`-reused** from Q1 `common.rs` (its `lower_*` are
   private), same posture as Q4. Emit logic verbatim → `pure_compute` still 281 and
   byte-identical, `read_hash_print` SysV still 1046.

**Honesty clause:** no metric was adjusted to flatter the result. The Win64 byte *drop*
(1249→1216) is reported with its cause (deviation 2), not claimed as an optimization. The
verdict reports the seam as **permanently code at L3b/I2**, which is the less flattering
reading, per spec §1.2's honesty requirement.
