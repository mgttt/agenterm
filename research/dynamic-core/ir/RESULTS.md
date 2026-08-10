# Neutral-IR experiment — RESULTS

Decisive experiment for [`plan/archive/design-neutral-ir-experiment.md`](../../../plan/archive/design-neutral-ir-experiment.md):
**can a single IR artifact defer ALL ABI and layout decisions to lowering, and be
independently lowered to two genuinely incompatible ABIs (SysV x86_64 and Win64) that
both run correctly?** Measured, not argued. Clean-room; no prior implementation or
reference repo was consulted — only published technical knowledge (SysV ABI, Win64 ABI,
x86-64 ISA encoding, the Apple-Bitcode lesson).

---

## Verdict — **bounded neutrality (有边界可达)**

- **① is a PASS.** `pure_compute` lowers **byte-identically** through both ABIs and
  executes correctly (exit 163, matching the prior round). The boolean gate holds — the
  thesis is **not** falsified.
- **The ABI-placement half of the thesis holds cleanly.** Argument registers, stack
  spill order, the Win64 32-byte shadow space, the SysV red zone, and the return register
  were derived *entirely* from each call's semantic signature, with **zero** IR
  involvement — and the Win64 side **executes**, including `CreateFileA` (7 args → 3
  spilled onto the stack above the shadow) and `CreateProcessA` (10 args → 6 spilled).
  One neutral IR, two placements, both correct.
- **Neutrality has a sharp boundary, and it is not where the spec's headline warning
  pointed.** The ABI *mechanics* are deferrable; what leaks is the OS-interface
  *content* — reach (symbol names vs syscall numbers), out-param widths, and — the hard
  wall — **OS struct layout**. See the leak list (§②), the main product.
- **③ does not disqualify:** every emitted artifact is **< 200%** of the prior round's
  directly-compiled baseline, even with a deliberately naive (stack-slot, no register
  allocation) lowerer.
- **The deeper structural finding (the dilemma):** even *inside* the neutral subset,
  OS-call neutrality is bought with **encapsulation** — the IR names an intent
  (`FileOpen`), and the lowerer injects the target's real symbol, its extra constant
  args, and its reach mechanism. That is exactly the encapsulation the surrounding
  architecture forbids in its kernel. **"Neutral IR" and "no encapsulation" are in
  tension:** the neutrality achieved is encapsulation *relocated* from the kernel into
  the IR/lowerer, where it regrows linearly per intent (confirmed in ⑤).

This refines the prior round's leak. That round's "forced sysv64 + in-kernel
`sysv64→win64` bridge" conflated the *call mechanism* with *OS content*. This round shows
the **mechanism is genuinely neutral-izable** (two independent lowerings, both correct);
what is irreducibly per-target is the **OS interface content**, with struct layout the
part that has no neutral form at all.

---

## Measurement conditions (so the numbers are comparable)

| | |
|---|---|
| Compiler (host tool) | `rustc 1.97.0 (2d8144b78 2026-07-07)`, MSVC host |
| ISA | x86_64 only (spec §2 — ABI isolated from ISA) |
| Targets | **SysV x86_64** (Linux ABI + syscall reach) and **Win64** (Windows ABI + symbol reach) |
| IR shape | typed three-address / SSA-lite register IR (NOT a stack machine, NOT LLVM — spec §6). One value type: `Word` (pointer-width). Memory is byte/word granular. Calls carry a semantic signature only. |
| Lowerer | hand-written x86-64 emitter (`lower/asm.rs`), shared generic lowering (`lower/common.rs`), two ABI back-ends (`lower/sysv64.rs`, `lower/win64.rs`) |
| Register allocation | none — naive stack-slot (each IR temp → a frame slot). Inflates size; an optimizing lowerer would be smaller. Recorded as-is. |
| Byte counts | raw emitted **code-image** bytes (`out/*.bin`), no entry wrapper, no panic handler |

**Execution status (honest split, mirroring the prior round):**
- **Win64: built and RUN** against the real `kernel32`. All three payloads verified
  (below). The load-and-jump uses `VirtualAlloc`/`VirtualProtect`, and stdout is
  redirected to a temp file so the printed output is checked programmatically.
- **SysV: byte-measured, NOT executed.** The host is Windows with no WSL; a SysV
  lowering that touches the OS would place args for and `syscall` into a Linux kernel.
  Its bytes are real x86-64 (see the note under §① on why they are trustworthy), but it
  is not run — same posture as the prior round's Linux side.
- **`pure_compute` is the exception:** it has no OS surface, so its two lowerings are
  **byte-identical**, and executing it on Win64 *is* executing the SysV lowering. So the
  boolean gate ① has **real execution evidence on BOTH ABIs.**

