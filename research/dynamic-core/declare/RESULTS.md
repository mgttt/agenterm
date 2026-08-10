# Q13 — Declare without a host layout oracle: bake-and-pray vs bake-and-detect — RESULTS

Decisive experiment for [`plan/archive/design-declare-detection-experiment.md`](../../../plan/archive/design-declare-detection-experiment.md).
Takes Q6's three irreducibly-baked Windows layout facts and asks: on a host that
publishes no struct layout, can a wrong offset be **detected (fail-fast)** instead of
**silently crashing**? Built and run on real Windows/x86_64 using only ③ `sym` + ④ `call`.
Clean-room; reuses Q6's four-primitive contract as the capability-under-test.

---

## Verdict — **只能烤，但可检测（bake-only, but detectable）**

On Windows you **still cannot query the true offset** — there is no runtime `offsetof`
oracle, so baking the constant is unavoidable (① below). **But a wrong bake is
DETECTABLE**: for every one of Q6's three layout facts, a ③+④-only self-check **fired on a
deliberately corrupted offset** on the real box (② below), turning Q6's silent per-target
trust into an explicit fail-fast. The cost is **~18–38 LOC/fact, entirely payload-side,
+0 kernel bytes** (③). This is the improvement the spec set out to find: **bake-and-pray →
bake-and-detect.** The residue (④) is the class of fields with **no semantic round-trip**
(write-only-unvalidated, unpredictable-read); there detection is impossible and bake-and-pray
survives — but it shrinks to exactly that set. And detection has a **strength gradient**:
constructive (FACT 1) and OS-round-trip (FACT 2) are airtight modulo naming; cross-field
(FACT 3) is **weak** — it bootstraps trust from *other* baked offsets, which this very run
demonstrated by catching a real 32-bit-vs-64-bit offset bug I had baked (see deviation 1).

Against Q4 (⑤): layout binding-truth **is** checkable where naming binding-truth is not, so
the Q4/Q6 trust set shrinks from **{naming + layout}** to **{naming}** — layout exits the
trust hole, *modulo* the symbols the self-check itself calls.

---

## Measurement conditions

| | |
|---|---|
| Host | Windows Server 2022 Datacenter 10.0.20348 (**real machine**), x86_64, 8 logical CPUs |
| Compiler | `rustc 1.97.0 (2d8144b78 2026-07-07)`, `-O` |
| Primitives | `sym`/`call` rewritten from scratch mirroring `core/kernel.rs` contract (clean-room, std harness) — **byte-identical in role to Q6's**; unchanged |
| Facts under test | Q6's three baked layout facts: `WIN32_FIND_DATAA.cFileName`, `sockaddr_in.sin_*`, `SYSTEM_INFO.dwPageSize` |
| Detection proof | per fact: correct offset must PASS **and** a corrupted offset must FIRE, on this box |
| ISA / other OS | x86_64 only; no Linux, no BTF, no PDB, no symbol server (spec §1.2 / time box) |

---

## ① Host oracle — **partial: VALUES yes, OFFSETS no, and the values are gated behind offsets**

`GetSystemInfo` (the only Windows "describe" oracle reachable via ③+④) publishes machine-fact
**values**, measured live:

| machine fact | value (this box) |
|---|--:|
| wProcessorArchitecture | 9 (AMD64) |
| dwPageSize | 4096 |
| dwAllocationGranularity | 65536 |
| dwNumberOfProcessors | 8 |

But every value is **gated behind a struct offset the host does NOT publish** (`@0/@4/@32/@40`).
So the oracle answers *values* only if you already know the *layout* to extract them — a
**bootstrap circularity**. A runtime **struct-field-offset oracle (`offsetof`) does not exist**
on Windows; obtaining one would require parsing PDB via `dbghelp`/a symbol server — the §1.2
pathology, excluded by design (it would make a 3 KB kernel a debug-infrastructure client).

