# ⚠️ 已归档：dynamic-core 已判决实验规格

> **归档于 2026-08-10。** Q0–Q23 研究轨已经封闭，综合结论与重新开启条件由
> `research/dynamic-core/SYNTHESIS.md` 和 `research/dynamic-core/README.md` 拥有。
> 本文件只保存该问题的实验前判据与历史结果，不是活跃版本任务。

# Q13 — Declare on a host that publishes no layout: bake-and-pray, or bake-and-detect?（历史规格）

> ⚠️ **Not AgenTerm product scope.** An independent experiment on the dynamic-core
> research track (`research/dynamic-core/`). Not a must-ship of any version plan,
> owns no swimlane, changes no `PRD.md` capability state.

| field | value |
|-------|-------|
| **Date** | 2026-08-08 |
| **Purpose** | Decide, with measured evidence: on a platform that publishes **no** machine-readable struct layout (Windows — no BTF), does primitive ⑤ `Declare` have a form **better than "bake the offset in and pray"**? Concretely: can a wrong layout assumption be **detected (fail-fast)** instead of **silently crashing** when the platform's struct changes? |
| **Implementation** | `research/dynamic-core/declare/` (NOT hung on the root workspace; raw `rustc`) |
| **Prereqs read** | Q6 `primitives/RESULTS.md` (⑤ Declare = host-conditional; Windows offsets are **irreducibly baked** trust); Q7 `tables/RESULTS.md` (L3a tablifies only in query form + host oracle); Q4 `equiv/RESULTS.md` (L1 naming binding-truth unqueryable); this file §1/§3 |
| **Source discipline** | **Clean-room.** No existing implementation read or referenced; only public Win32 API contracts. May reuse this track's `primitives/` four-primitive test object as the capability-under-test. |

---

## 0. Background + what is already decided (out of scope)

Q6 fixed the closed list at **five, host-conditional**: ① memory ② execute ③ reach
④ call ⑤ declare, closed **iff the host publishes the descriptions** (e.g. Linux BTF).
Absent that, ⑤ **degenerates to a baked table** — and Q6 measured this literally: three
of its four capabilities needed a field offset (`WIN32_FIND_DATAA.cFileName@44`,
`sockaddr_in.sin_*@0/2/4`, `SYSTEM_INFO.dwPageSize@4`), **every one entered as a baked
constant = unverified per-target trust** (`primitives/kernel5.rs::LAYOUT_TABLE` is the
bake made literal).

**Windows has no general offsetof oracle.** So on the only platform we can really run,
⑤ is "bake then pray." Q6 called this "a transferred burden, not kernel bloat" — but
**where it transfers to, who guarantees it there, and how a wrong bake gets caught — none
of that was tested.** Q13 tests exactly that gap.

> **Q13: on a host that publishes no layout (Windows), does `Declare` have a form better
> than bake-and-pray? Can a wrong layout assumption be DETECTED rather than silently
> crash on a platform-version change?**

### Already decided, not reopened here (numbered)

1. The five-primitive closed list and its host-conditional asterisk (Q6). **Not reopened.**
2. That Linux/BTF is a genuine host oracle (Q6/Q7). **Not tested** — BTF is not the problem;
   the platform *without* an oracle is. No Linux, no BTF, no CO-RE here.
3. Four primitives are mechanically complete for *doing* (Q6). Declare is a ③+④ usage
   pattern, not a new mechanism. **Not reopened.**

---

## 1. Hard constraints (violating any → experiment invalid)

### 1.1 Only ③+④ (and ①②), never a new kernel semantic
Every probe / self-check must be built from `sym`+`call` over the same four-primitive
contract as `primitives/main.rs`. No new cross-platform kernel API.

### 1.2 Pathology detector
**Any urge to answer "what is the real offset?" by parsing debug info (PDB / `dbghelp` /
symbol server) is the disease this experiment must DETECT, not satisfy.** That path turns
a 3 KB kernel into a debug-infrastructure client — it violates the whole track. If layout
truth genuinely cannot be obtained without it, **that is a finding** (record it as
permanent residue), not a licence to build a parser. Equally forbidden: manufacturing a
detection where none exists to make the result look better — every "detectable" claim
must be **demonstrated by firing on a deliberately corrupted offset**.

### 1.3 Criteria frozen before code
§3 is fixed before a line of `declare/` is written and not edited afterward. Any deviation
goes in RESULTS §deviations, not into §3.

---

## 2. Minimal experiment content

Reuse Q6's three capabilities verbatim as the layout facts under test (they are already
the honest baked-offset set), add a **detection layer**, and try to break it:

| layout fact | dir | Q6 status | candidate self-check (③+④ only) |
|---|---|---|---|
| `WIN32_FIND_DATAA.cFileName@44` | READ | baked | **constructive**: FindFirstFile a file whose name we chose → cFileName@off must equal it |
| `sockaddr_in.sin_family@0 / sin_addr@4` | WRITE | baked | **round-trip**: bind 127.0.0.1:0 → getsockname reads back → family==AF_INET, addr==0x7f000001 |
| `SYSTEM_INFO.dwPageSize@4` | READ | baked | **cross-field**: wProcessorArchitecture@0==9 (AMD64) ∧ dwPageSize@4∈{known pow2} |

