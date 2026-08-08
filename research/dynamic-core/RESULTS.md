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
- **Net: the layer count is not decided by the metric the whole argument rested on.**
  1-layer wins on raw size by a hair (and by more once §1.3's upper-bound caveat is
  applied); 2-layer wins on TCB and evolvability. This is a genuine "the decisive
  criterion turned out non-decisive" outcome — reported as-is.

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

### ④ +1 capability marginal cost

**NOT MEASURED — stopped at ③ per the §4.4 time-box** ("到量出③为止，不做④"). Design
data point only: in variant B a new capability = a new payload blob against the *unchanged*
kernel (out-of-kernel only); in variant A it re-links a new whole binary. Left for a
follow-up run.

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

**Verdict as written by §4: marginally 1-layer, on the ② tiebreak — but the primary
axis ③ is a true tie, so the choice is effectively a values call:** minimal size
(1-layer) vs bounded TCB + clean version coexistence (2-layer). The experiment did **not**
find the runaway in-kernel growth that would have condemned 1-layer, nor a ③ advantage
that would have vindicated 2-layer.

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
4. **④ `call` handles the integer/pointer-word subset only** (≤7 args, no float/struct/
   by-value return) — all the file adapters need. A full libffi-style descriptor is out
   of scope. This is the honest minimal ④.
5. **`memcpy/memset/memmove/memcmp`** are provided by the kernel (no libc). They are part
   of the freestanding floor and count toward ①.
6. **No fifth primitive was needed.** The four primitives (①–④) sufficed for both OSes
   and both payloads; the §1.1 "urge to add a 5th" never arose. Recorded as a finding.
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
```

Independent reference hash (FNV-1a/64 of the 35-byte input) = `a49d2cbecc13994f`
(computed in Python: offset basis `0xcbf29ce484222325`, prime `0x100000001b3`).

Sizes are also emitted by each build script's final `ls`/`Get-ChildItem` step.
