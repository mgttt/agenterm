# Dynamic-core experiment — RESULTS

Decisive experiment for `plan/design-dynamic-core-experiment.md`: **does the dynamic
core want 1 layer or 2?** Measured, not argued. Clean-room; no prior implementation
was consulted.

---

## TL;DR verdict

- **③ (the decisive metric) does NOT distinguish the two variants.**
  Adding a second OS (Linux → Linux+Windows) grows the **already-shipped OS binary by
  0 bytes** in *both* variants, because all Windows code sits behind `cfg` gates that
  are textually excluded from the Linux build (verified byte-identical). The kernel
  never grows to *add a capability*; it only carries raw reachability for each OS.
  Neither variant is disqualified by kill-criterion §4.1.
- Because ③ ties, §4 falls through to **② (total delivery)**, where **1-layer is
  marginally smaller or equal** (Linux: 3104 vs 3496 B; Windows: 4608 vs 4608 B).
- **⑤ (TCB) and ⑥ (coexistence) favor 2-layer** — but §4 only consults them if ②
  ties, which it doesn't.
- **④ (marginal cost of +1 capability) — now measured (follow-up run) — is the first
  criterion that does NOT tie.** Adding a second capability (spawn a subprocess):
  a program that does *not* use it grows by **0** in 2-layer (new capability = a separate
  blob file; the file-only payload blob is byte-identical) but by **+~0.4 KB per
  capability** in a true single-product 1-layer (`A_fused − A_rhp` = +432 B Linux / +512 B
  Windows). ④ is a **slope** criterion, which §3 ranks above the ② intercept — so on the
  experiment's own priority it **tips the balance toward 2-layer**. Caveat surfaced: the
  spawn capability needed `CreateProcessA` (10 args) and the ④ `call` primitive shipped
  with a 7-arg ceiling, so adding the capability **forced a one-time in-kernel change**
  (`call` 7→11 arms, +208 B Linux / +512 B Windows) that grew *both* variants' kernels —
  no 5th primitive, but ④ was not shipped at full generality. See §④.
- **Net: ③ ties; ② marginally favors 1-layer on raw size; ④/⑤/⑥ favor 2-layer.** By §4's
  literal tree (which lists only ③→②→⑤/⑥) the verdict stays "hair's-breadth 1-layer"; by
  §3's stated "slope > intercept" priority, the now-measured ④ slope moves it to 2-layer.
  The decisive-by-design metric (③) was non-decisive; the metric the prior round deferred
  (④) turned out to be the one that separates them. Reported as-is.

---

## Measurement conditions (so the numbers are comparable)

| | |
|---|---|
| Compiler | `rustc 1.97.0 (2d8144b78 2026-07-07)`, bundled `rust-lld` |
| Language | Rust, `#![no_std] #![no_main]`, **no libc / no CRT / no runtime** |
| ISA | x86_64 only (per §2) |
| Linux target | `x86_64-unknown-linux-gnu`, **cross-compiled from Windows** (no C toolchain; static ELF, no libc) |
| Windows target | `x86_64-pc-windows-msvc` (native PE, `/nodefaultlib`, no CRT) |
| Common flags | `-O -C panic=abort -C debuginfo=0` |
| Linux extra | `-C force-unwind-tables=no`; linked `--strip-all -static` (stripped release) |
| Windows extra | MSVC target mandates unwind tables (cannot disable); `/DEBUG:NONE` (no PDB) |
| Blobs (variant B) | compiled `-C relocation-model=pic`, flattened with `ld.lld --oformat binary` |
| Byte counts | strip-equivalent release artifacts; exact flags live in `build/build_linux.sh` and `build/build_windows.ps1` |

**Execution status:** Windows artifacts were **built and run** (results verified, below).
Linux artifacts were **built and byte-measured but not executed** — the host has no WSL
distribution installed, so Linux binaries cannot run here. The variant-B load-and-jump
mechanism was independently proven on Windows (a sysv64 flat blob is loaded via
`VirtualAlloc`/`VirtualProtect` and entered correctly). Linux binaries are valid static
ELF with entry code at the expected offsets. This reverses the spec's suggested order
(Linux-first) to **Windows-executed / Linux-cross-measured**, as the cover note permits.