For **each** fact: run the capability with the correct baked offset (must pass the
self-check), then **corrupt the offset** and confirm the self-check **fires** (the decisive
negative probe — detection proven, not asserted). Also probe ①: what layout/machine facts,
if any, does Windows actually *publish* at runtime (GetSystemInfo), and is that oracle
itself gated behind a layout you must bake?

- **ISA**: x86_64 only. **OS**: Windows, really run (this box). No second ISA, no WSL.

---

## 3. Criteria (fixed before code; not edited after)

> The proposition is about **error observability**, not about obtaining the right value.
> The main criterion is DETECTION, and it is a boolean gate proven by a corruption probe.

| # | criterion | method | nature |
|---|-----------|--------|--------|
| **①** | **Host oracle availability** | For each layout/machine fact, classify what Windows *publishes at runtime*: `{machine-fact value queryable \| struct-offset queryable \| not queryable}`. Count. Note whether the oracle's answer is itself behind a baked layout. | list |
| **②** | **Detection (MAIN)** | For each baked fact: does a ③+④-only self-check exist that **fires when the offset is wrong**? **Boolean gate, proven by corrupting each offset and confirming a loud failure** (not a silent wrong result). | **boolean (decisive)** |
| **③** | **Cost** | LOC + bytes of the detection layer, split **kernel-in vs kernel-out (payload)**, vs Q6's "+182 B in-kernel, avoidable." Marginal cost per additional fact (slope). | intercept + slope |
| **④** | **Coverage boundary** | Which facts admit a self-check, which are **detection-impossible** (bake-and-pray forever)? The permanent residue list — the honest ceiling. | list |
| **⑤** | **Relation to Q4** | Q4/L1: name→number binding-truth is unqueryable. Is layout binding-truth checkable? By how much does the trust hole narrow, and modulo what? | analysis |

### Measurement discipline
- Bytes = `rustc -O` object `.text` where a byte count is given; LOC excludes tests/docs.
- **Split kernel-in vs kernel-out** — a payload-side check adds **0 kernel bytes**; that is
  the opposite of Q6's in-kernel +182 B and must be reported as such.
- Every number third-party-reproducible; commands in RESULTS.
- "Detectable" is only claimed where the corruption probe **actually fired** on this box.

---

## 4. Decision tree + kill criterion + time box

**Main = ② (detection boolean gate).**

```
1. ② For each baked layout fact, does a ③+④-only self-check FIRE on a corrupted offset?
   · If NO fact is detectable  -> Q13 negative: Windows layout is a PERMANENT silent-trust
     hole; the architecture must document it as an irreducible trust residue. STOP, write it.
   · If SOME  -> partial: list the detectable set (the improvement over bake-and-pray) and
     the undetectable set (permanent residue -> ④).
   · If ALL   -> strong: baked-but-detectable across the board; bake-and-pray -> bake-and-detect.
2. ② holds (some/all) -> ③ measure the detection layer's cost; compare to Q6 +182 B.
3. ① independently classify the host oracle (is there a real query, or only self-check?).
4. ④ residue list; ⑤ Q4 trust-hole narrowing.
kill criterion: if detection fires for NO fact AND no host oracle exists ->
   "permanent trust hole" (the honest negative) -> stop, write it.
time box: STOP once ①②④ have answers. Do NOT do Linux/BTF, a PDB parser, or a symbol server.
```

**Tree vs §3 priority reconciliation:** ② (boolean, main) is the root; ① (list) and ④
(list) are the mandated stop-set; ③ (cost) and ⑤ (analysis) fall out of the same artifact
and are computed, not separately built — so covering all five does not breach the ①②④
time box. Every §3 criterion is a node.

---

## 5. Directory structure

```
research/dynamic-core/declare/
├─ README.md   ← reproduce commands + result pointer
├─ RESULTS.md  ← ①②③④⑤ + decision trace + deviations
└─ main.rs     ← Win64 real run: four primitives + 3 caps + per-fact self-check
                 + corruption probes proving detection fires + host-oracle probe
```

## 6. Excluded options (do not re-propose)

| option | why excluded |
|--------|--------------|
| PDB / dbghelp / symbol-server layout lookup | §1.2 pathology — turns the kernel into a debug-info client; violates the track |
| Bake the offset into ④'s signature | Q6 already excluded (mixes layout into call = ARM64EC lesson) |
| Full BTF/CO-RE consumer | that is the *host-has-oracle* case (Q6/Q7); Q13 is the host-WITHOUT-oracle case |
| Version-gate every struct on OS build number | coarse fallback only; considered as a candidate, not the primary form (Windows struct ABI is stable, so gating over-rejects) |

## 7. What this experiment does NOT answer

- The Linux/BTF oracle path (Q6/Q7 own it).
- A general Windows offsetof oracle (claimed absent; if present it would need PDB — §1.2).
- Publish half (unwind/ENDBR) — Q6 stubbed it; not layout detection.

## 8. Conclusion backfill

See `research/dynamic-core/declare/RESULTS.md`: decision trace, number table, deviations,
honesty clause, overturned expectations.

---

*Research-track projection. No version ownership, no PRD capability-state change.*
