# Adapter-reuse experiment — RESULTS

Decisive experiment for `plan/design-adapter-reuse-experiment.md` (the track's Q3 —
the user prompt labeled it "Q5"; same question, same `reuse/` directory): **can an
adapter package be *reused* — not merely coexist — without a central registry
anointing an official one?** Measured, not argued. Clean-room; no prior implementation
was consulted.

---

## TL;DR verdict

- **① (the decisive gate) PASSES.** With two independent payloads (`read_hash_print`
  and `read_len`) both needing the file adapter, the content-addressed store holds the
  adapter **once** (`store/aaf8…bin`, 552 B Win / 232 B Linux) while the baked baseline
  carries it **twice** (once inside each payload blob). CA total **< baseline** at N=2
  and the gap **widens linearly** with the number of file-reading payloads. Reuse is
  real, in bytes.
- **② YES — reuse *and* coexistence, the property we wanted.** Two incompatible adapter
  versions (`v1` full read vs `v2` truncated-8-byte read) live in the store under
  different hashes; `rhp` binds `v1`, `read_len` binds `v2`; **both run correctly and
  simultaneously** (`a49d2cbecc13994f`, `len=0023` vs `len=0008`) with **no anointing,
  no negotiation, no conflict.** A name-based scheme ("fileio") would have forced one to
  win or demanded a version solver.
- **③ price is bounded and, crucially, O(1) in adapter count.** The content-addressed
  loader costs **+609 B in-kernel** over the Q0 embed loader (Linux, clean number),
  **+1648 B** more if it *verifies* content on load (an FNV-1a/64 hash pulled into the
  TCB; a real SHA-256 would be larger). Adding a third/fourth adapter adds **zero**
  loader code — so the mechanism itself does **not** grow outward. Passes kill-criterion.
- **④ the honest boundary.** Content addressing dedups **identical bytes, not equivalent
  behavior.** Confirmed both directions with real hashes: the same source compiled at two
  opt levels produced the **same** hash (`aaf8…` — build determinism makes dedup work
  across independent builders), but a behaviorally-equivalent *different* implementation
  (loop-read) produced a **different** hash (`4b78…` — dedup fails). And content
  addressing gives integrity/dedup/coexistence but **no discovery**: you cannot *find*
  "the file adapter" — you can only fetch a hash you already hold.
- **Net: 复用可达，有一条发现边界 (reuse achievable, bounded by discovery).** The
  content-addressing hypothesis **largely holds**: it delivers reuse (①), coexistence
  without anointing (②), at a constant, non-growing cost (③). Fragmentation is **not
  "unsolved"** — it is converted from *"N incompatible copies that cannot be shared"*
  into *"byte-identical copies auto-share; divergent copies coexist harmlessly."* The
  part that remains unsolved is **discovery** (mapping a name/intent → the right hash),
  which is exactly where "anointing" would re-enter — and which this experiment
  deliberately does not build (§7).

---

## Measurement conditions (comparable with Q0)

| | |
|---|---|
| Compiler | `rustc 1.97.0 (2d8144b78 2026-07-07)`, bundled `rust-lld` |
| Language | Rust, `#![no_std] #![no_main]`, no libc / no CRT / no runtime |
| ISA | x86_64 only |
| Linux target | `x86_64-unknown-linux-gnu`, cross-compiled from Windows (static ELF, no libc) |
| Windows target | `x86_64-pc-windows-msvc` (native PE, `/nodefaultlib`, no CRT) |
| Common flags | `-O -C panic=abort -C debuginfo=0`; blobs `-C relocation-model=pic` flattened via `ld.lld --oformat binary` |
| Byte counts | strip-equivalent release; exact flags in `build/build_linux.sh` / `build/build_windows.ps1` |
| Content hash | **FNV-1a/64** (16 hex chars), compact and already in-tree; a production system would use SHA-256 (enlarges the verify loader, §③) |

**Execution status:** Windows artifacts were **built and run** (verified below). Linux
artifacts were **built and byte-measured but not executed** (no WSL on the host), same as
Q0. The content-addressing *mechanism* is proven on Windows; the store's dedup is
**structural** (identical content → identical filename), so it holds identically on Linux.