**Reading:** the host oracle is *partial* — enough to answer machine facts, useless for struct
layout, and even its machine answers presuppose baked offsets. **Baking is unavoidable on
Windows.** The question therefore correctly shifts from "can we query?" to "can we detect?"

---

## ② Detection — THE MAIN JUDGEMENT (boolean gate) — **DETECTED for all 3 facts**

Each fact: run with the correct offset (must PASS) then with a corrupted offset (must FIRE).
Live driver output (`out/q13.exe`, real Windows):

```
[1] WIN32_FIND_DATAA.cFileName  (READ, baked @44)
    [PASS] correct @44: self-check corroborated the offset
    [FIRE] corrupted @52: cFileName@52 = "er.tmp", expected "q13_marker.tmp" (offset assumption WRONG)
[2] sockaddr_in.sin_family@0 / sin_addr@4  (WRITE, round-trip)
    [PASS] correct @0/@4: self-check corroborated the offset
    [FIRE] corrupted family@8: bind failed (WSAGetLastError=10047) — family/addr offset WRONG
[3] SYSTEM_INFO.dwPageSize  (READ, baked @4, cross-field)
    [PASS] correct @4: self-check corroborated the offset
    [FIRE] corrupted @0: dwPageSize@0 = 9 — not a plausible power-of-two page size (offset WRONG)
```

| fact | dir | detection mechanism (③+④ only) | corruption → signal | strength |
|---|---|---|---|---|
| **1** cFileName | READ | **constructive**: create `q13_marker.tmp`, FindFirstFile it, require the string at the assumed offset == the name *we chose* | @52 reads `"er.tmp"` ≠ `"q13_marker.tmp"` → value mismatch | **strong** (known in → known out) |
| **2** sockaddr_in | WRITE | **round-trip**: write fields at assumed offsets, `bind` 127.0.0.1:0, `getsockname` reads the OS's own view back | family@8 → `bind` rejects with **WSAEAFNOSUPPORT (10047)** | **strong** (OS validates + writes back) |
| **3** dwPageSize | READ | **cross-field**: anchor on wProcessorArchitecture@0==9 ∧ page@4 plausible pow2 ∧ granularity@40==65536 | page@0 reads `9` → not a plausible page size | **weak** (bootstraps from other baked offsets) |

**Boolean gate (spec §4 step 1): PASSES for all three** — every fact both passed on the
correct offset and fired on the corrupt one. The kill criterion ("detection fires for NO
fact") is **not** triggered. The mechanism is not "query the right value"; it is
**cross-reference the offset's effect against a fact the API's *semantic contract*
guarantees** (the filename you passed, AF_INET's meaning, AMD64's page size). The expected
values are themselves baked — but a baked *expected value + check* fails **loudly** where a
baked *offset* fails **silently**. Trust moves from "layout offset (silent when wrong)" to
"API semantic contract (loud when wrong)" — a strictly better place, because API contracts
are far more stable across platform versions than struct layout.

---

## ③ Cost — **~18–38 LOC/fact, payload-side, +0 kernel bytes**

Detection-layer LOC (non-comment/non-blank), measured from `main.rs`:

| self-check | LOC | note |
|---|--:|---|
| `dir_selfcheck` (FACT 1) | 25 | + `make_probe_file` 8 / `delete_probe_file` 3 (constructive setup) |
| `socket_selfcheck` (FACT 2) | 38 | includes bind + getsockname round-trip |
| `pagesize_selfcheck` (FACT 3) | 18 | cross-field |
| **marginal per fact** | **~18–38** | payload-side only |

**Kernel-in vs kernel-out split (the number the spec demanded):**
- **Kernel bytes added by detection = 0.** The four primitives are unchanged; every check is
  ordinary ③+④ payload code. Detection is *pure kernel-out*.
- Contrast Q6: promoting Declare into the kernel cost **+182 B .text** (avoidable → 0 if left
  as a baked table). Q6's 0-byte option is **bake-and-pray** (`kernel5.rs::LAYOUT_TABLE`, an
  offset with no check). Q13 keeps kernel bytes at **0** *and* adds detection, at ~27 LOC/fact
  of payload code. **So the honest cost comparison is: detection is free in the kernel and
  cheap in the payload; it buys fail-fast that the +0 B / +182 B Q6 forms both lacked.**