---

## The six criteria

Two payloads: **pure_compute** (floor, touches no OS) and **read_hash_print** (reads a
file, FNV-1a/64, prints hex — exercises syscall on Linux, GetProcAddress+FFI on Windows).
Two variants: **A = 1 layer** (static link), **B = 2 layer** (frozen loader + flat PIC
payload blob). All sizes in bytes.

### Raw artifact sizes

> These are the **Phase-2 (pre-spawn) numbers**, so ①②③⑤ below stay directly comparable
> with the prior run. The spawn capability (§④) shifts every kernel-bearing artifact by a
> fixed +208 B (Linux) / +512 B (Windows) via the one-time ④ `call` extension; §④ reports
> those deltas and the new spawn artifacts separately.

| artifact | Linux | Windows |
|---|--:|--:|
| A_pure (1L, pure_compute) | 2512 | 3584 |
| A_rhp  (1L, read_hash_print) | 3104 | 4608 |
| B_kernel_pure (2L, kernel+pure blob) | 2904 | 3584 |
| B_kernel_rhp  (2L, kernel+rhp blob) | 3496 | 4608 |
| blob_pure (2L payload only) | 166 | 166 |
| blob_rhp  (2L payload only) | 761 | 1128 |
| **B kernel-only** (binary − blob, ~constant) | **~2738** | **~3418–3480** |

### ① Floor — bytes to run pure_compute

| | Linux | Windows |
|---|--:|--:|
| **A (1 layer)** | **2512** | **3584** |
| **B (2 layer)** = kernel + pure blob | 2904 | 3584 |

1-layer floor is lower on Linux (392 B) and equal on Windows (PE 512/4096-B alignment
rounds both to 3584). The 2-layer floor carries the loader + embedded blob.

### ② Total delivery — bytes to run read_hash_print

| | Linux | Windows |
|---|--:|--:|
| **A (1 layer)** | **3104** | **4608** |
| **B (2 layer)** | 3496 | 4608 |

1-layer ≤ 2-layer (Linux −392 B; Windows tie by alignment). Per §1.3, variant A here
*also* routes through the primitive table, so **A is an upper bound** on true 1-layer
cost — a real 1-layer could inline the primitives and be smaller still. So 1-layer's ②
edge is real and would only widen.

### ③ +1 OS marginal cost (Linux-only → Linux+Windows) — THE DECISIVE METRIC

Split into in-kernel (`core/`) vs out-of-kernel (`adapters/`, `pack/`), bytes and lines.

**Byte growth of the already-shipped OS binary:**

> **0 bytes, both variants, in-kernel and out-of-kernel.**
> Immediately after adding the Windows code (before any unrelated edits) the Linux
> artifacts were **byte-identical** to the Linux-only baseline
> (2512 / 3200 / 2904 / 3480 / 166 / 745). All Windows mechanism lives behind
> `#[cfg(windows)]` / `#[cfg(target_os="linux")]` and is excluded from Linux codegen.
> This is the "flat slope" the thesis wanted, and it holds for **both** layer counts.

**New bytes shipped for the second OS** (you build one binary per machine — §0):

| new Windows artifact | bytes | note |
|---|--:|---|
| A_pure / A_rhp (1L) | 3584 / 4608 | a whole new per-OS binary |
| B kernel (2L, reusable) | ~3418 | **built once, serves every Windows payload** |
| B blob_pure / blob_rhp (2L) | 166 / 1128 | per-payload, OS-specific adapter |

Crucially the Windows kernel is the **same six-primitive kernel** as Linux's, with the
Windows *reach* mechanism (GetProcAddress+FFI) instead of Linux's (raw syscall). It does
**not** grow to add a file abstraction — file I/O lives in the adapter/payload, not the
kernel. So per §4.1 ("in-kernel bytes grow with OS count → judged loser") **neither
variant is disqualified**: in-kernel byte growth to an existing OS is 0, and each new
OS's kernel is a bounded, semantics-free constant.

**Source lines to add Windows** (`git diff` baseline→+Windows, excludes build scripts & docs):

