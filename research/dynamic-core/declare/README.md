# Q13 — Declare detection (bake-and-pray vs bake-and-detect)

On a host that publishes no struct layout (Windows — no BTF), can a wrong baked offset
be **detected (fail-fast)** instead of **silently crashing**? Answer, measured on real
Windows/x86_64: **只能烤但可检测** — you still cannot *query* the true offset (no runtime
`offsetof` oracle), but a wrong bake is **detectable** for every layout fact that
participates in a semantic round-trip, using only ③ `sym` + ④ `call`, at **+0 kernel bytes**.

- Spec: [`plan/design-declare-detection-experiment.md`](../../../plan/design-declare-detection-experiment.md)
- Results (numbers, decision trace, deviations): [`RESULTS.md`](./RESULTS.md)

## Reproduce

```powershell
cd research/dynamic-core/declare
mkdir out 2>$null
rustc --edition 2021 -O -A nonstandard_style -A dead_code -A unused_mut main.rs -o out/q13.exe
cd out ; ./q13.exe
```

Runs three self-checks (constructive filename, socket bind+getsockname round-trip,
GetSystemInfo cross-field), each with the correct offset (`[PASS]`) and a corrupted offset
(`[FIRE]`), proving detection by a negative probe rather than assertion.

## Files

```
main.rs   Win64 real run: four primitives (sym+call) + Q6's 3 baked layout facts,
          each wrapped in a ③+④-only self-check + a corruption probe; host-oracle probe.
RESULTS.md ①②③④⑤ + verdict + permanent-residue list + deviations.
```

Not hung on the root workspace (raw `rustc`). Clean-room; reuses only Q6's four-primitive
contract as the capability-under-test.
