# Q10 — copy-and-patch stencil lowering: does it dissolve Q2's in/out-kernel tradeoff?

Spec + pinned criteria: [`plan/design-stencil-lowering-experiment.md`](../../../plan/design-stencil-lowering-experiment.md).
Clean-room: built from the public description of copy-and-patch only; **no prior
implementation (incl. CPython's) was read**. Parallel to and independent of Q9/Q6.

## What this is

Q2 measured a hand-written IR→x86_64 encoder at **X = 3003 B ≈ the whole kernel**, making
"lowerer in-kernel vs out-of-kernel" a **real no-free-lunch tradeoff** (in-kernel: smaller
total delivery; out-of-kernel: smaller frozen TCB). Q2 then flagged an untested
alternative: **copy-and-patch** — let a *real optimizing compiler* emit machine-code
templates (stencils) at BUILD time, so the runtime "lowerer" is only **memcpy + a
relocation applier**. The reviewer's hypothesis: X collapses to a few hundred bytes, the
ISA cost moves to stencil *data*, and the tradeoff **dissolves**.

**Q10 built the minimal viable copy-and-patch backend from scratch and measured it.**

## Verdict — **NOT worth it at this scale; the tradeoff is NOT dissolved.**

The *pure* "memcpy + relocation applier" **is** tiny, as predicted (**651 B**). But that
was never the expensive part. The **opcode decode/dispatch survives unchanged as code and
dominates**, and copy-and-patch adds a **per-ISA stencil table** on top. Net: the whole
runtime footprint is **5826 B ≈ 1.94× Q2's 3003 B** — *bigger*, not smaller. So X does
**not** drop to hundreds of bytes, the frozen-TCB argument does **not** flip, and the
in/out-kernel tradeoff is **shifted up at both endpoints, not dissolved**. This **refutes
the reviewer's expected value** — the Futamura first-projection win is real but small,
localized to the encoder sub-component (which was already cheap), and paid for with data.

**All three Q1/Q2 payloads execute byte-identically** through the stencil backend on the
real Windows host (`pure`→163, `rhp`→`a49d2cbecc13994f`, `spawn`→prints `exit=07`/exit 7),
so the numbers rest on a working mechanism, not a mock-up.

---

## Measurement conditions

| | |
|---|---|
| Host | Windows Server 2022 x86_64 (real). `rustc 1.97.0`, `-O -C panic=abort -C debuginfo=0` |
| Real compiler for stencils | `rustc -O2` (LLVM), `--target x86_64-unknown-linux-gnu --emit=obj -C relocation-model=static` → ELF object → holes = `R_X86_64_PC32` |
| IR | Q2's 24-opcode register-machine bytecode, **byte-identical, unchanged** (`ir.rs`) |
| Payloads | Q1/Q2's three, **unchanged** (`ir_gen.rs` reused verbatim) |
| Byte口径 | flat-PIC blob subtraction, **identical to Q2** (`-min-jump-table-entries=200`, `relocation-model=pic`, ELF, `flat.ld`) so X_total is directly comparable to Q2's 3003 B; plus per-function `llvm-size` on the object for the code/data split |
| Register model | **memory register file** (vreg id = its slot); no physical-register allocation, nothing carried in registers across stencils |

---

## ① X's new value — the main criterion, measured three ways

| level | Q10 (copy-and-patch) | Q2 (hand encoder) | ratio |
|---|--:|--:|--:|
| **memcpy + relocation applier only** (`place`+`patch`) | **651 B** | — (fused into emit) | — |
| **whole runtime lowerer, code** (`emit`+`place`+`patch`+`lower_and_run`) | **4515 B** | 2883 B | **1.57×** |
| **whole runtime footprint, code+data** (flat subtraction, Q2口径) | **5826 B** | **3003 B** | **1.94×** |

Object section split (precise, `llvm-size -A`):

```
Q10 applier:  emit 3541 | place 586 | patch 65 | lower_and_run 323   = 4515 B code
              + 20 Stencil structs (32 B ea) + code/hole rodata      ≈ 1210 B data
Q2 lowerer:   emit 2709 | lower_and_run 174                          = 2883 B code
```

**Reading:** the reviewer's hypothesis was right about the *applier* (651 B — genuinely a
few hundred bytes) but wrong about the *whole runtime*. The `emit` opcode
decode/dispatch — which BOTH designs must have — is the dominant term, and copy-and-patch
does not touch it. Q10's `emit` (3541 B) is even a bit larger than Q2's entire `emit`
(2709 B, decode+encoders fused) because each arm builds an operand context and calls the
generic `place()`. **Even generously trimming Q10's decode to Q2's 2709 B** leaves
`2709 + place 586 + patch 65 + stencil data 571 = 3931 B > 3003 B` — the "bigger" verdict
is robust to that implementation artifact.