**Flat blobs are ELF-target on both OSes** (clean flat extraction; the PE loader bridges
sysv64→win64 when calling the OS). Consequently the *adapter-free payload blobs are
byte-identical across OS* (`ca_payload_rhp` = 617 B on both), and only the adapter blobs
(which contain the per-OS reach code) differ by OS. Loader `.exe`s are PE-aligned to 512
on Windows (which blurs ③ there); **Linux is the clean ③ number.**

---

## Artifact sizes (bytes)

### Blobs (flat, unpadded on both OSes)

| blob | Linux | Windows | what it is |
|---|--:|--:|---|
| `baked_rhp` (payload + adapter v1) | 841 | 1160 | **baseline**: adapter compiled in |
| `baked_readlen` (payload + adapter v1) | 433 | 752 | **baseline**: adapter compiled in |
| `ca_payload_rhp` (payload only) | 617 | 617 | CA: no adapter baked in |
| `ca_payload_readlen` (payload only) | 209 | 209 | CA: no adapter baked in |
| `ca_adapter_v1` (full read) | 232 | 552 | CA: **stored once, shared** |
| `ca_adapter_v2` (truncated read) | 242 | 576 | CA: incompatible version (②) |
| `ca_adapter_v1alt` (loop read, ≡ v1) | 260 | 600 | CA: equivalent-not-identical (④b) |

### Loaders (PE 512-aligned on Windows; unpadded ELF on Linux)

| loader | Linux | Windows | notes |
|---|--:|--:|---|
| `loader_embed` (Q0 variant-B; embeds one blob) | 2904 | 4608 | includes an 841-B embedded blob (Linux) |
| `loader_ca` (content-addressed, **no** verify) | 2672 | 4608 | embeds no blob; reads store at runtime |
| `loader_ca_verify` (recomputes hash on load) | 4320 | 6656 | + a hash fn in the TCB |

### Content store (note the dedup)

```
store/aaf8b49f6b10aa5c.bin   552   <- adapter v1  (referenced by BOTH programs, ONE file)
store/26505ca2d1bca982.bin   576   <- adapter v2
store/d001c3fc65093a29.bin   617   <- payload rhp
store/20dadc64497288c1.bin   209   <- payload readlen
```

Manifest (a program's content-addressed bill of materials) = **33 bytes** (two 16-hex
hashes + newline).

---

## ① Reuse / dedup — THE DECISIVE GATE

Two independent payloads both need the file adapter. Does the adapter's bytes appear
once or twice?

**Total on-disk bytes for the set {rhp, read_len}:**

| | Linux | Windows |
|---|--:|--:|
| **Baked baseline** (`baked_rhp + baked_readlen`) — adapter counted **twice** | 841+433 = **1274** | 1160+752 = **1912** |
| **Content-addressed** (`payload_rhp + payload_readlen + adapter×1`) | 617+209+232 = **1058** | 617+209+552 = **1378** |
| **Saving at N=2** | **216** | **534** |

**The slope (why this is the decisive metric).** Each baked file-payload embeds ~224 B
(Linux) / ~540 B (Windows) of adapter; the content-addressed store holds it **once**. For
**N** file-reading payloads:

```
baked_total   ≈ Σ payload_logic_i  +  N × adapter      (adapter counted N times)
CA_total      ≈ Σ payload_logic_i  +  1 × adapter      (adapter counted once)
saving        ≈ (N − 1) × adapter        → LINEAR in N
```

At N=1 CA is ~8 B *larger* (the caps-indirection glue); the crossover is **N=2**, and CA
wins by an ever-widening margin thereafter. **① passes: the adapter is genuinely shared
on disk, not duplicated.** (The Q0 ⑥ "coexistence" result never showed this — two baked
blobs coexist while *each* carries its own adapter copy; that is coexistence *without*
reuse = the fragmentation this experiment set out to test.)

Whole-delivery note: CA also ships **one universal frozen loader** (2672 B Linux) for
*all* programs, whereas the embed baseline ships one loader **per program** with the
~2.7 KB kernel duplicated each time (Q0 ⑤/⑥). That only strengthens ①; the table above
isolates the pure adapter-reuse signal.

## ② Incompatible versions + reuse coexistence

`v1` (full read) and `v2` (read ≤ 8 bytes) are a **breaking behavioral change**: a payload
built for one misbehaves on the other. Under content addressing they carry different
hashes (`aaf8…` vs `2650…`) and both live in the store. Each program's manifest names the
version it wants:

```
rhp        → payload_rhp,     adapter_v1   → prints a49d2cbecc13994f  (hash of all 35 bytes)
readlen/v1 → payload_readlen, adapter_v1   → prints len=0023          (0x23 = 35 bytes)
readlen/v2 → payload_readlen, adapter_v2   → prints len=0008          (truncated to 8)
```

All three **run correctly on Windows** (verified). **② = YES:** two incompatible versions
coexist, each consumer binds to exactly the bytes it named, with **no central registry,
no anointing, no version negotiation.** Cost = the two adapter blobs coexisting in the
store (232+242 Linux). Contrast: a *name*-keyed scheme ("give me `fileio`") must either
anoint one version (JVM-style monopoly) or ship a version solver (a package manager,
excluded by §1.1).

## ③ Mechanism cost (Q0 in-kernel / out-of-kernel split)

**In-kernel bytes** (the content-addressed loader vs the Q0 embed loader). Linux is the
clean number (Windows PE-alignment rounds `loader_ca` and `loader_embed` both to 4608):

| | Linux |
|---|--:|
| embed-loader code (`loader_embed` − 841 B embedded blob) | ~2063 |
| `loader_ca` code (no verify, embeds no blob) | 2672 |
| **content-addressing mechanism, in-kernel** | **+609** |
| **+ on-load verification** (`loader_ca_verify` − `loader_ca`) | **+1648** |

The +609 B buys: in-kernel file read of the store (via ③④), `manifest.txt` read + parse,
`store/<hash>.bin` path building, the multi-blob load loop, and caps assembly. The +1648 B
buys the **integrity property** (recompute FNV-1a/64 over each blob, compare to its hash);
a cryptographic hash (SHA-256) would cost more.

**Kill-criterion check (§4.3): is the in-kernel cost O(1) in adapter count?** **Yes.** The
loader is a loop over the manifest; a program with 2 or 8 adapters links the *same* loader
code. So the mechanism is **not** an outward-growth term — it does not reintroduce the
disease it set out to cure.

**Lines (in/out-of-kernel, excl. comments):** in-kernel `loader.rs` = 176 vs
`loader_embed.rs` = 32 → **~144 in-kernel mechanism lines** (~30 of them the verify-only
FNV/hex path). Out-of-kernel: `caps.rs` = 8 (the reuse ABI) + per-blob glue
(`adapter_v1.rs` = 17, `payload_rhp.rs` = 15) + a 33-byte manifest per program.

## ④ Discovery / resolution without a registry — the suspect part

**(a) Resolution cost.** To launch a program the loader performs `1 (manifest) + (1
payload + A adapters)` file opens and reads that many blobs. `rhp`: 3 opens (manifest +
payload_rhp + adapter_v1), reading 33 + 617 + 552 B (Windows). The per-program manifest is
**33 bytes**. That is the entire "resolution" — no index, no lookup table, no solver: a
hash is a filename.

**(b) The dedup limit — content addressing dedups bytes, not behavior.** Measured both
directions with real hashes:

| case | hashes | dedup? |
|---|---|---|
| same source, opt-level 2 vs 1 (independent builds) | `aaf8b49f6b10aa5c` == `aaf8b49f6b10aa5c` | **works** — build determinism ⇒ two people building the same source auto-share |
| behaviorally-equivalent *different* impl (loop read) | `aaf8b49f6b10aa5c` ≠ `4b78ab5713675a37` | **fails** — equivalent behavior, different bytes ⇒ two store entries, no sharing |

So the reuse content addressing delivers is reuse of **identical artifacts**. This is the
limit the hypothesis warned about, and it is real. Its saving grace: reproducible builds
make "same source → same hash" hold across independent builders (measured), so convergence
on a shared adapter is *achievable* by sharing source, not by a registry.

**(c) The discovery gap.** Content addressing has **no name→hash direction.** The loader
cannot answer "what is the file adapter?"; it can only fetch a hash it was handed (in
`manifest.txt`). Integrity, dedup, and coexistence are free; **discovery is not provided**
— and any name layer added on top is exactly where "anointing" re-enters. This experiment
stops at that boundary by design (§7).

---

## §4 decision trace (rules fixed before building)

