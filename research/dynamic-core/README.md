# dynamic-core — research track index

A clean-room research track on **"maximum information and control from minimum
resources"**: how small can a self-extending native core be, and where does the
growth come from. Every question here is settled by a **decisive experiment**
(criteria fixed *before* building, a decision tree, a kill criterion, a time
box) rather than by argument. The method itself is packaged as
[`.claude/skills/decisive-experiment/SKILL.md`](../../.claude/skills/decisive-experiment/SKILL.md) —
read it before writing a new Q spec.

> ⚠️ **Not AgenTerm product scope.** No version plan owns this, no `PRD.md`
> capability state changes. Specs live in `plan/design-*-experiment.md`, code
> lives here.

## The question board

Status vocabulary: **decided** / **running** / **specced, not started** /
**candidate (not specced)**. A candidate carries **no conclusion**, however
obvious it looks.

| Q | Question | Status | Conclusion so far | Spec | Code |
|---|----------|--------|-------------------|------|------|
| **Q0** | **How many layers?** 1 layer (mechanism + platform adaptation fused) vs 2 layers (frozen minimal kernel + replaceable payload) | **decided** | **2-layer**, but only on the experiment's own priority order — see below. The decisive-by-design metric ③ **tied**; the deferred ④ is what separated them | [`design-dynamic-core-experiment.md`](../../plan/design-dynamic-core-experiment.md) | `core/` `adapters/` `payloads/` `pack/` `build/` → [`RESULTS.md`](./RESULTS.md) |
| **Q1** | **Can a neutral IR defer *all* ABI/layout decisions to lowering?** Same ISA, two incompatible ABIs (SysV64 vs Win64) | **decided** | **Bounded neutrality.** ① passes — `pure_compute` lowers **byte-identically** to both ABIs and runs. The **ABI-placement half is clean**: registers, spill order, shadow space, red zone, return register are derived from the semantic signature alone, zero IR involvement (`CreateProcessA`, 10 args → 6 spilled, runs). This **overturns Q0's read** that one ABI must be fixed and bridged — Q0 conflated *mechanism* with *content*. Every leak (L1–L5) is OS-interface **content**: non-neutral naming, semantic-vs-native arity, and above all **OS struct layout has no neutral form** (spawn collapses to a coarse intent, the two lowerings share nothing). **L2 is the core dilemma: neutral OS calls require encapsulation, which the kernel forbids — so encapsulation did not vanish, it moved into the lowerer and regrows at O(targets × intents).** ⑤ splits it: ABI back-end ~20–30 lines/target (**fixed**) vs OS-interface content ~90–110 lines/target (**linear in intents**). ③ never trips the 200% ceiling | [`design-neutral-ir-experiment.md`](../../plan/design-neutral-ir-experiment.md) | `ir/` → [`RESULTS.md`](./ir/RESULTS.md) |
| **Q2** | **How big is a minimal usable IR→native lowerer (X bytes), and does it belong inside or outside the kernel?** | **running** | — (Q0's 2.7 KB TCB measured a kernel that *cannot run neutral IR*; X is the missing piece) | [`design-lowering-cost-experiment.md`](../../plan/design-lowering-cost-experiment.md) | `lowering/` |
| **Q3** | **Composition of the second layer** — can adapter packages be *reused* (not merely coexist) without a central registry anointing an official one? Failure mode is fragmentation, the mirror image of the JVM monopoly. Q0's ⑥ proved coexistence only | **decided** | **Reuse achievable, bounded by discovery.** Content addressing shares an adapter on disk (① pass: stored once, saving linear in N) *and* lets incompatible versions coexist with no anointing (② yes), at a bounded, adapter-count-**O(1)** in-kernel cost (③ +609 B; +1648 B to verify). Boundary (④): it dedups *bytes* not *behavior* (reproducible builds make same-source→same-hash hold), and provides **no name→hash discovery** — the one place anointing could re-enter. Fragmentation is *converted*, not unsolved | [`design-adapter-reuse-experiment.md`](../../plan/design-adapter-reuse-experiment.md) | `reuse/` → [`reuse/RESULTS.md`](./reuse/RESULTS.md) |
| **Q4** | **Verifiability** — "hand it a blob and it executes" is an unverifiable execution surface (Thompson's trusting-trust, amplified when an agent produces its own code). Target shape: one neutral IR lowered by independent paths must be **behaviourally equivalent**, as a *structural invariant*, not an after-the-fact check (diverse double-compilation) | **decided** | **bounded structural achievability** — the invariant is real & un-forgettable (a construction gate; a mutated neutral byte is refused) but covers **only the neutral core**. Unverifiable-by-structure fraction = intent regions (Q1's L1–L5): **0% pure → ~30–41% file-I/O → ~45–56% spawn**. Beyond it: after-the-fact differential testing (Tier B), collapsing to Tier C for zero-shared/un-runnable targets. Does **not** slide into a correctness proof | [`design-equivalence-invariant-experiment.md`](../../plan/design-equivalence-invariant-experiment.md) | `equiv/` → [`RESULTS.md`](./equiv/RESULTS.md) |
| **Q5** | **The ISA axis** — Q0/Q1/Q2 deliberately pin x86_64. What does a second ISA cost: is IR neutrality preserved, and does "N kernels, one per ISA" stay bounded? This is the track's **largest untested assertion**: Q0 §0.2 asserts "the kernel does not grow with ISA count, you just build N of them" **by reasoning only**, and the whole `N×M → N` collapse rests on N being cheap. Note Q1 counted its 202-line x86-64 encoder as *shared* because both its targets were the same ISA — a second ISA moves that into per-ISA cost, so Q1's two-way split must be re-derived as a **three-way** one (shared / per-ISA / per-target) | **decided** | **§0.2 assertion HOLDS; N×M→N stands.** IR + payloads lowered to aarch64 **byte-identical** (① gate passes — no ISA hint, no new type). A second ISA costs a **bounded ~307-LOC per-ISA lowerer bucket** + **+13% kernel** (four primitives 568→644 B), both **linear in ISA count only, constant in intents/OS** — not the multiplicative blow-up that would falsify §0.2. Three-way split: shared 238 / per-ISA 307–350 / per-target 99–137 each. ISA-axis leaks I1–I5: reach content **and set** is per-(ISA,OS) not per-OS (aarch64 has no `open`/`fork`); immediates & alignment stay in the lowerer (non-leaks); **the ABI-placement axis collapses into the ISA on aarch64** (one AAPCS64 for both OSes — so Q1's "ABI is per-target" is itself ISA-relative); struct-layout (Q1 L3) is ISA-independent. Encoder **26/26 vs LLVM**; byte-measured + encoder-validated, not executed (no aarch64 host) | [`design-isa-axis-experiment.md`](../../plan/design-isa-axis-experiment.md) | `isa/` → [`isa/RESULTS.md`](./isa/RESULTS.md) |
| **Q6** | **Primitive completeness** — is the four-primitive floor stable, or does it creep? Q0 needed no fifth primitive kind, but the second capability forced *completing* ④ `call` (arg ceiling 7→11) at a cost paid by **both** variants' kernels. Open: is that a one-time step or a slope? | **candidate** | Evidence exists but no experiment: see `RESULTS.md` §④ (b2) | — | — |
| **Q7** | **OS-interface content as DATA** — Q1 & Q4 independently hit the *same* seam (OS-interface content L1–L5). Can that content stop being per-target hand-written *code* and become *data* (tables) an intent-/target-agnostic marshaller interprets? If so, the seam improves on growth, verifiability, and reuse at once | **decided** | **Bounded reachable.** For the **single-native-call family** (Alloc/Open/Read/Close/Write) the OS content fully becomes data over one fixed marshaller and **executes** (pure→163, rhp→`a49d2cbecc13994f`): marginal **code** cost of +1 intent and +1 same-ISA target = **0** (engine has zero `match intent` / target branch — verified), all growth is data, and the data is schema-checkable (④: recipe well-formedness caught pre-emit; but the L1 name→number *binding truth* stays trust). Seam stays **code** at (a) **L3b orchestration/control-flow** (multi-call dataflow, SysV fork/branch — forcing it to data = the IDL slide the experiment detects) and (b) **I2 cross-ISA restructuring** (`openat`/`clone`: the syscall *set* changes shape, Q5). L3's layout half **L3a** tablifies only **in query form + host oracle = the missing 5th primitive Declare**. ③ engine ~70–112 fixed LOC vs Q1's growing ~90–110/target; emitted bytes ≤ Q1. ⑤ dimensional correction (Q5): layout=per-OS, reach=per-(ISA,OS), ABI=per-ISA — the seam is not one flat table | [`design-os-interface-as-data-experiment.md`](../../plan/design-os-interface-as-data-experiment.md) | `tables/` → [`tables/RESULTS.md`](./tables/RESULTS.md) |
| **Q8** | **Executable-memory floor** — the reference survey (`plan/reference-cross-target-execution.md` §7.1) claims platforms are systematically clawing back the ground primitives ①(mem RW↔RX) ②(jump into it) ③(raw syscall) stand on ("marked executable" ≠ "the platform lets you execute"). Measured on this real Windows Server 2022 / x86_64 box, not cited | **decided** | **Windows floor holds by default; "shaken" downgraded to "deployment precondition".** Default policy (`DynamicCode = 0x0`): ①② fully available — all three exec-memory routes (RW→RX flip = the Q0 kernel path, direct RWX, section-object map) jump in and run. **ACG** (`ProcessDynamicCodePolicy.ProhibitDynamicCode`) is **opt-in (default off)**; once enabled it cuts **all three** routes with `ERROR_DYNAMIC_CODE_BLOCKED` (1655) — stronger than the survey's "two paths". Gap is **policy-configurable** for our own process (we can just not enable it) but **hard inside a process ACG is externally imposed on** without `AllowThreadOptOut`. Two measured conditional windows: RX made **before** ACG still executes after (one-shot AOT), and `AllowThreadOptOut` + per-thread opt-out restores the flip. ③'s raw-syscall half is unused on Windows (Q0 kernel returns −1); its **symbol-resolution half + primitive ④ are unaffected by ACG** (don't need exec memory). **Model impact:** definitions unchanged, but ② must be polymorphic (direct RX / cross-process / **interpret**) with interpretation a first-class fallback, and ③ must be split into raw-syscall vs symbol-resolution halves. Linux/macOS/iOS/OpenBSD **not testable here (no WSL)** — kept as **"unverified transcription"** per the survey; iOS "no legal path at all" would be the one hard gap | [`design-executable-memory-floor-experiment.md`](../../plan/design-executable-memory-floor-experiment.md) | `platform/` → [`platform/RESULTS.md`](./platform/RESULTS.md) |
| **Q9** | **Interpretation as a first-class backend** — Q8's direct consequence: it split the four primitives into ①②(generate new code, fragile under ACG/hardened/iOS) vs ③④(reach existing code, robust), forcing "interpretation must be a first-class fallback from day one." Key insight: the interpreter runs the *same* neutral IR — it is another **backend**, not another architecture. How big, how slow, how much does it cover, and does Q1's L1–L5 seam survive interpretation? | **decided** | **Interpretation IS a viable first-class fallback.** ① all three Q1 payloads run through `interp::run` on the **byte-identical unchanged IR** (pure→163, rhp→`a49d2cbecc13994f`, spawn→`exit=07`/7) — the IR is a **completely interpretable** backend surface, no IR change. ② eval-core **55 LOC / 1908 B**, whole interpreter **136 LOC / 3177 B** = **21% of Q1's lowerer** (487 LOC / 14819 B) and kernel-magnitude (vs 568/644 B). ③ the one hard cost is **compute-bound inner loops only**: ~**77× vs optimized native** (JIT ceiling), ~5× vs Q1's naive lowering — but **OS-bound payloads = 1.0×** (interpretation overhead is drowned by the OS call). So: first-class **availability** fallback, performance **平替 on OS-bound / 降级可用 on hot compute**. ④ eval-core's ISA-specific LOC = **0** (machine-code-free, verified) → collapses Q5's per-ISA 307–350 LOC bucket to ~0; the lowerer's 14819 B is mostly the x86-64 encoder the interpreter simply lacks. ⑤ **L1–L5 survive verbatim** — same 9 kernel32 symbols, same injected constants, same `STARTUPINFOA=104`/offset-0 layout, same 32-bit out-params — content identical to `win64.rs`, only syntax differs. **The OS seam is a property of the OS interface, orthogonal to execution method; interpretation eliminates the ISA machinery, not the seam.** | [`design-interpreter-backend-experiment.md`](../../plan/design-interpreter-backend-experiment.md) | `interp/` → [`interp/RESULTS.md`](./interp/RESULTS.md) |

**Provenance for every Q:** built from public technical knowledge only; no
prior/related implementation is read or referenced (clean-room, per
[`prd/PRD_02_14_research_provenance.md`](../../prd/PRD_02_14_research_provenance.md)).

**Directory ownership:** Q1/Q2/Q3 run in parallel and each owns exactly one
subdirectory (`ir/`, `lowering/`, `reuse/`). Do not edit another Q's directory.
Q0's code sits at the top level of this directory.

---

# Q0 — 1 layer vs 2 layer, measured

Builds one dynamic core two ways — **1 layer** (mechanism + platform adaptation
fused into one artifact) and **2 layer** (a frozen minimal kernel + a replaceable,
runtime-loaded payload) — across **two operating systems** (Linux and Windows,
x86_64), and measures six numbers. The whole point is the numbers in
[`RESULTS.md`](./RESULTS.md), not the code.

**One-line conclusion:** the decisive-by-design metric ③ (marginal cost of adding a
second OS) is **a tie** — adding Windows grows the existing OS binary by **0 bytes** in
*both* variants. The metric the first run deferred, **④ (marginal cost of +1 capability),
is the one that actually separates them**: a non-user of a new capability grows **0** in
2-layer (new capability = a separate blob) but **+~0.4 KB/capability** in a true
single-product 1-layer. ④ is a *slope* criterion, which §3 ranks above the ② size
intercept, so on the experiment's own priority it tips the balance to **2-layer** (joined
by ⑤ TCB and ⑥ coexistence); 1-layer's only remaining edge is raw ② size. Caveat found:
the capability forced a one-time completion of the ④ `call` primitive (7→11 args) that
grew both variants' kernels. See `RESULTS.md` §④ and the §4 decision trace.

> **Two honest readings survive.** By §4's literal decision tree (which never listed ④
> as a node) it is marginally 1-layer on the ② byte tiebreak; by §3's stated priority
> (slopes outrank intercepts) it is 2-layer. The tree/priority mismatch is a **bug in the
> spec**, recorded rather than papered over — and it is why the skill above insists on
> reconciling the decision tree against the stated priority order.

## The kernel — four primitives, nothing else

```
① memory   mem_alloc (reserve+commit RW) / mem_protect (RW <-> RX)
② jump     load a code blob and transfer control (the variant-B loader)
③ reach    raw_syscall (Linux) + sym = symbol resolution (Windows GetProcAddress)
④ call     invoke any native address from a data description of its args (libffi model)
```

No cross-platform *semantics* live in the kernel — no `open()`, no portable file model.
Platform differences are carried by adapters/payloads. File I/O is done by *calling the
platform's own functions* through ③/④, never by a kernel abstraction.

## Layout

```
core/abi.rs        the ONLY kernel<->payload contract (primitive table); panic handler
core/kernel.rs     the four primitives, Linux + Windows, cfg-gated; loader; entry
                   (④ call: arg ceiling 7->11, raised for spawn's CreateProcessA — §④)
adapters/linux/    read_file/write via raw syscalls; spawn via fork/execve/wait4
adapters/windows/  read_file/write via GetProcAddress+FFI; spawn via CreateProcessA+wait
payloads/pure_compute/     floor payload (no OS)
payloads/read_hash_print/  total-delivery payload (read -> FNV-1a/64 -> print)
payloads/spawn_echo/       +1 capability (§④): spawn child, wait, report exit code
pack/variant_a_onelayer/   static-link everything into one binary; fused.rs = the
                           single-product model (all capabilities in one) for ④'s (b)
pack/variant_b_twolayer/   frozen kernel/loader + flat PIC payload blobs
build/                     build_linux.sh, build_windows.ps1, flat.ld
out/                       build outputs (git-ignored)

ir/                        Q1 (separate spec/owner)
lowering/                  Q2 (separate spec/owner)
reuse/                     Q3 (separate spec/owner)
```

## Build & reproduce

```sh
rustup target add x86_64-unknown-linux-gnu
rustup component add llvm-tools            # for rust-lld / flat-blob extraction
bash research/dynamic-core/build/build_linux.sh
```
```powershell
pwsh research/dynamic-core/build/build_windows.ps1
```

Each script prints the artifact sizes and writes them to `out/`. Correctness-verification
commands (Windows) and the independent reference hash are in `RESULTS.md`.

## What Q0 does NOT do

The first run stopped at ③ per the §4.4 time-box; a **follow-up run added criterion ④**
(one capability: spawn a subprocess) and nothing else — still **no macOS, no second ISA,
no optimization.** Everything else it deliberately left open became Q1–Q6 above.
