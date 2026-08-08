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
| **Q6** | **Primitive completeness** — is the four-primitive floor stable, or does it creep? Q0 needed no fifth primitive kind, but the second capability forced *completing* ④ `call` (arg ceiling 7→11) at a cost paid by **both** variants' kernels. Open: is that a one-time step or a slope? | **decided** | **Floor creeps partially — two floors, opposite answers.** The **kernel** floor is **stable**: three capabilities of divergent shape (mmap-file arity 7 / dir arity 2 / socket arity 3) did **not** force ④'s ceiling (11) up — Q0's 7→11 was a **one-time step, not a hidden per-capability slope** (① measured, boolean gate not tripped); socket/mmap/dir add **0 kernel bytes** (they are just more ③+④ calls). **Claim K of §1.1 ("内核永不为覆盖能力而变大") HOLDS.** But the completeness **claim** (Claim R, "nothing unreachable") is **FALSIFIED** for one class: **struct layout (`offsetof`)** — 2 of 3 capabilities need a field offset and **nothing in ①②③④ produces one**; it is only **bakeable** (unverified per-target trust) or, where a host publishes layout (Linux BTF), **fetched via ③+④ + a payload parser** — absent host publication (Windows system structs) it is **irreducibly baked**. Portability work is **transferred, not eliminated** (confirms Q7's L3a by construction; refutes Q0 §④(b2)'s slope fear). Is a 5th primitive **necessary**? **Mechanically NO** (Declare = ③+④ usage: GetSystemInfo/RtlAddFunctionTable/BTF-read), **conceptually YES** (a "do-only" model makes the metadata concern invisible). Closed list = **FIVE, host-conditional** (① memory ② execute ③ reach ④ call ⑤ declare); ⑤ in-kernel floor = **+182 B .text** but **avoidable** (0 if left as ③+④ + baked table). No genuine **sixth** kernel class (orchestration = payload code; callbacks = ①②④). **Not** "no closed list" — the list closes, with an asterisk | [`design-primitive-completeness-experiment.md`](../../plan/design-primitive-completeness-experiment.md) | `primitives/` → [`RESULTS.md`](./primitives/RESULTS.md) |
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

# 技术清单 — the horizontal read

The question board above is **vertical**: one question, one verdict, one experiment.
This section is **horizontal**: one *class of technique* per line, with the number that
was actually measured and where it came from. It is the cross-cut of Q0–Q10, and it is
the thing you hand to someone who has to *choose* mechanisms rather than *settle* a
question.

**Provenance is mandatory and is the point of this section.** Every line carries one of:

- **[实测]** — a number produced by a Q in this track, on the host stated below, with a
  reproduce command in that Q's `RESULTS.md`.
- **[转述未验]** — carried from [`plan/reference-cross-target-execution.md`](../../plan/reference-cross-target-execution.md)
  (below: **R1**) and **never reproduced here**. R1's own §11 warns that its §2/§3/§5/§7/§8
  had **no working WebSearch/WebFetch** — those are model knowledge. R1 §4/§6 *were*
  checked against live primary sources on 2026-08-08; they are still **not our
  measurements**, and are marked **[转述·一手查证]**.

**No third category is allowed.** A transcription is never promoted to a conclusion.

> **Host for every [实测] number unless stated otherwise:** Windows Server 2022
> Datacenter 10.0.20348 (real machine), x86_64, `rustc 1.97.0`, `-O`.
> **Linux/SysV artifacts and all aarch64 artifacts are byte-measured and
> encoder-validated, NOT executed** (no WSL, no ARM host) — track-wide posture, restated
> here so no row is read as "runs on Linux".

## ① Execution — how the payload's own logic runs