Concrete in-kernel exes (PE-aligned, this host) confirm the trend:

```
payload   Q10 stencil    Q2 hand-enc    delta
pure         10752          7680        +3072
rhp          11264          7680        +3584
spawn        11776          7680        +4096
```

## ② Where the cost went — and what it scales with

- **Stencil data:** 20 stencils, **406 code bytes + 55 PC32 holes**. Tight encoding
  (`u16 off + u8 kind` per hole) = **571 B**; Rust-materialized (`&[u8]`/`&[Hole]` fat
  refs, 32 B struct headers) ≈ **1210 B**.
- **Scales with `opcode-count × ISA-count`** (the *product*), same multiplicative
  structure as any per-ISA lowerer — the whole table duplicates per ISA. Plus a
  per-`argc` variant factor on `CALL` (call0..call4): **structural arity cannot be a
  hole**, so it forces variant explosion. Control-flow ops are *not* stencils at all
  (see ④), so they don't add to the table but do add to applier code.
- **Total (code+data) vs Q2's X:** **bigger** (5826 vs 3003). Per the spec's own test,
  "if the total is bigger, it is not dissolving the tradeoff — and here it is worse than
  merely moving bytes from code to data: it *added* net bytes, because the dominant
  decode/dispatch never moved to data at all."

## ③ Is the tradeoff dissolved? — **No.**

Recomputed in Q2's口径:

| | Q2 in-kernel | Q2 out-of-kernel | Q10 in-kernel | Q10 out-of-kernel |
|---|--:|--:|--:|--:|
| runtime lowerer/applier footprint | 3003 B | 3003 B (as blob) | **5826 B** | **5826 B (as blob)** |
| frozen TCB | ~6.2 KB | **~2.93 KB** | **~8.7 KB** | ~2.93 KB |

Copy-and-patch **raises both endpoints** and preserves the tradeoff's shape: in-kernel
still wins total delivery, out-of-kernel still wins frozen TCB. Because X **grew** rather
than shrank, the "X is negligible → obviously in-kernel" collapse the reviewer hoped for
**does not trigger** — the frozen-TCB penalty of going in-kernel is *worse* than Q2's, not
better. **Decision: the tradeoff is not dissolved; copy-and-patch is a net loss vs Q2's
hand-written lowerer on size, at both placements.**

## ④ Cost & boundary — what a real compiler at build time buys, and what won't stencilize

- **Build-time compiler dependency.** Stencils are emitted by `rustc -O2` (LLVM) at build
  time; terminal users run no compiler (confirmed — the shipped artifact is data +
  applier). **Cross-compiler reproducibility is by *contract*, not by *bytes*:** the hole
  set is defined by symbol name (stable), but the exact machine bytes are whatever your
  build compiler chose — a different compiler/version yields different (still-correct)
  bytes. You ship *your* compiler's output. Who/when: the toolchain author, once per
  `(opcode-variant × ISA × ABI)`.
- **Control flow will NOT stencilize (prediction CONFIRMED, with a precise reason).** A
  compiled stencil cannot **leave CPU flags live across its boundary**, and a branch
  target is a **code offset resolved during layout** (not a value the compiler can
  pre-bake). So `JMP`/`Jcc`/`LABEL` are emitted by the applier as ~20 fixed bytes +
  a rel32 back-patch — a **residual encoder** copy-and-patch cannot remove. *Nuance the
  naive prediction missed:* with a **memory** register file, the *data-flow* side of
  branching (reconciling register state at join points — the thing that makes CPython's
  register-carrying stencils hard) is **trivial**, because all state is in memory. Control
  flow is hard **only** at the flags/target-resolution seam, not generally.
