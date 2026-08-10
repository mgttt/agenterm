# Q6 — Primitive completeness: is the four-primitive floor stable? — RESULTS

Decisive experiment for [`plan/archive/design-primitive-completeness-experiment.md`](../../../plan/archive/design-primitive-completeness-experiment.md).
Adds three capabilities of divergent arg-count/shape (memory-mapped file, directory
traversal, socket bind) using **only** Q0's four primitives (③ sym + ④ call + ①② memory),
on real Windows/x86_64. Clean-room; reuses only the `core/` four-primitive **contract**.

---

## Verdict — **地板部分蠕变：内核尺寸稳定，但 §1.1 的"没有够不到的东西"被证伪**

Two different "floors" give **opposite** answers, and separating them is the whole result:

1. **The kernel floor (bytes, arity, primitive count) is STABLE.** Three capabilities spanning
   arity 2→7 did **not** force ④'s ceiling (11) up — the Q0 7→11 was a **one-time step, not a
   slope** (① measured, not argued). Socket/mmap/dir added **0 kernel bytes**: they are just
   more ③+④ calls. **Claim K of §1.1 — "内核永远没有为覆盖某能力而变大" — HOLDS.**

2. **The completeness *claim* is FALSIFIED.** §1.1 also asserts "③+④ 够到平台上任何已存在的东西
   … 没有够不到的东西" (**Claim R**). It is false for one class: **struct-field layout
   (`offsetof`)**. Two of the three capabilities need a field offset; **nothing in ①②③④
   produces one**. The offset is reachable only by (a) **baking a constant** the payload/adapter
   carries as unverified per-target trust, or (b) where the host publishes machine-readable
   layout (Linux BTF), fetching it via ③+④ + a payload parser. On Windows system structs there
   is **no such publication**, so the fact is **irreducibly baked**. The portability work is
   **transferred, not eliminated** — the same shape as §0's "禁止封装 转移了可移植性而非消灭它",
   now located at the layout-description layer.