| technique | verdict | measured | provenance |
|---|---|---|---|
| **Interpretation** | **Required, and must be first-class** — not a patch bolted onto a JIT-shaped design | eval-core **55 LOC / 1908 B**, whole interpreter 136 LOC / **3177 B** (= 21% of the Q1 lowerer's 14819 B); **ISA-specific LOC in eval-core = 0** (grep-verified), which collapses Q5's per-ISA 307–350 LOC bucket to ≈0; **OS-bound payloads 1.0×**, compute-bound **≈77× vs optimized native** (≈5× vs Q1's naive lowering); runs the **byte-identical unchanged** Q1 IR — the IR is a *complete* interpretable backend surface, not a subset | **[实测] Q9** ([`interp/RESULTS.md`](./interp/RESULTS.md)); the "must be first-class" follows from **[实测] Q8** |
| **JIT / lowering to native** | **A conditional accelerator, not a floor** — it is the piece the platform can take away | X = **3003 B** flat-safe (in-kernel jump-table version ~2777) against a minimal kernel of **~2.93 KB** → **X ≈ the whole kernel**; in-kernel wins total delivery by ~1.5 KB, out-of-kernel halves the frozen TCB (~2.93 KB vs ~6.2–6.4 KB) — a real no-free-lunch tradeoff. Under ACG it is simply unavailable (see ⑥) | **[实测] Q2** ([`lowering/README.md`](./lowering/README.md)) + **[实测] Q8** ([`platform/RESULTS.md`](./platform/RESULTS.md)) |
| **copy-and-patch stencil** | **Measured, and it does not pay at this scale.** R1 §10.1 ranked it #1 of the untried techniques on the argument that it dissolves the Q2 tradeoff. **It does not** — X *grew* | The pure `memcpy` + relocation applier **is** tiny as predicted (**651 B**) — but that was never the expensive part: **opcode decode/dispatch survives as code and dominates** (`emit` 3541 B). Whole runtime code **4515 B**, whole footprint code+data in Q2's own口径 **5826 B ≈ 1.94× Q2's 3003 B**, frozen TCB in-kernel ~8.7 KB vs Q2's ~6.2 KB. **Both endpoints of the tradeoff move up; its shape is preserved.** Bigger than Q2's hand-written lowerer *and* than Q9's interpreter (3177 B). All three payloads execute byte-identically, so the numbers rest on a working mechanism. Boundaries found: control flow **will not stencilize** (a stencil cannot leave CPU flags live across its boundary; a branch target is a layout-time offset) → ~20 B of residual encoder; `CALL` arity is structural, not a hole → per-`argc` variants; stencil table scales **opcodes × ISAs** | **[实测] Q10** ([`stencil/RESULTS.md`](./stencil/RESULTS.md)); the hypothesis it refutes is **[转述未验] R1 §7.4/§8.1/§10.1** |
| **AOT (pre-lowered native, no runtime codegen)** | **Trivially available**, and it is the one codegen route that survives a hardened process — at the cost of being one-shot | Q0's variant-B payloads *are* precompiled flat native blobs, mapped and jumped, executed on Windows. Q8-T1 measured that memory flipped to RX **before** ACG is enabled still executes **after** — i.e. "lay all the code down before the lock" works, but you can never generate more | **[实测] Q0** ([`RESULTS.md`](./RESULTS.md)) + **[实测] Q8 J5-T1** |

## ② Reach — getting to code that already exists

| technique | verdict | measured | provenance |
|---|---|---|---|
| **Symbol resolution** (`GetProcAddress` / `dlsym`) | **First-class and constantly available** — the one reach mechanism nothing has taken away | ACG (`ProhibitDynamicCode`) does **not** touch `LoadLibraryA`/`GetProcAddress`; with ACG on, the probe keeps calling kernel32 exports and printing normally. It needs no executable memory | **[实测] Q8 J6** |
| **FFI — invoke any address from a data description of its args** | **The load-bearing wall.** With ③+④ the kernel implements *nothing* and still covers new capabilities — but only over the integer/pointer word subset | Three capabilities of divergent shape (mmap-file arity 7, dir arity 2, socket arity 3) added **0 kernel bytes** and did **not** force ④'s arity ceiling (11) up — max observed native arity **7**. Q0's 7→11 was a **one-time step, not a per-capability slope**. **Boundary (honest):** float/SIMD args, struct-by-value, varargs, `sret` are *shape* limits ④ cannot express at any arity — out of scope, not solved | **[实测] Q6** ([`primitives/RESULTS.md`](./primitives/RESULTS.md)); exec-memory independence **[实测] Q8 J6** |
| **Raw syscall** | **Split the primitive: this half has a completely different foundation from symbol resolution** | Windows: unused by construction — Q0's kernel `raw_syscall` returns −1 and the Windows path goes entirely through ③-symbol + ④. Linux: the adapters' only reach mechanism (**byte-measured, not executed** — no WSL) | **[实测] Q8 J6 / Q0** for the Windows half; Linux half **byte-measured only** |
| — the OpenBSD hazard | If true, raw syscall issued **from generated code** kills the process (`msyscall`/pinsyscalls, 7.3+) — which would make the "raw-syscall primitive + emit it into JIT memory" design fatal-by-construction there | **not measured — no OpenBSD host.** Q8 files it explicitly as an unverified transcription | **[转述未验] R1 §7.1**, restated as unverified in Q8's credibility table |

## ③ Description — the class the four-primitive model made invisible

| technique | verdict | measured | provenance |
|---|---|---|---|
| **`Declare` / layout query** (`GetSystemInfo`, `RtlAddFunctionTable`, Linux BTF) | **A real, distinct concern — mechanically not a new primitive, conceptually yes.** The diagnosis: **reaching an *address* ≠ reaching a *description*.** `sym` resolves symbol→address; **no amount of `sym` answers `offsetof`**, and nothing in ①②③④ produces an offset | Q6 ran all three capabilities with **baked** offsets (`WIN32_FIND_DATAA.cFileName`@44, `sockaddr_in.sin_port`@2, `SYSTEM_INFO.dwPageSize`@4 — baked *even to read the host's own answer*). Kernel cost of promoting Declare to a uniform in-kernel query channel = **+182 B `.text`** (550→732), and it is **avoidable → 0** if left as a ③+④ usage pattern + a payload-side baked table. **Claim R of §1.1 ("nothing unreachable") is falsified** for the layout class | **[实测] Q6 ②③**; the query-form requirement independently reached by **[实测] Q7 L3a**; the CO-RE/BTF mechanism itself is **[转述·一手查证] R1 §4.1e** |
| **Cost of not doing it** | Layout constants get **baked into the artifact**, and the failure mode when the platform's struct changes is a **silently wrong offset, not an error** | Q7's oracle is a **stub** (`FieldSrc::Queried` carries the answer inline) — the finding is "L3a needs a query channel", **not** "here is the oracle". Where a host publishes nothing machine-readable (Windows system structs), the fact is **irreducibly baked** | **[实测] Q6 ②** + **[实测] Q7 deviation 3** |

## ④ Distribution & reuse

| technique | verdict | measured | provenance |
|---|---|---|---|
| **Content addressing** | Dedup + version coexistence + **no registry, no anointing** — at a cost that is **O(1) in adapter count** | Loader mechanism = **+609 B** in-kernel over the Q0 embed loader (Linux, the clean number); **+1648 B** more to *verify* content on load. Two payloads sharing a file adapter: CA 1058 B vs baked 1274 B at N=2, saving **linear in N**. Incompatible v1/v2 coexist by hash and **both run**. Boundary: it dedups **bytes, not behavior**, and provides **no name→hash discovery** — the one place anointing could re-enter | **[实测] Q3** ([`reuse/RESULTS.md`](./reuse/RESULTS.md)) |
| **Table-driven marshalling** (OS-interface content as data) | For the **single-native-call family**, capability growth becomes **pure data** over a fixed engine | Engine = **70 LOC fixed** single-call core (+42 L3a struct-building, +47 schema types → ~70–112 fixed LOC), vs Q1's **~90–110 LOC/target that grows per intent AND per target**. Marginal **code** cost of +1 intent = **0**, of +1 same-ISA target = **0**; all growth is data (~5–13 LOC/intent, ~57–58 LOC/target). Discipline is structurally checked: `grep -c 'abi.name ==' marshal.rs` → **0** (no per-target branch); `match .*intent` hits **only a doc comment**. Emitted bytes ≤ Q1 (Win64 rhp 1216 vs 1249). Executes | **[实测] Q7** ([`tables/RESULTS.md`](./tables/RESULTS.md)) |
| — where it stops | **L3b orchestration/control flow** (multi-call dataflow, SysV `fork`+branch — *no flat table has a branch*) and **I2 cross-ISA restructuring** (aarch64 has no `open`/`fork`: the syscall *set* changes shape, not just its number). Forcing either into data = inventing a call-sequencing bytecode = **the IDL slide** | `spawn_boundary()`: tablable-as-data **1**, needs-query-channel **2**, **irreducibly code 5**. Also unchecked by any schema: whether symbol index 1 *really is* `CreateFileA` — the naming *binding truth* stays trust | **[实测] Q7 ⑤**; I2 folded in from **[实测] Q5**, analyzed not executed |
| **Relocation / flat-blob discipline** | A flat, non-relocated blob that is itself a **code generator** carries constraints Q0's precompiled payloads never had: it must be **memset-free and jump-table-free** | `llvm-objdump` located a real `jmpq *%rcx` jump table in the emit path; suppressed with `-C llvm-args=-min-jump-table-entries=200` plus taking scratch from primitive ① instead of `memset`. **Measured cost: +8% on X** (2777 → 3003 B) and a more fragile build | **[实测] Q2** (spec §8 decision trace; build scripts carry the flag) |
| — placement reach | **Real, and hit in practice.** `R_X86_64_PC32` holes silently truncate beyond **±2 GB**; a "give me N bytes" memory primitive that does not guarantee proximity **fails silently, not loudly** | Q10's applier had to co-locate code / register file / const pool / *a copy of the env table* in **one arena** (flipping only the code sub-range to RX) so every rip-relative hole stays in reach. **This confirms R1 §10.3: primitive ① is missing a placement-constraint parameter.** The aarch64 `CALL26` ±128 MB half remains **[转述未验]** (no ARM host) | **[实测] Q10 ④** for the x86 PC32 half; **[转述未验] R1 §7.4/§10.3** for the rest |

## ⑤ Verifiability

| technique | verdict | measured | provenance |
|---|---|---|---|
| **Structural equivalence guard + diverse double-compilation** | **A construction gate, not an after-the-fact check** — and it is nearly free | **~55 LOC** of guard (riding on ~60 LOC of region instrumentation coextensive with the lowerer's own control flow); **0 extra bytes in the artifact**; needs **no execution** and no second OS. A mutated Neutral byte makes `build` return `Err` and yields **no runnable bytes** (negative test passes) | **[实测] Q4** ([`equiv/RESULTS.md`](./equiv/RESULTS.md)) |
| — why a whole-image `memcmp` cannot be the invariant | It passes **only** for `pure_compute`; it would reject every real payload | The strict **Neutral** regions *are* byte-identical across targets (246 / 676 / 623 bytes) — but the shared path also contains **frame size (M1)** and the **entry ctx register (M2)**, which are **baked ABI facts** and byte-*divergent* while behaviourally trivial. Hence: compare **by region** (Neutral = bytes, Control = target label, Frame/CtxReg/Intent = quarantined). **Byte-identity is strictly stronger than equivalence even inside the shared core** | **[实测] Q4** (a Q4-new finding Q1 did not surface) |
| — the ceiling | Structurally unverifiable fraction = the intent regions = Q1's L1–L5: **0% pure → ~30–41% file-I/O → ~45–56% spawn**. Below that: after-the-fact differential testing (Tier B), collapsing to Tier C where a target is un-runnable or (L5) where equivalence **cannot even be stated** neutrally | **[实测] Q4 ②④** |

## ⑥ Platform-imposed — not chosen, must be handled

| technique | verdict | measured | provenance |
|---|---|---|---|
| **Three ways to obtain executable memory** (`RW→RX` flip / direct RWX / section object) | **Primitive ② must be polymorphic.** Under default policy all three work; under ACG **all three die** | Default `DynamicCode` policy reads **0x0** (ACG is **opt-in**): M1/M2/M3 each jump in and return 42. With ACG on: M1 fails at `VirtualProtect(→RX)`, M2 at `VirtualAlloc(RWX)`, M3 at `MapViewOfFile(FILE_MAP_EXECUTE)` — **all three, all `1655 = ERROR_DYNAMIC_CODE_BLOCKED`** (code from this machine's own `winerror.h`). Two conditional windows measured: pre-ACG RX survives (one-shot AOT), and `AllowThreadOptOut` + per-thread opt-out restores the flip | **[实测] Q8 J1–J5**. This **corrects R1 §7.1**, which says ACG cuts *two* paths |
| — consequence | On an externally-imposed ACG process (and, if R1 is right, iOS), the only general legal path left is **interpretation** | Q8 → Q9: interpretation is measured at 3177 B and 1.0× on OS-bound work — the fallback is affordable | **[实测] Q8 §对四原语模型的影响** + **[实测] Q9** |
| **Not yet handled by any experiment** | I-cache coherence across threads (ARM/RISC-V; the *other* thread needs its own `ISB`); CET-IBT `ENDBR64` / ARM BTI landing pads / arm64e PAC signing; Windows x64 unwind registration (`RtlAddFunctionTable`, and the leaf-function exemption); TLS access models; code retirement | **nothing measured.** Q6's Publish half is an explicit **stub** (call shape only, not a real unwind registration) | **[转述未验] R1 §7.2/§10.5**; stub status **[实测] Q6 deviation 4** |
| **Other platforms' policies** (Linux `MemoryDenyWriteExecute` / `MFD_NOEXEC_SEAL`, macOS `MAP_JIT` + entitlement, iOS "no legal path at all", OpenBSD `wxallowed`) | If iOS is as described it is the one **hard** gap; everything else looks policy-configurable | **not measurable here** (no WSL, not macOS/iOS/OpenBSD). Q8 keeps every one of these as an unverified transcription rather than folding them into its verdict | **[转述未验] R1 §7.1**, listed row-by-row in Q8's credibility table |

## ⑦ In the space, not yet used by us

Everything in this group is **[转述未验]** or **[转述·一手查证]** — *no* line here has a
number of ours behind it. It is here so the space is visible, not so it can be cited.

| technique | why it is on the list | provenance |
|---|---|---|
| **Binary translation** (QEMU TCG, Rosetta 2, FX!32) | The survivors all **avoid** the OS axis rather than solving it (jacket into native libs / sit on an already-ported OS). The one project that really carried a foreign OS surface (Lx86) died of it. Directly usable idea: **persist the lowering result instead of redoing it every load** — three independent convergences (FX!32, Windows XTA cache, Rosetta 2) | **[转述未验] R1 §3.1–3.9, §10.8** |
| **eBPF-style verifier + load-time JIT** | The only shipping system whose constraints resemble ours. Also the clearest "you cannot have both": kernel `verifier.c` = **20,065 lines**, three orders of magnitude over our kernel; userspace eBPF runtimes simply **do not** reimplement it (rbpf's `verifier.rs` = 13 KB) | **[转述·一手查证] R1 §4.1b/§4.1f** (kernel source + GitHub, checked 2026-08-08) |
| **CO-RE / load-time binding against a host description** | The shipping answer to "keep layout out of the payload": payload carries *name + type + relocation record*, loader carries the **layout oracle**, offsets are computed at bind time and folded in. Q7 reached the same shape from the other direction but built **no oracle** | **[转述·一手查证] R1 §4.1e**; our half **[实测] Q7 L3a** |
| **"Take the intersection, not the union"** | eBPF's 11 registers are the **intersection of real 64-bit ABIs**, not a neutral choice — deliberately sacrificing neutrality (11 regs, 32-bit zero-extension) buys a per-arch JIT of **~4k lines** instead of a register allocator, and pushes edge architectures onto the interpreter. We have never priced this option | **[转述·一手查证] R1 §4.1a/§10.7** |

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