| location | +lines / −lines | attribution |
|---|--:|---|
| `core/kernel.rs` (**in-kernel**) | +123 / −2 | ~106 are the Windows primitive block + 2 Windows entry points; ~15 are `#[cfg]` guards/comments added to existing Linux fns |
| `adapters/windows/readfile.rs` (**out-of-kernel**, new) | +55 / −0 | the entire Windows file adapter (GetProcAddress+FFI) |
| `pack/*` adapter selection (**out-of-kernel**) | +8 / −0 | 4 lines × 2 crate roots (`cfg(dc_os=…)` adapter pick) |

The in-kernel line cost is **identical for both variants** — they share one
`core/kernel.rs`. So ③ (byte growth *and* line cost) **ties between A and B.**

### ④ +1 capability marginal cost — MEASURED (follow-up run, 2026-08-08)

> The prior run stopped at ③ per the §4.4 time-box. This follow-up measures ④ only.
> Second capability = **spawn a child process, wait, report its exit code**
> (Linux `fork`/`execve`/`wait4` via ③ raw_syscall; Windows `CreateProcessA` +
> `WaitForSingleObject` + `GetExitCodeProcess` via ③ sym + ④ call). New payload
> `spawn_echo` (prints `exit=07`, exits 7) plus the file-only control (`read_hash_print`)
> re-measured after the capability set grew.
>
> **Execution:** Windows built **and run** — `A_spawn`, `B_kernel_spawn` both print
> `exit=07` and exit 7; `A_fused` runs the file path and prints the correct hash; the
> existing `A_pure`/`A_rhp`/`B_kernel_rhp` still pass (no regression). Linux
> cross-compiled and byte-measured only (no WSL), same as the prior round.

The task's demand: report **(a) the bytes of the new capability itself** and **(b) how
much a program that does NOT use it grows** — because (b) is what layering is supposed to
buy. All file sizes in bytes; Linux is unpadded (real code deltas visible), Windows is
PE-aligned to 512 (deltas round up to a block).

**Baseline (pre-spawn, 7-arg `call`) → after adding spawn (11-arg `call`):**

| artifact | Linux base | Linux now | Δ | Win base | Win now | Δ |
|---|--:|--:|--:|--:|--:|--:|
| A_pure (1L, **non-user**) | 2512 | 2720 | **+208** | 3584 | 4096 | **+512** |
| A_rhp  (1L, **non-user**) | 3104 | 3312 | **+208** | 4608 | 5120 | **+512** |
| B_kernel_pure (2L) | 2904 | 3112 | **+208** | 3584 | 4096 | **+512** |
| B_kernel_rhp  (2L, **non-user**) | 3496 | 3704 | **+208** | 4608 | 5120 | **+512** |
| blob_pure (2L payload) | 166 | 166 | **0** | 166 | 166 | **0** |
| blob_rhp  (2L payload, **non-user**) | 761 | 761 | **0** | 1128 | 1128 | **0** |
| A_spawn (1L, new) | — | 2992 | — | — | 4608 | — |
| A_fused (1L, file+spawn in one) | — | 3744 | — | — | 5632 | — |
| blob_spawn (2L, new) | — | 439 | — | — | 856 | — |

#### (a) The new capability itself

| | Linux | Windows |
|---|--:|--:|
| **2-layer**: `blob_spawn` — a standalone loadable file, loaded only by spawners | **439** | **856** |
| **1-layer standalone** `A_spawn` marginal over the floor (`A_spawn − A_pure`) | **272** | **512** |

The capability (spawn adapter + `spawn_echo` payload) is ~0.4–0.9 KB of code. In 2-layer
it is one more blob file; in 1-layer it is (in the per-payload model) one more whole binary.

#### (b) Growth of a program that does NOT use the new capability

This decomposes into **two independent components**, and they point different ways:

**(b1) — capability *adapter* carried by a non-user.**

| model | non-user growth | how measured |
|---|--:|---|
| **1-layer, true single product** (all capabilities in one artifact, runtime dispatch) | **+432 (L) / +512 (W)** | `A_fused − A_rhp`: a file-only run still ships the spawn adapter+payload |
| **1-layer, per-payload** (one binary per program, dead-code-strips unused caps) | **0** | `A_rhp` never links `spawn.rs`; unchanged whether or not the capability exists |
| **2-layer** | **0** | `blob_rhp` byte-identical (761/1128); spawn ships as a *separate* `blob_spawn` |

