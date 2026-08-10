# Q5 ISA-axis experiment — RESULTS

Decisive experiment for [`plan/archive/design-isa-axis-experiment.md`](../../../plan/archive/design-isa-axis-experiment.md):
**add a second ISA (aarch64) beside Q1's x86-64 — is the neutral IR still neutral on
the ISA axis, and does "N kernels, one per ISA" stay bounded?** Measured, not argued.
Clean-room: the AArch64 encoder is written from the published ARM ARM A64 encodings;
Q1's IR + payloads are reused **byte-identical**.

---

## Verdict — **§0.2 assertion HOLDS; the N×M→N collapse stands (with a quantified per-ISA line item)**

- **① ISA-neutrality gate: PASS.** `spec/ir.rs` and `payloads/payloads.rs` are
  **byte-identical** to Q1 (`cmp` clean) — the three payloads lowered to a second ISA
  with **zero IR edits**, no new value type, no ISA hint, no escape hatch. On aarch64,
  `pure_compute` lowers **identically** for both targets (192 bytes each), mirroring
  Q1's x86 result. The register/stack mechanics, being derived from the semantic
  signature, retargeted cleanly.
- **The per-ISA cost is real but BOUNDED and CONSTANT-IN-INTENTS.** Adding aarch64 cost
  a new **~307-LOC per-ISA bucket** (encoder 150 + generic lowering 157) and a **+13%
  kernel** (four primitives 568 B → 644 B). Both are **linear in ISA count only** — they
  do **not** grow when you add OS operations or capabilities (those stay in the
  per-target files). So `N (ISA) × M (OS/intents) → N` holds: the M axis is absorbed by
  per-target content, the N axis is a fixed per-ISA bucket.
- **§0.2's literal claim — "kernel size does not grow with ISA count, build N copies" —
  is CONFIRMED.** Each kernel is built for one ISA and contains only that ISA's code;
  the aarch64 four-primitive kernel is ~644 B, same order as x86's ~568 B. Adding
  aarch64 to a fleet = one more small kernel, not a bigger kernel. **Not falsified.**
- **What the assertion glossed, now quantified:** it named "the kernel" but the
  IR→native **lowerer** is a separate per-ISA artifact (~307 LOC) it never costed. That
  cost is bounded and constant-in-intents; whether it burdens the kernel depends on
  Q2's open in/out-of-kernel decision. Either way it does not grow a single kernel with
  ISA count.
- **⑥ cost: comfortably within budget** — every aarch64 blob is **59–116%** of Q1's
  baseline (all < 200%), and *smaller than the x86 emit* from the same naive lowerer.

---

## Measurement conditions
| | |
|---|---|
| Host tool | `rustc 1.97.0 (2d8144b78 2026-07-07)`, MSVC host, x86-64 |
| ISAs | x86-64 (Q1) + **aarch64** (this round) |
| aarch64 targets | **a64-linux** (AAPCS64 + SVC syscalls) and **a64-win** (AAPCS64 + kernel32 symbol reach) |
| IR | reused from Q1, **byte-identical** (typed 3-address, one `Word` type) |
| lowerer | hand-written A64 encoder (`a64.rs`), generic lowering (`common_a64.rs`), two targets |
| register allocation | none — naive stack-slot, same as Q1 (comparability) |
| byte counts | raw emitted code-image bytes (`out/out/*.bin`) |