- **`CALL` arity will not stencilize** — one stencil per `argc` (structural, not a hole).
- **The ±2 GB placement constraint is REAL and was hit** (reference §7.4). `R_X86_64_PC32`
  holes silently truncate beyond ±2 GB; the applier had to co-locate code / regfile /
  const-pool / a *copy of the env table* in **one arena** (only the code sub-range flipped
  to RX) so every rip-relative hole stays in reach. A "give me N bytes" memory primitive
  that doesn't guarantee proximity **would fail silently**, exactly as the reference warns.

## ⑤ Relation to Q5's three-way split — form change, not a saving

Q5: +1 ISA = **+~307 LOC lowerer**. Under copy-and-patch, +1 ISA =
**a new stencil table (DATA)** + **a new relocation applier** (PC32 for x86;
`CALL26`/`ADRP+ADD` for aarch64 — reference: ~8–15 reloc kinds, ~200–400 B each) +
**a new control-flow byte-emitter** (the ④ residual is ISA-specific raw bytes). So
copy-and-patch **converts the per-ISA *encoder* into per-ISA *data*** (the stencils —
which §5 of the reference notes are **distribution-time strippable**, the one genuine
architectural merit), but leaves an **irreducible per-ISA code residual** (reloc kinds +
control-flow emit). It is a **shift toward data**, consistent with this track's
"mechanism-fixed, capability-via-data" principle — **but not a size win**: at this scale
the data + surviving decode make the total *larger* (①), and it does not make adding an
ISA free.

**Cross-Q closer:** the memcpy+patch engine (`place`/`patch`) has **zero `match opcode`**
(verified) — the data-flow codegen genuinely became data, echoing Q7's "+1 intent = 0
marshaller code." But adding a data-flow opcode still costs **+1 decode arm of code**;
fully datafying the decode *is* building an interpreter — which is **Q9** (whole
interpreter **3177 B**, no RWX, no stencil table, no ±2 GB constraint). Copy-and-patch
sits *between* Q2 (all code) and Q9 (all data/interpret) and, on size, is **worse than
both** here: bigger than Q2's compiler *and* bigger than Q9's interpreter.

---

## Deviations from the spec / honest notes

1. **Q10's `emit` is mildly inflated** by an operand-context struct passed to the generic
   `place()` (an implementation choice, not fundamental). Quantified in ①; the verdict is
   shown robust to it. Not tuned away, per the no-metric-tuning discipline.
2. **X split reported at three levels** (applier-core / whole-code / code+data) rather than
   one number, because the single "X" the reviewer imagined (memcpy+applier) turned out to
   be only the *smallest* of three and would overstate the win if reported alone.
3. **All three payloads executed on Windows**; Linux/SysV byte-measured only (no WSL) —
   same posture as Q1/Q2. The stencil bytes are host-agnostic x86_64; the holes are
   patched to real (Windows) runtime addresses.
4. **No second ISA built** (time-box; ⑤ is a projection from the reference's reloc-kind
   analysis, not a two-point measurement — evidence strength noted).
5. **Only the opcodes the three payloads use** were stencilized (20 stencils). No float,
   no full opcode coverage — per the time-box.
6. **No fifth primitive added.** The applier is a pure user of ① (`mem_alloc`/
   `mem_protect`) and ② (jump), like Q2's.

## Reproduce

```powershell
pwsh research/dynamic-core/stencil/build/build_stencil.ps1
cd research/dynamic-core/stencil/out
[IO.File]::WriteAllText("$PWD\input.txt","dynamic-core experiment 2026-08-08`n")
.\A_st_pure_windows.exe;  $LASTEXITCODE     # 163
.\A_st_rhp_windows.exe                       # a49d2cbecc13994f
.\A_st_spawn_windows.exe; $LASTEXITCODE      # prints exit=07, exits 7
```

The script also rebuilds the stencil object with `rustc -O2`, regenerates
`out/stencils_gen.rs` via `stencilize.exe` (the build tool that parses the ELF holes),
and prints the flat X_total and the `llvm-size` code/data split. Independent reference
hash (FNV-1a/64 of the 35-byte input) = `a49d2cbecc13994f` — same as Q0/Q1/Q2, proving the
stencil-lowered payload is not co-wrong with the kernel.