> **This is the first criterion where the variants measurably diverge in the predicted
> direction.** A true single-product 1-layer forces every user to carry every capability's
> adapter — a **linear** term (+~0.4 KB per capability, on every program). 2-layer's
> non-user growth is **0** by construction (new capability = new blob file). 1-layer can
> match that 0 *only* by giving up the "single product" form and compiling one binary per
> program — which then duplicates the whole ~2.7 KB kernel into every binary (the ⑥
> "coexistence by duplication" cost). **2-layer is the only variant that isolates a new
> capability from non-users *and* shares one kernel.**

**(b2) — the shared kernel grew to admit the capability (an honest, unexpected cost).**

The spawn capability needed `CreateProcessA` — **10 arguments** — but the ④ `call`
primitive shipped with a **7-arg ceiling** (register-only arms). Adding the capability
therefore **forced an in-kernel change**: the `call` arm table was raised 7 → 11. That
code lives in `core/kernel.rs`, which **both variants embed**, so **every kernel-bearing
binary grew** — non-users included — by **+208 B (Linux) / +512 B (Windows)**:

- 1-layer: every `A_*` grew +208/+512 (they all link `call` via `native_table`).
- 2-layer: the **"frozen" kernel** grew +208/+512 (`B_kernel_*`) — it was not, in fact,
  frozen against this capability. The payload **blobs** are insulated (0), because blobs
  do not embed the kernel.

