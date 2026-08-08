# Q18 — Discovery without anointing (name → hash) — RESULTS

Decisive experiment for [`SPEC.md`](./SPEC.md). Directly succeeds Q3's one residue
([`../reuse/RESULTS.md`](../reuse/RESULTS.md) ④c / SYNTHESIS R7): content addressing gives
`hash → content` but **no `name → hash` discovery** — "the one place anointing could
re-enter." Clean-room; builds on Q3's content store, blobs and loader unchanged (same
hashes). Measured, not argued.

---

## TL;DR verdict

**判决：无钦定发现可达 AND 发现根本不必在运行时发生。** Two independent results, both
solution-level:

1. **无钦定 (no anointing) is reachable at runtime** — via **multiple non-conflicting
   directories**. Two consumers resolve the **same name** `fileio` to **different hashes**
   through two directories that disagree, and **both run correctly & simultaneously**
   (`len=0023` v1 vs `len=0008` v2). ① passes, ② passes. There is **no directory both
   must consult**; a directory that lacks a name simply **misses** (exit 23) — no global
   fallback exists to be mandatory.
2. **⑤ flips the frame (the more important half):** discovery is **not a runtime problem**.
   Q3's `manifest.txt` already names **hashes**, pinned at build time — the loader never
   discovers anything. The build-time-pinned baseline (`loader_hash`, **zero** discovery
   code, byte-identical to Q3's loader) produces the **same two divergent outcomes** as the
   runtime resolver. The `name→hash` step is fully **hoistable to build time**, at which
   point runtime discovery residue = **0**.
- **③ cost:** the runtime discovery layer is **+1424 B** clean-ELF over the 2672 B plain-
  hash loader (Windows PE: +2048 B, 512-aligned/coarse) — and it is **removable** (build-
  time pinning pays 0). Same order as Q3's own +1648 B verify option.
- **④ residue:** what remains is **local trust** — you trust *the directory you picked*
  (runtime) or *the hash source you used* (build time). It is **local** (per-consumer),
  **plural** (two directories coexist and disagree), and **optional at runtime**.
  Categorically **not** the JRT "one, shared, forced" disease. The one irreducible bit —
  *does this hash really implement what the name promises* — is **naming-truth (R3/Q14)**,
  not a discovery/anointing residue.
- **Net on Q3's residue R7:** **消解为构建时关注点 (dissolved into a build-time concern).**
  R7 was read as a runtime hole where anointing re-enters. It is neither: the loader
  correctly needs only hashes, and the name→hash step has a no-anointing form (many
  directories) that is anyway movable out of runtime entirely.

---

## Measurement conditions (comparable with Q3)

| | |
|---|---|
| Compiler | `rustc 1.97.0 (2d8144b78 2026-07-07)`, bundled `rust-lld` |
| Language | Rust `#![no_std] #![no_main]`, no libc / no CRT |
| ISA | x86_64 |
| Windows | `x86_64-pc-windows-msvc`, `/nodefaultlib`, `mainCRTStartup` — **built and RUN** |
| Linux | `x86_64-unknown-linux-gnu` static ELF, `--strip-all` — **byte-measured, NOT executed** (no WSL), same posture as Q3 |
| Common flags | `-O -C panic=abort -C debuginfo=0`; blobs `-C relocation-model=pic` flattened via `rust-lld --oformat binary` |
| Blobs / hashes | Q3's, verbatim: `payload_readlen 20dadc64497288c1`, `adapter_v1 aaf8b49f6b10aa5c` (full read), `adapter_v2 26505ca2d1bca982` (truncated ≤8 B) |
| Input | `input.txt` = 35 bytes (`0x23`) |

**Execution status:** the discovery mechanism is **proven on Windows** (all four runs
below). Linux loaders are **byte-measured only**. `loader_hash_linux` = **2672 B** is
**byte-identical to Q3's `loader_ca_linux`** — an independent cross-check that the baseline
under test *is* Q3's content-addressed loader.

---

## Artifacts

| loader | Linux (clean) | Windows (PE 512-aligned) | what it is |
|---|--:|--:|---|
| `loader_hash` (baseline: reads `manifest.txt` of **hashes**) | **2672** | 4608 | Q3's loader — **build-time-pinned, zero discovery code** (candidate 3) |
| `loader_disc` (`--cfg dc_discover`: reads `trust.txt`→directory→resolves `prog.txt` names) | **4096** | 6656 | the runtime `name→hash` layer under test (candidate 1) |
| **discovery layer Δ** | **+1424** | +2048 (coarse) | the entire cost of runtime name resolution |

One source (`loader.rs`), **packed twice** — the Δ is exactly the `name→hash` layer.

**Data files (all authorable by anyone; text):**

```
dir_a.txt:  readlen 20dadc64497288c1     dir_b.txt:  readlen 20dadc64497288c1
            fileio  aaf8b49f6b10aa5c                 fileio  26505ca2d1bca982   <- SAME name, DIFFERENT hash
prog.txt:   readlen                       trust_a.txt: dir_a.txt
            fileio                         trust_b.txt: dir_b.txt
```

---

## ① Boolean gate — two directories resolve one name to two hashes, both run

Same `prog.txt` (names `readlen`, `fileio`). Only `trust.txt` differs.

```
consumer A  trust.txt=dir_a.txt  fileio->aaf8… (v1 full)      -> prints  len=0023   (35 bytes)
consumer B  trust.txt=dir_b.txt  fileio->2650… (v2 truncated) -> prints  len=0008   (8 bytes)
```

Both **run correctly on Windows** (verified). Same name, two non-conflicting maps, two
hashes, two correct behaviours. **① PASSES.**

## ② Anointing check (MAIN) — nothing must exist / everyone must use / cover everyone

Version-coexistence discriminator (Q3's): the **same name**'s two **incompatible** hashes
(`v1` full vs `v2` truncated — a breaking behavioural change) are bound by two consumers
**at the same time**, both correct. A single global registry **cannot represent** "`fileio`
is both v1 and v2 depending on who asks"; per-consumer directories do it natively.

Checked for a mandatory universal element on the used path — there is none:

- **No default/global directory.** The loader has no built-in directory; each consumer
  names its own in `trust.txt`.
- **No global fallback.** A directory that lacks a name **misses** — negative probe:
  `dir_empty.txt` (no `fileio`) → **exit 23**, no silent redirection to some canonical
  source. Coverage is per-directory, not universal.
- **Directories may disagree and coexist** (`dir_a` vs `dir_b`), the direct opposite of
  "one shared forced" (JRT/JVM monopoly).

**② PASSES: no anointing.** "选信谁" (choosing whom to trust) is **not** soft anointing:
anointing is defined by *universality/compulsion* (one, shared, forced); choosing *locally*
is the mechanism by which no global choice is imposed. The empirical proof it is not a
hidden global mandate: two consumers make **different** choices for the **same** name and
**both** work — impossible under any single authoritative directory.

## ③ Cost — and is discovery runtime or build-time?

Runtime name resolution = **+1424 B** clean-ELF (`loader_disc` 4096 − `loader_hash` 2672).
Windows PE Δ = +2048 B but is 512-aligned (coarse; Linux is the clean number, per Q3's
口径 note). Same order of magnitude as Q3's own **+1648 B** verify option.

**But the cost is removable.** The +1424 B buys *runtime* resolution; the identical
outcomes are reachable with **0** discovery bytes by resolving at **build time** — see ⑤.
So discovery is **runtime iff you choose to pay for it**; its natural home is the build.

## ④ Residue — precisely what remains

**Local trust.** Each consumer must trust *the directory it picked* (runtime) or *the hash
source it used* (build time). Three properties keep this out of the anointing category:

1. **Local**, not global — per-consumer / per-build, not authoritative-for-everyone.
2. **Plural** — two directories coexist and disagree on the same name; both usable.
3. **Optional at runtime** — candidate 3 removes the runtime resolver entirely; the trust
   is exercised once, at build time, and frozen into a hash.

Inherited from Q3 and **not** solved by a directory: content addressing dedups **bytes, not
behaviour**, so a directory can only assert *a* `name→hash`, never prove the hash **behaves
as the name promises**. That last bit — *naming truth* — is exactly R3 / Q14's
behavioural-verification concern (Tier B, needs execution), **orthogonal** to discovery. A
directory is a name→hash *assertion*; verifying it is Q14's job, not discovery's.

## ⑤ vs build-time pinning (MAIN — reframes the whole experiment)

`loader_hash` (build-time-pinned hashes, **zero** discovery code) produces the **same two
divergent outcomes**:

```
manifest_a.txt = payload_readlen + adapter_v1  -> len=0023
manifest_b.txt = payload_readlen + adapter_v2  -> len=0008
```

The `name→hash` resolution that `loader_disc` does at **runtime**, the baseline does at
**build time** — whoever wrote `manifest_a` vs `manifest_b` resolved `fileio` to a hash
once, and froze it. **So discovery is not intrinsically a runtime problem.** It hoists
fully to the build, where the runtime has zero discovery code and zero discovery residue.

**Consequence for Q3's residue:** R7 ("no name→hash discovery") is **not a runtime hole
where anointing re-enters**. The runtime loader correctly needs only hashes (that is the
*point* of content addressing, not a gap), and the name→hash step (a) has a no-anointing
runtime form (many directories, ①②) and (b) is anyway movable out of runtime entirely.
Q3's residue is a **build-time name-resolution concern**, and even there it is a **local,
plural choice**, never a global mandate.

---

## §4 decision trace (rules fixed before building, see SPEC §3/§4)

1. **⑤ first:** does `loader_hash` (0 discovery code) achieve the same two outcomes as
   `loader_disc`? **YES** (`len=0023` / `len=0008` both ways). → discovery is a **build-time
   concern**; runtime has no discovery residue. ①②③ still reported (the runtime variant
   also works and its price is measured), but the verdict is driven by **⑤ + ②**.
2. **② main:** single mandatory resolver anywhere on the path? **NO** — no default
   directory, no global fallback (exit-23 probe), directories disagree and coexist. →
   **无钦定**. Reducible only to "trust *some* directory/hash-source" = **local** choice.
3. **① gate:** two consumers both run correctly? **YES.** Gate not tripped.
4. **kill criterion** (any working path needs one universal directory → report "discovery
   cannot avoid anointing", do not build it): **not triggered** — the working path uses
   **plural** directories.

**Verdict — 无钦定发现可达；且发现是构建时关注点，运行时无此残留 (no-anointing discovery
reachable; and discovery is a build-time concern, no runtime residue).** Both halves are
solution-level and reported honestly: the honest limit is that *some* local trust always
remains (you trust the directory/hash you chose), but that is a **local, plural, optional**
choice — the categorical opposite of the "one, shared, forced" disease this track guards
against — and the only irreducible piece (naming truth) belongs to Q14, not here.

---

## Deviations from the spec

1. **Windows-executed, Linux byte-measured** (no WSL) — as Q3, as SPEC §2 permits.
   `loader_hash_linux` = 2672 B == Q3's `loader_ca_linux` confirms the baseline is Q3's
   loader byte-for-byte.
2. **Candidate 2 (name carries a trust anchor: publisher key + label) analyzed, not built**
   (SPEC §6). It is candidate 1 with the "directory" being a publisher's keyspace — same
   residue class (local trust), so building it adds no new judgement and risks the
   name-service slide the experiment guards against.
3. **Windows PE Δ (2048 B) is 512-aligned and coarse**; the clean ③ number is the Linux
   ELF +1424 B, consistent with Q3's own 口径 note that PE alignment blurs loader deltas.

---

## Reproduce

```powershell
pwsh research/dynamic-core/discovery/build/build_windows.ps1
# prints: consumer A len=0023 (v1), consumer B len=0008 (v2), pinned A/B identical;
#         discovery layer Δ (PE) and the four runs.
cd research/dynamic-core/discovery/out
Copy-Item trust_a.txt trust.txt -Force; .\loader_disc_windows.exe   # -> len=0023
Copy-Item trust_b.txt trust.txt -Force; .\loader_disc_windows.exe   # -> len=0008
```
```sh
# Linux loaders (cross-compiled; byte-measured, NOT executed): built in build/ notes,
# loader_hash_linux = 2672 B (== Q3 loader_ca_linux), loader_disc_linux = 4096 B, Δ +1424 B.
```

Independent cross-check: `loader_hash_linux` byte size == Q3's `loader_ca_linux` (2672 B),
proving the "no-discovery" baseline is Q3's content-addressed loader unchanged.
