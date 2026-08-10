# Neutral-IR experiment

Implements [`plan/archive/design-neutral-ir-experiment.md`](../../../plan/archive/design-neutral-ir-experiment.md):
**can one IR artifact defer ALL ABI/layout decisions to lowering, and be independently
lowered to SysV x86_64 and Win64 so both run correctly?**

**Result: bounded neutrality (有边界可达).** The boolean gate ① passes (`pure_compute` is
byte-identical across both ABIs and executes). The ABI *placement* mechanics are genuinely
deferrable and execute correctly on Win64 (7- and 10-arg calls spill correctly from one
IR). Neutrality leaks at the **OS-interface content** — symbol names vs syscall numbers,
out-param widths, and — with no neutral form at all — **OS struct layout** (spawn). See
[`RESULTS.md`](RESULTS.md) for the full ①–⑤ and the leak list (the main product).

## Layout

```
spec/ir.rs          the candidate neutral IR (typed 3-address, one Word type; no ABI/layout)
payloads/           the prior round's 3 payloads, re-expressed in the IR (semantics reused)
lower/asm.rs        x86-64 encoder — SHARED, ISA-only
lower/common.rs     generic lowering (arith/mem/control/call-dispatch) — SHARED
lower/sysv64.rs     lowerer A — SysV ABI + Linux syscall reach (byte-measured)
lower/win64.rs      lowerer B — Win64 ABI + Windows symbol reach (built AND run)
main.rs             driver: lower each payload twice, measure bytes, JIT-run on Win64
out/                emitted code images + driver.exe
```

## Run

```powershell
cd research/dynamic-core/ir
rustc --edition 2021 -O -A dead_code main.rs -o out/driver.exe
cd out; ./driver.exe
```