**So is a fifth primitive necessary?** The honest, two-reading answer (both stated, per the
skill's honesty clause):

- **Mechanically: NO.** Declare's operations — querying page size (`GetSystemInfo`), publishing
  unwind info (`RtlAddFunctionTable`), reading a BTF blob — are **ordinary ③+④ calls**. Four
  primitives suffice as *mechanisms*. What is missing is not a kernel entry point but a **host
  facility** (published metadata); where it is absent, no primitive count fixes it — you bake.
- **Conceptually: YES.** "Declare" names a distinct completeness-concern (bidirectional metadata:
  publish code facts + query platform facts) that a **"do-only" four-verb model makes structurally
  invisible** — which is exactly why layout constants keep getting silently baked and break across
  platform versions (`struct stat` 32/64-bit, glibc `_TIME_BITS`). The completeness *argument*
  cannot be closed without either naming this concern or admitting layout is baked trust.

**The closed list can be given, and it is FIVE — conditionally.** ① memory ② execute ③ reach
④ call ⑤ declare. Its completeness holds **iff the host publishes the descriptions**; absent that,
⑤ degenerates to a baked table and the closure is "complete modulo trusted layout data." This is
**not** "无法给出封闭清单" — the list closes — but its closure carries a host-conditional asterisk.

This **confirms Q7's L3a finding by direct construction** (layout needs query + host oracle) and
**refutes the specific fear of Q0 §④(b2)**: the arg-ceiling was a step, not a hidden per-capability
slope in the kernel.

---

## Measurement conditions

| | |
|---|---|
| Host | Windows Server 2022 Datacenter 10.0.20348 (**real machine**), x86_64 |
| Compiler | `rustc 1.97.0 (2d8144b78 2026-07-07)`, `-O` |
| Primitives | `sym`/`call` rewritten from scratch mirroring `core/kernel.rs` contract (clean-room, std harness) |
| Capabilities | mmap-file, dir-traversal, socket-bind — **all executed** on Windows |
| Byte floor | `--emit=obj --crate-type=lib` `.text` section sum via `llvm-objdump -h` |
| ISA / other OS | x86_64 only; Linux analyzed (BTF path), not executed (no WSL) — full-track posture |

---

## ① ④ arity ceiling — the MAIN judgement (slope) — **STEP, not slope**

Every native call actually issued, arity recorded by the harness (not asserted):

| capability | native calls (arity) | max |
|---|---|--:|
| A mmap-file | CreateFileA(7), CreateFileMappingA(6), MapViewOfFile(5), UnmapViewOfFile(1), CloseHandle(1) | **7** |
| B dir-traversal | FindFirstFileA(2), FindNextFileA(2)×N, FindClose(1) | **2** |
| C socket-bind | WSAStartup(2), socket(3), bind(3), closesocket(1), WSACleanup(0) | **3** |
| declare(describe) | GetSystemInfo(1) | 1 |

**MAX native-call arity across all capabilities = 7.** ④'s ceiling is **11**.
**Forced past 11 by any capability? NO.** → The Q0 `CreateProcessA` 7→11 raise was a
**one-time completion toward the libffi model, not a per-capability slope in the kernel.**
Boolean gate (spec §4.1): **not tripped** — ④ is not a hidden fifth growth source.

> Honest caveat: this holds for the **integer/pointer word subset**. Shapes ④ still cannot
> express at ANY arity — float/SIMD args, struct-by-value, varargs, sret — were **out of scope**
> (spec §2, §7) and remain ④'s known boundary (Q0 deviation 4). None of the three chosen
> capabilities needed them; a `printf`/varargs or a by-value-struct syscall would hit that wall,
> which is a **shape** limit of ④, not an **arity** slope.

---

## ② Is Declare necessary, or transferable? — **transferable-to-baked; the fact itself is unreachable**

Each layout fact the three capabilities touch, classified — and **all ran** with baked offsets:

| capability | layout fact | direction | classification |
|---|---|---|---|
| A mmap-file | none | — | **N/A** — MapViewOfFile returns a raw byte pointer; pure ④ |
| B dir-traversal | `WIN32_FIND_DATAA.cFileName` @44 | read | **baked constant** (unverified per-target trust) |
| C socket-bind | `sockaddr_in.{sin_family@0, sin_port@2, sin_addr@4}` | write | **baked constants** (unverified per-target trust) |
| declare(describe) | `SYSTEM_INFO.dwPageSize` @4 | read | **baked even to read the host's OWN answer** |

**Result:** four primitives + baked offsets **executed all three** (dir listed 5 entries incl.
`harness.exe`; mmap FNV-1a/64 of first 64 B of `main.rs` = `1b521fe4fb6a2a11`; UDP socket bound
to 127.0.0.1:0; page size = 4096). **Claim K holds — the kernel did not grow.**

But **nothing in ①②③④ produced any offset.** `sym` resolves a *symbol → address*; it cannot
answer `offsetof`, and no number of `sym` calls can. The offset entered as a **baked literal**.
Judgement:

- **Declare is NOT strictly necessary to RUN** on a target you hand-measured — the layout is
  bakeable (this is precisely what a hand-written adapter, and Q7's stub oracle, already do).
- **Declare IS necessary to be correct-by-construction across targets you did NOT hand-measure.**
  A baked `@44`/`@2` is silent trust: it is right until the platform's struct changes and then it
  is a silently-wrong-offset landmine, not an error. Declare (query the host) makes it
  self-correcting — **exactly Q7's "query form + host oracle."**
- The gap is therefore a **transfer**, not an elimination: portability work moves from the kernel
  into per-target baked data (payload/adapter) or into a host that publishes layout. **Claim R
  ("nothing unreachable") is falsified for the layout class** — `offsetof` is reachable by
  *nothing* here; it is only *bakeable*.

---

## ③ If Declare is added, how big is the floor? — **+182 B .text (in-kernel), and avoidable**

`kernel4.rs` (four primitives) vs `kernel5.rs` (four + `declare`), identical otherwise.
`.text` section sum (`llvm-objdump -h`, `-O`, `--crate-type=lib --emit=obj`):

| kernel | `.text` (bytes) | Δ |
|---|--:|---|
| kernel4 (four primitives) | **550** | baseline |
| kernel5 (+ declare) | **732** | — |
| **Δ declare** | **+182 B .text** (+ ~44 B rodata layout/switch table) | **+33% over kernel4** |

> **口径 (2026-08-08 audit) — the Δ is clean, one cross-experiment percentage was not.**
> Both numbers come from **one file pair, one command, one target** (`-O
> --crate-type=lib --emit=obj`, std harness, msvc, `llvm-objdump -h` `.text` sum), so
> **550 → 732 = +182 B = +33%** is a like-for-like delta. **Withdrawn:** this row also
> read "**≈+28% over Q5's 644 B**". That divides a Δ measured on *this* 550 B baseline by
> *another experiment's* 644 B — Q5's `prim.rs` on `aarch64-unknown-linux-gnu`, `no_std`
> + `panic=abort` + `--crate-type staticlib`. Cross-target, cross-build, cross-file: the
> percentage is meaningless and is deleted rather than repaired. **Δ is only ever divided
> by its own baseline** ([`../COMPARABILITY.md`](../COMPARABILITY.md) §2 U6, §6 R-P). The
> 550 ≈ 568 "same order" observation below is kept as an *order-of-magnitude sanity note*,
> which is all it ever was — **not** a comparable pair.

**But this +182 B is OPTIONAL and avoidable.** `declare`'s three operations are all
expressible via the existing ③+④:
- Describe-machine = `sym`+`call` `GetSystemInfo` (host-answerable).
- Publish = `sym`+`call` `RtlAddFunctionTable`.
- Query-layout = a table lookup — and the **table is baked trust either way**.

So the in-kernel byte cost of Declare is **0 if you leave it as a ③+④ usage pattern + a
payload-side baked table**, or **+182 B if you promote it to a uniform in-kernel query channel**.
The floor creep in *bytes* is real but **you choose whether to pay it in the kernel or outside it**
— the fact never becomes free; it is baked trust wherever it lives.

**Is there a SIXTH class?** Re-running ①'s method (add more capability shapes, look for a fact
reachable by nothing): the remaining hard cases are **(a) orchestration / control flow** (Q7's
L3b: `fork`+branch, multi-call dataflow) — but that is **payload-side generated code, not a kernel
primitive** (the payload's own ①②-produced code branches); and **(b) callbacks / function pointers
into generated code** (`EnumWindows`, thread start routine) — **covered by ①②④** (the callee is a
word address ② already made executable). Publish (unwind/ENDBR/i-cache) folds into ⑤. **No genuine
sixth *kernel* class surfaced.** The list closes at five.

---

## ④ Revised completeness argument

**Original (§1.1):** "①+② produce & run new code; ③+④ reach anything existing; therefore nothing
is unreachable and the kernel never grows to cover a capability."

**What it gets right (kept):** ①+② (produce/run) and ③+④ (reach-address/invoke) are mechanically
complete for *doing*, including invoking publish-APIs and reading metadata sources — those are more
③+④ calls, and they add **0 kernel bytes**. The "no kernel growth per capability" law survives (①).

**What it gets wrong (fixed):** "reach anything existing" silently conflates two things —
reaching an **address** (③ `dlsym`, real) and reaching a **description** (`offsetof`, `sizeof`,
ABI variant, page size, field encoding). A description is neither code-you-run nor an
address-you-call; **no "do" verb yields a description.** Layout is therefore reachable only if the
host *publishes* it (then ③+④ fetch it — no new primitive) or is *baked* as trusted per-target data
(the burden transferred, not covered).

**Revised closed list (FIVE, host-conditional):**

| # | primitive | nature |
|---|---|---|
| ① | memory | do (reserve/commit/protect; Q8: exec-memory conditional) |
| ② | execute | do (hand control to bytes; Q8: three implementations incl. interpret) |
| ③ | reach | do (symbol resolution + raw syscall) |
| ④ | call | do (data-described invoke, integer/pointer subset) |
| ⑤ | **declare** | **describe/publish** — query the host for layout/ABI/machine facts; publish generated-code facts. **Mechanically a ③+④ usage pattern; conceptually a distinct concern the four "do" verbs make invisible.** |

**New completeness argument:** ①+② give self-extension; ③+④ give reach-and-invoke over addresses;
**⑤ gives reach over *descriptions*.** The three groups close the space **iff every description a
payload needs is either host-published (⑤ queries it) or hand-baked (⑤ carries it as trusted data).**
Where a host publishes no machine-readable layout, closure holds only **modulo baked trust** — that
residue is irreducible and is the honest price of "no encapsulation." This is a **closed list with a
host-conditional asterisk**, not an open-ended one, and not "no closed list can be given."

**Relation to the 30–50-primitive convergence (Drawbridge/Gramine/WASI):** those counts are the
price of a host interface that *encapsulates semantics* (open, spawn, socket as first-class host
calls). Our five stays at five precisely because it refuses that — semantics live in the payload via
③+④, and the only thing that must additionally cross the boundary is **descriptions** (⑤). Five is
reachable **only** in the "kernel = FFI + describe channel" shape; the moment the kernel starts
answering `open()` itself, it is on the 30–50 road. Q6 does not walk that road; it names the fifth
concern and stops.

---

## Decision trace (spec §4 tree, walked)

1. **① boolean gate (main, slope):** max arity 7 ≤ 11 → ④ not forced up → **one-time step holds**,
   ④ is not a hidden kernel slope. Kill criterion **not** tripped. → continue.
2. **② boolean gate:** four primitives + baked offsets **ran** B and C (Claim K holds), **but**
   `offsetof` is produced by no primitive → a layout fact is unreachable-by-primitive → **Claim R
   falsified**. Declare judged **transferable-to-baked (necessary conceptually, not mechanically).**
3. **③:** in-kernel Declare floor = **+182 B .text** (avoidable → 0 if left as ③+④ + baked table).
   No sixth kernel class found (orchestration is payload code; callbacks are ①②④).
4. **④:** revised list = **five, host-conditional**; new completeness argument given.

**Verdict: 地板部分蠕变 — 内核尺寸/arity/原语数在 KERNEL 意义上稳定；§1.1 的完备性*claim*（没有够不到
的东西）被证伪，缺口是"描述类"(layout)，由载荷侧烘焙常量(信任转移)承担。封闭清单 = 五条，带宿主条件星号。**

---

## Reproduce (third-party runnable)

```powershell
cd research/dynamic-core/primitives
mkdir out 2>$null ; copy main.rs out\
rustc --edition 2021 -O -A nonstandard_style main.rs -o out/harness.exe
cd out ; ./harness.exe          # runs A/B/C on real Windows, prints ① arity + ② classification

# ③ byte floor:
$BIN = "$(rustc --print sysroot)/lib/rustlib/x86_64-pc-windows-msvc/bin"
rustc --edition 2021 -O --crate-type=lib --emit=obj -A warnings kernel4.rs -o out/kernel4.o
rustc --edition 2021 -O --crate-type=lib --emit=obj -A warnings kernel5.rs -o out/kernel5.o
& "$BIN/llvm-objdump.exe" -h out/kernel4.o   # sum .text -> 550
& "$BIN/llvm-objdump.exe" -h out/kernel5.o   # sum .text -> 732  (declare = +182 B)
```

**Independent check:** mmap FNV-1a/64 of the first 64 bytes of `main.rs` is content-dependent
(printed by the harness); the arity table and the byte delta are deterministic across runs.

---

## Deviations from the spec

1. **`sym`/`call` rewritten for a std harness**, not `#[path]`-reused from `core/kernel.rs` (that
   file is `no_std` with its own `_start`). Same four-primitive contract; the ④ `call` match arms
   and 11-ceiling are identical to `core/kernel.rs`. Emit-fidelity is not claimed; **arity and
   layout-reachability are the measured facts and are ABI-independent.**
2. **Linux BTF path analyzed, not executed** (no WSL) — spec §2 posture. The claim "where a host
   publishes layout, ③+④ can fetch it" is argued from CO-RE/BTF, not run here.
3. **③ byte floor is a Δ measured by object `.text`**, not the same flat-blob strip build as
   the **568 B / 644 B** four-primitive numbers (spec §3 permits this, explicitly labelled).
   **Two corrections from the 2026-08-08 口径 audit:** (a) those 568 / 644 B are **Q5's
   `isa/kernel/prim.rs`** — a fresh minimal transcription of the four primitives, `no_std` +
   `panic=abort` on `*-unknown-linux-gnu` — **not Q0's kernel**, which this file previously
   said (Q0's own kernel artifact is a ~2.7 KB whole stripped ELF, a third 口径 again);
   (b) kernel4's 550 B landing "in the same order" as 568 B is an **order-of-magnitude
   sanity note only** — it does **not** make the two numbers divisible, and the
   cross-experiment percentage that leaned on it has been withdrawn (see ③). The +182 B
   delta stands on its own baseline (550 → 732) and needs no outside number.
4. **Publish half (⑤a) is a stub** (`RtlAddFunctionTable` call shape), not a full unwind
   registration — spec §7 defers it; it is enough to show Publish is a ③+④ call, not a new mechanism.
5. **socket bind uses port 0 / loopback**, no remote traffic — enough to force writing
   `sockaddr_in` fields (the layout fact) without a network peer.

**Honesty clause:** no metric was adjusted to flatter the result. The finding is deliberately the
*less* tidy one — **the pretty "four primitives" number survives for the kernel, but the
completeness *argument* does not** — and both the mechanical-no and conceptual-yes readings of
"is Declare a primitive" are reported rather than collapsing to whichever looks cleaner. I did not
manufacture a fifth-primitive necessity: ④'s arity was measured stable, and Declare is shown to be
a ③+④ usage pattern whose only irreducible residue is baked layout trust.