Marginal slope: +1 fact = +1 self-check (~18–38 LOC payload, 0 kernel bytes). No shared
detection engine is forced — each fact's check is bespoke to whichever API round-trips it,
which is also why some facts have *no* check (④).

---

## ④ Coverage boundary — the permanent residue (honest ceiling)

A fact is **detectable** iff some Win32 API exists whose **known-input → known-output**, or
whose **OS round-trip write-back**, touches the field — so a wrong offset yields an observable
value mismatch. All three tested facts qualify. The **detection-impossible residue** (permanent
bake-and-pray) is fields with **no such API**:

| residue class | example | why undetectable |
|---|---|---|
| write-only, OS-consumes-internally, no validation | an opaque flags/reserved field `bind`/`CreateProcess` ignores | no return signal, no read-back — a wrong offset is invisible |
| read, value unpredictable, round-tripped by nothing | `WIN32_FIND_DATAA.ftCreationTime` | you cannot pre-know the FILETIME to compare against |
| size / `cb` field, loosely validated | `STARTUPINFOA.cb=104` | a wrong-but-in-range size may be silently tolerated |

**Detection also has a strength gradient, not a flat yes:**
- **constructive** (FACT 1) — you choose the input, you know the exact output → airtight *modulo naming*.
- **round-trip** (FACT 2) — the OS validates and writes back → airtight *modulo naming*.
- **cross-field** (FACT 3) — **weak**: it has no external ground truth (page size 4096 is itself
  a baked expected constant) and its anchors are *other baked offsets*. It catches gross drift
  but bootstraps from the very thing it checks. **This run proved the weakness both ways:** the
  check caught a real bug — I had baked the **32-bit** offsets for `dwAllocationGranularity`
  (@28) and `dwNumberOfProcessors` (@20); on x64 they are @40/@32, and the cross-field check
  fired (`dwAllocationGranularity@28 = 0 != 65536`) on the first run (deviation 1).

**Boundary, stated plainly:** detection converts bake-and-pray → bake-and-detect for every
layout fact that participates in a semantic round-trip, which — empirically — is *most* fields
a capability actually uses (they exist to be passed to or returned from an API). It does
**nothing** for fields with no round-trip; that set is the irreducible silent-trust residue and
should be documented as such in the design.

---

## ⑤ Relation to Q4 — the trust hole narrows from {naming + layout} to {naming}

Q4/Q7-L1: *"is symbol index 1 really `CreateFileA`?"* is **unqueryable** — the only way to
test the call is to make it, which is circular (Thompson). Q6 named **two** irreducibly-baked
Windows classes: naming (L1) and layout (`offsetof`).