---

## ① Neutrality — the boolean gate (PASS)

Driver output (`out/driver.exe`, run from `out/`):

```
emitted code size (bytes)   sysv64   win64   identical?
  pure_compute            281     281   true
  read_hash_print        1046    1249   false
  spawn_echo             1251    1557   false

== criterion ① — execution evidence (Win64 native, real kernel32) ==
  pure_compute  -> 163  (expected 163, prior round exit=163)  OK
  read_hash_print -> "a49d2cbecc13994f"  (expected "a49d2cbecc13994f")  OK
  spawn_echo    -> printed "exit=07", ret=7  (expected "exit=07", 7)  OK
```

- **`pure_compute` — NEUTRAL, executed on both ABIs.** The two lowerings are the *same
  281 bytes*; the Win64 run returns 163 (= the prior round's exit code). A leaf function
  with no calls needs no outgoing-arg area, so even the Win64 shadow space vanishes and
  the ABIs coincide exactly. **The gate is a pass; the thesis is not falsified.**
- **`read_hash_print` — Win64 executed correctly** (`a49d2cbecc13994f`, the prior round's
  reference hash of the fixed 35-byte input). This is the first payload that truly
  exercises ABI-divergent placement: `CreateFileA` (7 args) spills 3 args onto the stack
  above the 32-byte shadow on Win64, where SysV `open` needs only 3 registers. One IR
  call; the lowerer did the placement; it ran.
- **`spawn_echo` — Win64 executed correctly** (child `cmd.exe /c exit 7`; payload printed
  `exit=07` and returned 7). `CreateProcessA` (10 args) spills 6 onto the stack.

**Why the byte-measured SysV bytes are trustworthy without execution:** every opcode used
by the SysV back-end (`mov r,imm` / `mov r,[mem]` / `lea` / register `mov` / the ALU ops)
is *also* emitted and **proven by execution on the Win64 side** (they share `lower/asm.rs`
and `lower/common.rs`). The only SysV-exclusive opcode is `syscall` (`0F 05`), a
well-known 2-byte encoding. So the SysV code image rests on a proven encoder plus one
trivially-correct instruction. That is the same evidential basis the prior round used for
its (also unexecuted) Linux artifacts.

---

## ② Leak list — THE MAIN PRODUCT

Each entry is a construct that **forced** an ABI/layout/OS fact somewhere other than a
neutral IR. Under the discipline (spec §1.1–1.4) the IR itself stayed clean — no register
names, no `sizeof`, no convention names, no target hints — so *every* leak below is
"forced into the lowerer," and its cost shows up as **per-target code that grows with the
number of OS operations** (measured in ⑤). The list is non-empty but its boundary is
sharp; per spec §4 that is a result of equal value to an empty list.

### L1 — External references are not neutrally *nameable*. (all OS-touching payloads)
The IR can say "call the operation that opens a file for reading" (an intent) but cannot
*name* it. On Win64 it is the string `"CreateFileA"` in `kernel32`; on SysV it is syscall
number `2`. **Neither the string nor the number is neutral**, and they are not even the
same *kind* of thing. The binding lives entirely in the lowerer
(`win64.rs::SYMBOLS` — 9 strings; `sysv64.rs` syscall-number consts — 9 numbers), is
**disjoint** between targets, and **grows linearly with the number of intents**. This is
precisely the "grow the IR/POSIX to cover every reach" vector the spec warns is the
LLVM/Bitcode death — pushed out of the IR, but *not eliminated*, only relocated.

### L2 — Semantic arity ≠ native arity; target-only constant args must be injected. (all OS-touching payloads)
IR `FileOpen(path)` has **one** semantic arg. The native call is
`CreateFileA(path, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, 0, NULL)` — **7
args** — on Win64, or `open(path, O_RDONLY, 0)` — **3 args** — on SysV. The extra
arguments are target/API constants the lowerer supplies. So the neutral call signature is
an **abstraction over the real call** — i.e. the call is neutral *only because it is
encapsulated*. **This is the core dilemma:** neutrality of an OS call requires
encapsulation (an intent), which is the very thing the dynamic-core architecture rejects
(§1.2 there: "no `open()`, no portable file model"). **You can have a neutral IR, or
un-encapsulated OS access, but not both.**

### L3 — OS struct layout has NO neutral form. (headline; `spawn_echo`)
`CreateProcessA` requires a `STARTUPINFOA` whose `cb` field (offset 0) must equal the
struct's byte size (**104**), plus a `PROCESS_INFORMATION` whose `hProcess` sits at offset
0. Spec §1.1 forbids the IR from encoding `104`/offsets; §1.2 forbids the payload from
observing them; §1.4 forbids a target hint. **Yet the OS mandates them.** The only place
they can live is the target lowerer (`win64.rs::emit_spawn`). Consequence: the entire
`SpawnWait` operation **collapses to one coarse intent** whose two lowerings share
**nothing** — not the struct (Win64) vs no-struct (SysV fork/execve/wait4), not the
arity, not the reach mechanism. For this capability the "one IR" contributes only the
intent tag plus the neutral "format two digits and print" logic; **everything of
substance is per-target.** Neutrality here is a veneer.

### L4 — Out-parameter width is a layout fact. (minor; `read_hash_print`, `spawn_echo`)
`ReadFile`/`GetExitCodeProcess` deliver their real result through a pointer to a **32-bit**
field. Reading it back requires knowing it is 32 bits (`mov_rm32`). The neutral IR has
only `Word` (pointer-width) + byte granularity; a 4-byte out-param width is
target/API-defined. It was kept out of the IR only by hiding it inside the intent
lowering — and note the **per-target divergence**: SysV `read` returns the count directly
in the result register, so the *same intent* needs the out-param on one target and not
the other.

### L5 — Error/sentinel conventions differ. (minor; not exercised as a hard failure)
Win64 signals "invalid handle" as `(HANDLE)-1` and failure as a zero `BOOL`; Linux signals
failure as a negative return. A neutral "did it fail?" test cannot be written without
committing to one convention. The payloads sidestep it (they proceed regardless), but a
robust payload could not.

### NON-leak (the positive result) — ABI mechanics ARE deferrable.
Everything the spec's §1.1 primarily enumerated — *which register* an arg goes in, stack
spill order, the Win64 shadow space, the SysV red zone, the integer return register — was
lowered purely from the semantic signature, appears **nowhere** in the IR, and **executes
correctly** on Win64 for 7-arg and 10-arg calls. The ABI-placement half of the thesis is
clean. **All leaks are OS-interface content, not call mechanics.**

---

## ③ The cost of neutrality (size ratios)

Baseline = the prior round's variant-B payload **blobs** (the only "flat code image,
release, strip" artifacts that exist), which were compiled for **sysv64** (prior
deviation #3). So the like-ABI comparison is **my SysV emit vs the sysv64 blob**; the
Win64 emit is shown against the same blob as a cross-ABI reference (no directly-comparable
flat Win64 baseline was ever produced — prior Win64 artifacts are full PEs).

| payload | prior blob (sysv64, bytes) | my SysV emit | ratio | my Win64 emit | ratio |
|---|--:|--:|--:|--:|--:|
| pure_compute    | 166  | 281  | **169%** | 281  | **169%** |
| read_hash_print | 1128 | 1046 | **93%**  | 1249 | **111%** |
| spawn_echo      | 856  | 1251 | **146%** | 1557 | **182%** |

**No artifact trips the §4.3 200% ceiling — even with a naive, no-register-allocation
lowerer.** Caveats, stated so the number is not oversold: (a) the baseline is optimized
`rustc` output, mine is naive stack-slot; (b) the baseline blob includes an entry
trampoline + panic handler that my code-image number omits; (c) `pure_compute`'s 169% is
the compute-heavy case where the naive allocator's reload-per-op hurts most — an
optimizing lowerer would cut it sharply. The honest reading: **neutral lowering's size
cost is within budget**, and the budget headroom would only grow with a real register
allocator.

---

## ④ Lost expressiveness (what §1.2's ban forbids)

The ban ("the payload may not observe layout") makes the following permanently
inexpressible in the neutral IR. Ordered by how much each actually hurt these three
payloads:

| construct | status | did it bite? |
|---|---|---|
| **take a field's address / `offsetof`** | forbidden | **YES — fatal for `spawn_echo`.** `STARTUPINFOA.cb` and `PROCESS_INFORMATION.hProcess` both need a field offset. Only expressible by putting the offset in the lowerer → this *is* leak L3. |
| **out-param integer width** (`i32` field) | forbidden as an IR value type | **YES — minor.** Leak L4; hidden in the intent lowering. |
| **varargs** | forbidden (SysV needs `AL`=#SSE regs; Win64 duplicates floats into int+xmm) | **YES — indirectly.** Cannot call `printf`-family neutrally, so the payloads hand-roll formatting (the hex loop, the two-digit decimal loop are extra IR blocks that a vararg call would have replaced). A measurable expressiveness tax. |
| **struct/union by value** as param or return | forbidden (SysV eightbyte classification; Win64 hidden-ref for >8B; sret register differs) | not hit — the OS APIs used take structs **by pointer**. But that *forces* L3 (you must build the struct, which needs offsetof). |
| **unions** | forbidden (size = max member; layout target-defined) | not hit by these payloads |
| **bitfields** | forbidden (bit allocation is ABI/impl-defined) | not hit by these payloads |

Summary: the ban's teeth are `offsetof` (fatal at the struct boundary) and, mildly,
out-param widths and varargs. The by-value struct / union / bitfield bans were not
exercised by the three reused payloads, but remain permanent holes in expressiveness.

---

## ⑤ Lowering complexity — shared vs target-specific (lines of code, no comments/blanks)

| file | LOC | role |
|---|--:|---|
| `spec/ir.rs` | 123 | IR definition (shared; not a lowerer) |
| `lower/asm.rs` | 202 | x86-64 encoder — **shared, ISA-only** |
| `lower/common.rs` | 148 | generic lowering (arith/mem/control/call-dispatch) — **shared** |
| `lower/win64.rs` | 137 | **target-specific** (Win64) |
| `lower/sysv64.rs` | 109 | **target-specific** (SysV) |
| `payloads/payloads.rs` | 115 | the 3 payloads in neutral IR (shared across targets) |
| `main.rs` | 167 | driver/harness (not part of a lowerer) |

- **Shared lowerer = 350 LOC** (`asm` + `common`), written **once**, reused verbatim by
  both targets. Plus the 123-line IR spec, also shared.
- **Target-specific = 137 (win64) + 109 (sysv) = 246 LOC.**

The decisive sub-split (this is what spec ⑤ actually asks — "if the专属 part grows with
target count it is new outward growth"): **within each target file, the ABI-placement
mechanics are O(1); the OS-interface content is O(intents).**

| within a target file | ~LOC | scaling |
|---|--:|---|
| ABI placement (`AREG`/`SREG` reg list, arg-spill loop, shadow-space arithmetic) | ~20–30 | **fixed** — does not grow with capabilities |
| OS-interface content (symbol table / syscall numbers, per-intent constant args, the spawn struct/fork sequences) | ~90–110 | **grows per intent** |

So the genuinely worrying term is **not** the ABI back-end (a fixed ~25 lines per target)
but the OS-interface content, which is **O(targets × intents)** — the same linear growth
as leaks L1/L2, now measured as code. That term is the price of un-encapsulated OS access
under neutrality, and it is where a real system would eventually re-enact the
POSIX/LLVM-IR bloat if it kept adding intents.

---

## Reproduce (third-party runnable)

Raw `rustc`, no Cargo, no workspace involvement (matches the prior round's toolchain
discipline):

```powershell
cd research/dynamic-core/ir
rustc --edition 2021 -O -A dead_code main.rs -o out/driver.exe
cd out
./driver.exe
```

The driver prints the emitted byte sizes (③ numerators), dumps each lowered code image to
`out/<payload>.<abi>.bin`, and — on Windows — JIT-executes the Win64 lowerings against the
real `kernel32`, verifying: `pure_compute` → 163, `read_hash_print` →
`a49d2cbecc13994f` (of the fixed 35-byte input it writes to `input.txt`), `spawn_echo` →
prints `exit=07` / returns 7. Independent reference hash: FNV-1a/64 of
`"dynamic-core experiment 2026-08-08\n"` = `a49d2cbecc13994f`.

---

## Deviations from the spec (there are always some)

1. **SysV byte-measured, not executed** (no WSL) — as the cover note explicitly permits;
   `pure_compute` is executed on both ABIs because its two lowerings are byte-identical.
2. **`Exit`/`Ret` both lowered as "return the code in rax"** so the JIT harness can read
   results. A standalone build would replace `Exit` with a target exit call (Linux
   `exit` syscall / Win64 `ExitProcess`) — a handful of extra bytes not counted in ③.
3. **Naive stack-slot lowerer (no register allocation).** Inflates ③; an optimizing
   lowerer would be smaller. Chosen for auditability; recorded rather than optimized
   away (spec §4 forbids tuning the metric to flatter the result).
4. **Directory layout** follows the spec's suggestion (`spec/`, `lower/`, `payloads/`)
   but the crate is compiled as one `rustc` invocation via `#[path]` modules (no Cargo),
   so it never touches the root workspace.
5. **The `SpawnWait` intent is coarse by necessity, not by choice** — see L3. It could not
   be decomposed into finer neutral-signature intents because its operands are structs,
   which the IR is forbidden to describe.