> This qualifies the §1.1 completeness claim. **No fifth primitive was needed** — but ④
> was not *shipped* at its full libffi generality (it was arm-limited to 7). The second
> capability exposed that and forced completing ④. This is a **one-time step**, not a
> per-capability slope: once ④ handles ≤11 args, further capabilities in that arity cost
> **0** in-kernel. A properly variadic (asm, stack-arg) ④ would have absorbed
> `CreateProcessA` from day one at zero incremental in-kernel cost. Recorded, not hidden
> (per §1.1's explicit instruction).

**Source lines for the capability** (`git diff`/`wc -l`, excludes build scripts & docs):

| location | lines | in/out kernel |
|---|--:|---|
| `core/kernel.rs` — ④ `call` 7→11 arms, both OS + note | **+38 / −4** | **in-kernel** (shared; identical for both variants) |
| `adapters/linux/spawn.rs` (fork/execve/wait4) | +70 | out-of-kernel |
| `adapters/windows/spawn.rs` (CreateProcessA+wait) | +89 | out-of-kernel |
| `payloads/spawn_echo/logic.rs` | +20 | out-of-kernel |
| `pack/variant_a_onelayer/spawn_echo.rs` | +25 | out-of-kernel (pack) |
| `pack/variant_b_twolayer/payload_spawn.rs` | +26 | out-of-kernel (pack) |
| `pack/variant_a_onelayer/fused.rs` (measurement harness for b1) | +60 | out-of-kernel (not part of the capability) |

#### ④ verdict — does it change the prior judgment?

Prior run: ③ tied → fell through to ② (total delivery) → **marginally 1-layer** on the
size intercept, with ⑤/⑥ favoring 2-layer but uncounted because ② didn't tie.

④ is the **first criterion that actually distinguishes the two variants**, and it is a
**slope** criterion — which §3 explicitly ranks *above* the ② intercept ("一个今天小但
线性增长的设计，输给一个今天略大但持平的设计…所以 ③④ 比 ①② 重要"):

- On **(b1)**, 2-layer isolates each new capability from non-users at **0** cost while
  sharing one kernel. A true single-product 1-layer pays a **linear +~0.4 KB per
  capability on every program**; a per-payload 1-layer matches 0 only by duplicating the
  kernel. **(b1) favors 2-layer** and is exactly the "increasing slope" the whole thesis
  rests on.
- On **(b2)**, adding the capability grew the **shared kernel** in *both* variants
  (+208/+512) — a cost neither escapes — but it is a **one-time** completion of ④, not a
  per-capability slope.

**Two honest readings:**

1. **By §4's literal decision tree** — which names only ③ → ② → ⑤/⑥ and never lists ④ as
   a node — the verdict is unchanged: marginally 1-layer on the ② tiebreak.
2. **By §3's stated priority** (slopes ③④ outrank intercepts ①②), ④ is a slope, it
   discriminates, and it **favors 2-layer**. Under the experiment's own ranking, ④ tips
   the balance the *opposite* way from the ② intercept that gave 1-layer its hair-thin
   edge. Combined with ⑤ (TCB) and ⑥ (coexistence), which also favor 2-layer, the only
   remaining 1-layer advantage is the raw ② byte intercept.

**Net:** ④ did **not** tie (unlike ③). It is the criterion the prior round correctly
guessed would be "the only one still able to separate the variants." It separates them in
favor of **2-layer** for capability isolation — while also surfacing that "adding a
capability" can still touch the shared kernel when it stresses a primitive's generality
(a one-time cost, both variants). Reported as measured, not adjusted.

### ⑤ TCB — bytes that must be trusted/verified

| | Linux | Windows |
|---|--:|--:|
| **A (1 layer)** = whole binary, **grows with every payload** | 2512–3104 | 3584–4608 |
| **B (2 layer)** = kernel only, **fixed regardless of payload** | **~2738** | **~3418–3480** |

**Favors 2-layer.** B's trusted base is a single frozen loader (~2.7 KB Linux / ~3.4 KB
Windows) no matter what payload runs; A's trusted base is the entire product and expands
with each payload/capability.

### ⑥ Coexistence — can two incompatible versions of the adapter package coexist?

| | answer | kind |
|---|:--:|---|
| **A (1 layer)** | **YES** | trivially — each program is a self-contained static binary (full duplication, not really a shared "library") |
| **B (2 layer)** | **YES** | meaningfully — two payload/adapter blob **files** coexist and are loaded per-process by the same frozen kernel; no global singleton, no forced version unification |

Neither exhibits the JVM-style runtime failure (a global singleton forcing one version).
B demonstrates the *library* property (shared frozen kernel + independently-versioned
payloads); A achieves coexistence only by duplicating everything.

---

## §4 decision trace (rules fixed before building)

1. **③ decisive?** No. In-kernel byte growth to an existing OS = 0 for both; per-OS
   kernel is bounded and semantics-free; in-kernel line cost is identical (shared kernel).
   **③ ties. Neither disqualified.**
2. **③ tied → look at ② (total delivery).** 1-layer ≤ 2-layer (Linux −392 B, Windows
   tie). By the letter of §4, this points to **1-layer**.
3. (⑤/⑥ are only consulted if ② ties — it doesn't. They favor 2-layer and are recorded
   above for the record.)
4. **④ measured (follow-up).** §4's tree never lists ④ as a node, but §3 ranks slopes
   ③④ *above* the ② intercept. ④ does **not** tie: a non-user of the new capability grows
   **0** in 2-layer vs **+~0.4 KB/capability** in a true single-product 1-layer (`A_fused
   − A_rhp`). By §3's own priority this **outranks** the ② tiebreak and points to 2-layer.
   (One-time caveat: the capability forced completing ④'s arg ceiling 7→11, +208/+512 B in
   the shared kernel of *both* variants — see §④.)

**Verdict — two honest readings:**
- **By §4's literal tree** (③→②→⑤/⑥, ④ absent): marginally **1-layer** on the ② byte
  tiebreak — ③ is a true tie, so it's effectively a values call (minimal size vs bounded
  TCB + coexistence).
- **By §3's stated priority** (slopes ③④ > intercepts ①②): ③ ties, but the now-measured
  **④ slope favors 2-layer** (0 non-user growth per capability, without duplicating the
  kernel), joining ⑤ and ⑥. 1-layer's only remaining edge is the raw ② intercept. On the
  experiment's own ranking, the balance tips to **2-layer**.

The experiment found **no** runaway in-kernel growth condemning 1-layer, and **no** ③
advantage vindicating 2-layer — but ④ (the metric the prior round deferred) is the one
that separates them, in 2-layer's favor.

---

## Deviations from the spec (there are always some)