Q13 shows the **layout** class is different: *"is @44 really `cFileName`?"* **is** checkable,
because a wrong offset has an **observable consequence you can drive with known I/O** (write a
filename you chose, read it back at the offset). The asymmetry is exact: naming's consequence
(the call's effect) *is* the thing whose correctness you are trying to establish — no
independent probe exists; layout's consequence (a byte at an offset) can be cross-checked
against an API contract *without* first trusting the layout.

So the Q4/Q6 trust set **shrinks from {naming + layout} to {naming}** — one of Q6's two baked
classes **exits the trust hole**. The narrowing is **conditional**: the self-check itself calls
`FindFirstFileA`/`bind`/`GetSystemInfo`, so layout is verifiable **modulo naming trust**. If the
resolved symbols are trojaned, the read-back can be faked. Layout trust is thus *reduced to*
naming trust, not eliminated — but that is a real collapse of two holes into one, and it lands
on exactly the hole Q4 already declared irreducible.

---

## Decision trace (spec §4 tree, walked)

1. **② (main, boolean gate):** all three facts PASS-on-correct **and** FIRE-on-corrupt on the
   real box → detection exists for every tested fact → **not** the negative outcome; kill
   criterion not tripped. → continue.
2. **③:** detection layer = ~18–38 LOC/fact, **+0 kernel bytes** (payload-side), vs Q6's
   +182 B-or-bake-and-pray in-kernel forms.
3. **①:** host oracle is **partial** — machine-fact values published (gated behind unpublished
   offsets = bootstrap circularity); **no `offsetof` oracle** (PDB excluded). Baking unavoidable.
4. **④:** residue = fields with no semantic round-trip (write-only-unvalidated /
   unpredictable-read / loosely-validated size); plus a detection **strength gradient**
   (constructive/round-trip strong, cross-field weak).
5. **⑤:** trust set {naming+layout} → {naming}; layout exits, modulo naming.

**Verdict: 只能烤但可检测.** Windows offers no way to *query* the true offset (bake is forced),
but a wrong bake is *detectable* — silent crash converted to explicit fail-fast — for every
fact with a semantic round-trip, at 0 kernel bytes. Bake-and-pray → bake-and-detect, with a
named permanent residue.

---

## Reproduce (third-party runnable)

```powershell
cd research/dynamic-core/declare
mkdir out 2>$null
rustc --edition 2021 -O -A nonstandard_style -A dead_code -A unused_mut main.rs -o out/q13.exe
cd out ; ./q13.exe
```

The driver, on real Windows: creates a probe file, runs each of the three self-checks with the
correct offset (must print `[PASS]`) and with a corrupted offset (must print `[FIRE]`), prints
the ② per-fact DETECTED verdict, the ① host-oracle values, and the ④/⑤ boundary text, then
deletes the probe file. Deterministic across runs except the machine-fact values (CPU count).

---

## Deviations from the spec / honesty clause

1. **A real baked-offset bug was hit and caught mid-experiment** — the strongest single piece
   of evidence, so it is reported, not hidden. I initially baked the **32-bit** SYSTEM_INFO
   offsets (`dwAllocationGranularity@28`, `dwNumberOfProcessors@20`); on x64 those fields sit at
   @40/@32 because pointers/`DWORD_PTR` are 8 bytes. The cross-field self-check **fired on the
   first run** (`dwAllocationGranularity@28 = 0 != 65536`). I corrected the offsets to @40/@32.
   This is the experiment's thesis happening to its own author: a silent bake became a loud
   failure. It also grounds the ④ finding that cross-field detection is the **weak** tier.
2. **Corruption offsets are kept in-buffer** (e.g. cFileName @52, family @8) so the check fails
   by *value mismatch / API rejection*, not by an access violation. This models the realistic
   drift mode (a field moves a few bytes across a struct revision), not a wild pointer — and it
   is the harder case for detection (an AV would be trivially "detected" by crashing).
3. **FACT 3 detection is honestly labelled weak.** Page size has no layout-free independent
   oracle on Windows (`VirtualQuery` returns another struct = circular), so the check is
   cross-field corroboration whose anchors are themselves baked. Reported as-is, not dressed up.
4. **No metric was adjusted to flatter the result.** The verdict is the *middle* outcome
   ("bake-only but detectable"), not the flattering "we found a query oracle" — there is none —
   nor the defeatist "silent trust hole" — detection works. The permanent residue (④) and the
   modulo-naming caveat (⑤) are stated as limits, not engineered away.
5. **x86_64 / Windows only, four primitives only** — no PDB parser, no symbol server, no Linux
   (spec §1.2 pathology guard + time box). The four primitives are unchanged from Q6; detection
   is added purely as payload-side ③+④ usage.