1. **① decisive gate — does the adapter actually share on disk?** CA total (1058 L / 1378
   W) **< baseline** (1274 L / 1912 W) at N=2, adapter stored **once**, saving linear in
   N. **① PASSES → reuse is real. Continue.** (Had CA ≥ baseline, verdict would be "this
   mechanism does not solve fragmentation," stop — it isn't.)
2. **② reuse + coexistence or reuse-forces-anointing?** v1 and v2 coexist by hash, each
   consumer binds its named version, both run correctly, no anointing. **② = the property
   we wanted (reuse *with* coexistence).**
3. **③ price + kill-criterion.** +609 B in-kernel (non-verify) / +1648 B (verify), and
   **O(1) in adapter count** — the mechanism does not grow outward. **Not disqualified.**
4. **④ boundary.** Resolution is `1+(1+A)` opens + a 33-B manifest; dedup works for
   byte-identical (determinism confirmed) but **fails for equivalent-not-identical**;
   **no discovery** direction exists.

**Verdict — 复用可达，有一条发现边界 (reuse achievable, bounded by discovery).** The
content-addressing hypothesis holds where it counts: it turns "N un-shareable copies" into
"identical copies auto-share, divergent copies coexist harmlessly," at a bounded cost that
does not grow with the number of adapters. It does **not** solve *discovery* (name→hash),
and it dedups *bytes*, not *behavior* — both reported as measured, neither fatal. This is a
**bounded success**, not "fragmentation unsolved" and not an unqualified win.

---

## Deviations from the spec (there are always some)

1. **Windows-executed, Linux-cross-measured** (no WSL) — as Q0, and as §3 permits. The
   mechanism runs on Windows; the store's dedup is structural and OS-independent.
2. **Hash = FNV-1a/64, not SHA-256.** Content addressing is hash-agnostic; the compact
   in-tree hash keeps the verify-loader measurement honest as a *lower bound* — a
   cryptographic hash enlarges only the ③ "verify" column, stated explicitly.
3. **The ④(b) opt-level probe found determinism, not divergence** (same source, two opt
   levels → same hash). That inverted the naive expectation ("different flags → different
   bytes") into a *positive* finding for content addressing. The equivalent-not-identical
   case was then shown with a genuinely different implementation (`v1alt`, loop read).
   Recorded, not hidden.
4. **Blobs are ELF-target on both OSes** (Q0 §deviation 3): payloads are ABI-uniform, so
   adapter-free payload blobs are byte-identical across OS; only adapters differ by OS.
5. **No fifth primitive; no new capability KIND.** The loader reaches the store with the
   existing ③④; the reuse mechanism added *packaging* (store + manifest + caps table),
   not a kernel primitive. The §1.1 "urge to build a package manager" did **not** arise —
   the loader is hash→file→map→assemble, O(1) in adapter count. Recorded per §1.1.
6. **Loader is a universal frozen binary** (reads `manifest.txt`), not per-program. This
   is stronger than the spec's suggested per-program manifest embedding, and preserves the
   Q0 ⑤ frozen-TCB property while making "a program" pure data.

---

## Reproduce (third-party runnable)

```sh
# Linux artifacts (cross-compiled; byte-measured):
rustup target add x86_64-unknown-linux-gnu
rustup component add llvm-tools
bash research/dynamic-core/reuse/build/build_linux.sh     # prints sizes into out/
```
```powershell
# Windows artifacts (built, hashed into a content store, run):
pwsh research/dynamic-core/reuse/build/build_windows.ps1
cd research/dynamic-core/reuse/out
[IO.File]::WriteAllText("$PWD\input.txt","dynamic-core experiment 2026-08-08`n")  # 35 bytes
Copy-Item manifest_rhp.txt         manifest.txt -Force; .\loader_ca_windows.exe         # -> a49d2cbecc13994f
Copy-Item manifest_readlen.txt     manifest.txt -Force; .\loader_ca_windows.exe         # -> len=0023 (35 bytes, full read via v1)
Copy-Item manifest_readlen_v2.txt  manifest.txt -Force; .\loader_ca_windows.exe         # -> len=0008 (truncated via v2 — incompatible version coexisting)
Copy-Item manifest_rhp.txt         manifest.txt -Force; .\loader_ca_verify_windows.exe  # -> a49d2cbecc13994f (integrity-checked load)
```

Independent reference hash (FNV-1a/64 of the 35-byte input) = `a49d2cbecc13994f`
(offset basis `0xcbf29ce484222325`, prime `0x100000001b3`) — identical to Q0's, proving
the content-addressed adapter behaves exactly like the baked one.