1. **Order reversed to Windows-first-executed, Linux-cross-measured.** No WSL distro on
   the host → Linux binaries are built and byte-measured but not run. Windows binaries
   are built and executed (verified). Both OSes are covered and ③ is measurable, which
   is what the cover note said matters.
2. **Variant A also routes through the primitive table** (§1.3's known bias, restated):
   A is an **upper bound** on 1-layer cost, B is exact. A true 1-layer would be ≤ the
   A numbers here.
3. **Variant B payload blob uses a uniform `sysv64` ABI on both OSes**, compiled for the
   ELF target and flattened with `ld.lld --oformat binary` (PE flat-extraction was
   unreliable). The kernel's ④ `call` primitive bridges `sysv64 → win64` when invoking
   OS functions. Proven correct on Windows. Caveat: sysv64 has a 128-B red zone Windows
   does not; for these short leaf payloads it caused no issue, but a hardened build should
   disable the red zone.
4. **④ `call` handles the integer/pointer-word subset only** (no float/struct/by-value
   return). It shipped with a **7-arg ceiling**; the spawn follow-up (§④) raised it to
   **11 args** to admit `CreateProcessA` (10 args). Still a register/stack-word subset, not
   a full libffi descriptor. See §④ (b2) for the in-kernel cost this imposed on both variants.
5. **`memcpy/memset/memmove/memcmp`** are provided by the kernel (no libc). They are part
   of the freestanding floor and count toward ①.
6. **No fifth primitive was needed — but ④ was not shipped at full generality.** The four
   primitive *kinds* (①–④) sufficed for both OSes and all three payloads; the §1.1 "urge to
   add a 5th" never arose. However, the second capability (§④) forced *completing* the
   existing ④ (arg ceiling 7→11) — a one-time in-kernel cost, not a new primitive kind.
   Recorded honestly per §1.1.
8. **`spawn_echo` (§④) is executed on Windows, byte-measured only on Linux.** `A_spawn`
   and `B_kernel_spawn` print `exit=07` and exit 7 (child = `cmd.exe /c exit 7`); the
   10-arg `CreateProcessA` call through the extended ④ works. Linux uses fork/execve/wait4
   via ③ and is not run here (no WSL); its byte counts are still valid.
7. **Payload buffer via primitive ①.** A flat, RX-mapped blob cannot use static `.bss`
   (not writable), so `read_hash_print` requests its buffer from `mem_alloc`. This is
   more faithful to the model (payload uses ① for memory) and unifies A and B logic. It
   also surfaced a real 2-layer constraint worth recording.

---

## Reproduce (third-party runnable)

```sh
# Linux artifacts (cross-compiled from any host with a Rust toolchain + llvm-tools):
rustup target add x86_64-unknown-linux-gnu
bash research/dynamic-core/build/build_linux.sh        # prints sizes into out/

# Windows artifacts (on Windows, MSVC target; also needs llvm-tools for the ELF blobs):
rustup component add llvm-tools
pwsh research/dynamic-core/build/build_windows.ps1      # prints sizes into out/
```

Verify correctness (Windows):

```powershell
cd research/dynamic-core/out
[IO.File]::WriteAllText("$PWD\input.txt","dynamic-core experiment 2026-08-08`n")
.\A_rhp_windows.exe                 # -> a49d2cbecc13994f
.\B_kernel_rhp_windows.exe          # -> a49d2cbecc13994f  (identical => mechanism correct)
.\A_pure_windows.exe; $LASTEXITCODE # -> 163
.\B_kernel_pure_windows.exe; $LASTEXITCODE # -> 163
# criterion ④ — spawn a child, wait, report its exit code:
.\A_spawn_windows.exe; $LASTEXITCODE        # -> prints "exit=07", exit code 7
.\B_kernel_spawn_windows.exe; $LASTEXITCODE # -> prints "exit=07", exit code 7  (2-layer)
.\A_fused_windows.exe                        # -> a49d2cbecc13994f (file+spawn in one product; runs file path)
```

Independent reference hash (FNV-1a/64 of the 35-byte input) = `a49d2cbecc13994f`
(computed in Python: offset basis `0xcbf29ce484222325`, prime `0x100000001b3`).

Sizes are also emitted by each build script's final `ls`/`Get-ChildItem` step.
