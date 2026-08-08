# Equivalence-invariant experiment (Q4)

Implements [`plan/design-equivalence-invariant-experiment.md`](../../../plan/design-equivalence-invariant-experiment.md):
**can "one neutral IR, lowered by two independent paths, must be behaviourally
equivalent" be made a STRUCTURAL invariant — a violation blocks producing runnable
bytes — rather than a forgettable after-the-fact check? At what cost and coverage?**

**Result: bounded structural achievability (有边界可达).** The invariant is real and
un-forgettable (`VerifiedArtifact` gates the only path to runnable bytes; a mutated
neutral byte is refused), but it covers **only the neutral core** — everything except the
intent-call regions, which ARE Q1's OS-interface leaks (L1–L5). The unverifiable-by-
structure fraction is **0% (pure) → ~30–41% (file I/O) → ~45–56% (spawn)**. Beyond that
line only after-the-fact differential testing (Tier B) is possible, and on this single-OS
host `spawn`'s SysV path is pure **Tier C** (zero shared structure + un-runnable). See
[`RESULTS.md`](RESULTS.md) for ①–⑤, the coverage table, and the Tier×Leak map.

## Layout

```
main.rs          driver: build each payload through the gate, print coverage, run the
                 negative mutant test, JIT-execute (Win64) ONLY via VerifiedArtifact
verify.rs        the invariant: region model, check_congruence, VerifiedArtifact gate,
                 Coverage accounting
equiv_lower.rs   mod common — Q1 lower/common.rs with region-boundary recording added
                 (emit logic verbatim; 0 extra emitted bytes)
out/             emitted code images + driver.exe (git-ignored)
```

Reused verbatim from Q1 via `#[path]` (unchanged): `../ir/spec/ir.rs`,
`../ir/lower/asm.rs`, `../ir/lower/sysv64.rs`, `../ir/lower/win64.rs`,
`../ir/payloads/payloads.rs`.

## Run

```powershell
cd research/dynamic-core/equiv
mkdir out 2>$null
rustc --edition 2021 -O -A dead_code main.rs -o out/driver.exe
cd out; ./driver.exe
```