**Execution status (honest split, mirroring Q1's SysV side):** aarch64 is **not
executable** on this Windows-x86 host (no qemu, no hardware). So aarch64 is
**byte-measured and encoder-validated, NOT executed.** Validation is stronger than
Q1's unexecuted SysV side: **every emitted A64 instruction word is checked against LLVM
ground truth** (`rustc --emit obj --target aarch64-unknown-linux-gnu`), and the
lowering *structure* is identical to Q1's x86 path, which **was executed** on Win64.

---

## ① Neutrality gate — PASS

```
== encoder validation vs LLVM ground truth ==
  26/26 instruction encodings match LLVM
  encoder: ALL MATCH LLVM

emitted aarch64 code size (bytes)   a64-linux   a64-win   identical?
  pure_compute             192       192   true
  read_hash_print          660       728   false
  spawn_echo               840       956   false
```

- IR + payloads byte-identical to Q1 (`cmp isa/spec/ir.rs ir/spec/ir.rs` → clean;
  same for payloads.rs). **No IR construct was added or bent to admit aarch64.**
- `pure_compute` (no OS, no ctx) lowers to the **same 192 bytes** for both aarch64
  targets — the leaf-function ABI coincidence Q1 saw on x86 also holds on aarch64.
- **Encoder validated 26/26 against LLVM** (movz/movk, add/sub/mul/eor/and/orr, lsl/lsr,
  ldr/str x & w, ldrb/strb, cmp, cset, svc, blr, ret, add/sub-imm, add-sp, mov-reg).
  **Branch fixups separately validated** against LLVM: backward `b -2 words` =
  `0x17FFFFFE`, `cbnz x9, -3 words` = `0xB5FFFFA9`, forward `cbz +2 words` =
  `0xB4000049` — all exact.

**Why the byte-measured aarch64 bytes are trustworthy without execution:** every
opcode the lowerer emits is one of the 26 LLVM-validated encodings plus the three
LLVM-validated branch forms; the lowering logic that sequences them is the *same
Rust* as Q1's x86 path, which executed correctly on Win64 for 7- and 10-arg calls. So
the aarch64 code image rests on an LLVM-validated encoder + a proven lowering
structure — a firmer basis than Q1's (also unexecuted) SysV artifacts.

---

## ② Three-way split — Q1's two-way split fails on the ISA axis

Q1 measured **shared (350) vs target-specific (246)**. But Q1's "shared" 350 included
the **x86-only** encoder (`asm.rs`, 202) — counted as shared only because both targets
were the same ISA. Adding aarch64 forces a **three-way** split. LOC = non-blank,
non-comment lines.

| bucket | files | LOC | grows with |
|---|---|--:|---|
| **SHARED** (ISA-neutral, OS-neutral) | `spec/ir.rs` 123 + `payloads.rs` 115 | **238** | nothing — one copy serves all ISAs × all OSes (byte-identical Q1↔Q5) |
| **PER-ISA** — x86-64 | `asm.rs` 202 + `common.rs` 148 | **350** | **ISA count only** (once per ISA) |
| **PER-ISA** — aarch64 | `a64.rs` 150 + `common_a64.rs` 157 | **307** | **ISA count only** |
| **PER-TARGET** — x86 SysV/Linux | `sysv64.rs` | 109 | (ISA×OS) intents |
| **PER-TARGET** — x86 Win64 | `win64.rs` | 137 | (ISA×OS) intents |
| **PER-TARGET** — aarch64 Linux | `a64_linux.rs` | 99 | (ISA×OS) intents |
| **PER-TARGET** — aarch64 Windows | `a64_win.rs` | 118 | (ISA×OS) intents |

**Key structural finding — the ABI-placement bucket MOVES between ISAs.** On x86 the
two ABIs (SysV vs Win64) have *different* arg registers + shadow space, so arg
placement lived **per-target** (inside sysv64.rs/win64.rs, ~20–30 LOC each — Q1's
sub-split). On aarch64 **both** Linux and Windows use **AAPCS64** (same x0–x7 args,
same x0 return, no shadow space), so arg placement is **per-ISA** (one
`common_a64::place_args`, ~18 LOC, shared across both aarch64 targets). That is why the
aarch64 per-target files are *thinner* than x86's (99/118 vs 109/137): the placement
they each carried on x86 factored out. **Q1's "ABI is a per-target axis" conclusion is
itself ISA-dependent** — on aarch64 the ABI axis largely collapses into the ISA.

---

## ③ Per-ISA share — the MAIN criterion

**The per-ISA bucket for a second ISA = 307 LOC** (encoder 150 + generic lowering 157;
x86's was 350). Growth analysis:

| does the per-ISA bucket grow with… | answer |
|---|---|
| number of intents / OS operations | **NO** — those go in per-target files. The encoder and generic lowering are fixed. |
| number of OS targets on that ISA | **NO** — one encoder serves all OSes on the ISA (proven: `a64.rs`/`common_a64.rs` serve both a64-linux and a64-win). |
| number of ISAs | **YES, linearly** — one bucket per ISA (2 ISAs → 2 buckets). Not multiplied by OS or intents. |

So the per-ISA share is **O(ISA count)**, **constant in intents/OS** — the well-behaved
kind of growth. It is a genuine per-ISA *fixed cost* (larger than any single per-target
file), but it does **not** feed the N×M blow-up: the M axis (OS × intents) stays in the
small per-target files (99–137 LOC), and adding an ISA adds exactly one encoder+lowering
bucket. **§0.2's collapse holds; the cost model just gains the line item "+1 ISA ≈ +300
LOC lowerer, once."**

---

## ④ Kernel per-ISA delta — the four primitives, both ISAs

Compiled `kernel/prim.rs` (four primitives, Linux path) for both ISAs
(`--target {x86_64,aarch64}-unknown-linux-gnu -O -C panic=abort --emit obj`),
`.text` sizes via `kernel/textsize.rs`:

| primitive | x86-64 (B) | aarch64 (B) | Δ |
|---|--:|--:|--:|
| `raw_syscall` (hand-written asm — the ONLY per-ISA source) | 29 | 44 | +15 |
| `mem_alloc` (portable Rust) | 34 | 36 | +2 |
| `mem_protect` (portable Rust) | 27 | 40 | +13 |
| `call` — ④ trampoline (portable, rustc places args) | 365 | 396 | +31 |
| `exit` (portable Rust) | 34 | 32 | −2 |
| `load_and_run` — ② loader (portable Rust) | 77 | 92 | +15 |
| **TOTAL .text** | **568** | **644** | **+76 (+13%)** |

- **Almost the entire kernel is portable Rust that rustc re-targets for free.** The
  only hand-written ISA-specific source is `raw_syscall` (the `syscall`/`svc #0`
  instruction + its register binding, 29→44 B). Everything else — memory, the ④ call
  trampoline, exit, the loader — is *identical source*; rustc emits the per-ISA
  instructions.
- **The second ISA's kernel does not grow with ISA count** — it is a fresh ~644 B
  build for that one machine. This is exactly §0.2's claim, now measured. (Q0's full
  ~2.7 KB adds ELF overhead, mem* intrinsics, the table, and entry bootstrap; the
  four-primitive *core* is 568 B on x86, so aarch64's core at 644 B is the same order.)

---

## ⑤ ISA-axis leak list (new leaks vs Q1's ABI-axis L1–L5)

Q1's L1–L5 were all **OS-interface content**. The ISA axis adds a distinct set. Under
the discipline the IR stayed clean, so each is "forced into the lowerer."

### I1 — Reach content is per-**(ISA,OS)**, not per-OS. (refines Q1 L1)
aarch64-Linux syscall numbers **differ** from x86-Linux: read 63 (not 0), write 64
(not 1), mmap 222 (not 9), exit 93 (not 60). Q1 treated the syscall table as an OS
fact; it is actually indexed by (ISA,OS). Adding an ISA **duplicates** the reach table,
not shares it.

### I2 — The available syscall SET shifts with ISA. (genuinely new)
aarch64-Linux has **no `open`** (must use `openat(AT_FDCWD, …)` — an extra constant
arg) and **no `fork`** (must use `clone`, whose arg order is itself arch-specific). So
the same intent (`FileOpen`, `SpawnWait`) does **not** merely renumber across ISAs — it
**restructures** (different arity, different primitive). The neutral IR's intent absorbs
this (encapsulation again — Q1's core dilemma), but the per-target lowering *content*
diverges more than a number swap.

### I3 — Immediate range is an ISA fact (the "立即数范围" the spec flagged). (new; NON-leak — stays in lowerer)
x86 embeds an arbitrary 64-bit immediate in one `mov r, imm64`. aarch64 **cannot**: a
64-bit constant needs up to **4** `movz`/`movk` instructions, and ALU-immediate forms
are limited (12-bit for add/sub; bitmask-encodable patterns for logical). The IR's
`Const(u64)` never exposes this — the lowerer just emits more instructions. **Verified
it does NOT leak into the IR**; it is a per-ISA code-size cost only.

### I4 — Alignment / offset scaling is an ISA fact. (new; NON-leak — stays in lowerer)
aarch64 scaled load/store offsets must be a multiple of the access width (8 for 64-bit,
4 for 32-bit) and fit 12 bits; x86 allows any disp32. The lowerer must keep frame slots
8-aligned (they are). Absorbed by the lowerer; the IR never sees it.

### I5 — The ABI-placement axis has a different SHAPE per ISA. (new; structural)
On x86 the ABI axis is per-target (SysV ≠ Win64: arg regs + shadow space). On aarch64
both OSes share AAPCS64, so the ABI axis **collapses into the ISA** (§②). **Q1's
isolation of "ABI vs ISA" is ISA-relative** — what is a per-target difference on x86 is
a per-ISA constant on aarch64.

### NON-leaks (positive results on the ISA axis)
- **The IR needed zero changes** — no new value type, no ISA hint, no escape hatch.
  The headline neutrality question ("does the IR leak on the ISA axis?") is **NO** for
  this payload subset.
- **OS struct layout (Q1's headline L3) is ISA-INDEPENDENT within an OS family.**
  `STARTUPINFOA.cb == 104`, `PROCESS_INFORMATION.hProcess @ 0` are identical on x64 and
  ARM64 Windows (both LLP64, 8-byte pointers). The symbol names and API constants are
  identical too. So the a64-win target's OS-content is essentially the x86 Win64 target's
  OS-content — **only the instruction encoding differs.** The worst Q1 leak does **not**
  worsen on the ISA axis; it is orthogonal.

**Summary:** the ISA axis's new leaks are I1/I2 (reach content/set is per-(ISA,OS),
duplicated per ISA) — the same *kind* of linear-in-targets growth Q1 found, now also
along ISA. Immediates (I3) and alignment (I4) are ISA facts that **stay in the lowerer**
(non-leaks), costing code size, not IR neutrality. Register pressure never forced the IR
either (naive stack-slot spills everything; a real allocator would only shrink code).

---

## ⑥ Cost of neutrality (aarch64 emit vs baseline)

Baseline = Q1's prior variant-B blobs (sysv64: pure 166, rhp 1128, spawn 856), the same
baseline Q1 used. This is a **cross-ISA reference** (no directly-compiled flat aarch64
baseline exists — same caveat as Q1's Win64 column).

| payload | baseline (B) | a64-linux | ratio | a64-win | ratio | (Q1 x86 sysv for ref) |
|---|--:|--:|--:|--:|--:|--:|
| pure_compute    | 166  | 192 | **116%** | 192 | **116%** | 281 (169%) |
| read_hash_print | 1128 | 660 | **59%**  | 728 | **65%**  | 1046 (93%) |
| spawn_echo      | 856  | 840 | **98%**  | 956 | **112%** | 1251 (146%) |

**No artifact trips the 200% ceiling** — and aarch64 is **smaller than the x86 emit**
from the same naive lowerer. Honest reason: the naive "spill every temp, materialize
every constant" style punishes x86's bulky encodings (a `mov r,imm64` is 10 bytes; a
disp32 load ~7–8), while aarch64's fixed 4-byte instructions stay compact even for the
extra `movz`/`movk` a big constant needs. So on the ISA axis the cost of neutrality is
**well within budget** and does not favor x86.

---

## Reproduce (third-party runnable)
```powershell
cd research/dynamic-core/isa
rustc --edition 2021 -O -A dead_code main.rs -o out/driver.exe
cd out; ./driver.exe        # validates encoder vs LLVM, lowers 3 payloads ×2 targets, dumps bins

# kernel per-ISA delta (④):
cd ..
rustc --edition 2021 -O kernel/textsize.rs -o out/textsize.exe
rustc --edition 2021 -O -C panic=abort --crate-type staticlib --emit obj --target x86_64-unknown-linux-gnu kernel/prim.rs -o out/prim.x86_64.o
rustc --edition 2021 -O -C panic=abort --crate-type staticlib --emit obj --target aarch64-unknown-linux-gnu kernel/prim.rs -o out/prim.aarch64.o
./out/textsize.exe out/prim.x86_64.o
./out/textsize.exe out/prim.aarch64.o
```
LLVM ground truth for the encoder was captured with
`rustc --emit obj --target aarch64-unknown-linux-gnu` on `global_asm!` probes; the 26
expected words are inlined in `main.rs::validate_encoder` and re-checked on every run.

---

## Deviations from the spec (there are always some)
1. **aarch64 byte-measured + encoder-validated, NOT executed** — no qemu/hardware; the
   spec cover explicitly permits this. `pure_compute` is identical across the two
   aarch64 targets but still not *executed* (both need an aarch64 CPU); its correctness
   rests on the LLVM-validated encoder + Q1's executed x86 structure.
2. **③ baseline is Q1's sysv64 blob (cross-ISA reference)** — no directly-compiled flat
   aarch64 baseline was produced (would need the aarch64 link toolchain + flat.ld); same
   deviation Q1 recorded for its Win64 column.
3. **`common_a64.rs` factors AAPCS64 placement into `place_args`** (per-ISA), whereas
   Q1's x86 kept placement inside each per-target file. This is not cosmetic — it is the
   ISA-axis finding I5 (aarch64's one calling convention), and it slightly shifts the
   LOC between the per-ISA and per-target buckets vs a naive file-by-file mapping. The
   ② table classifies by *role*, not by file, and notes the shift.
4. **Naive stack-slot lowerer (no register allocation)** — same as Q1; inflates ⑥,
   recorded rather than optimized away.
5. **`kernel/prim.rs` is a fresh minimal transcription of the four primitives** (not a
   `#[path]` include of `core/kernel.rs`, which is x86-cfg'd and would not compile for
   aarch64). Semantics match Q0's kernel; it exists solely to measure ④.
