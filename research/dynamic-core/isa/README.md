# Q5 — ISA-axis experiment

Implements [`plan/design-isa-axis-experiment.md`](../../../plan/design-isa-axis-experiment.md):
**add a second ISA (aarch64) beside Q1's x86-64 — is the neutral IR still neutral on
the ISA axis, and does "N kernels, one per ISA" stay bounded?**

**Result: §0.2 assertion HOLDS; the N×M→N collapse stands.** The IR + payloads lowered
to aarch64 **byte-identical** (① gate passes). A second ISA costs a **bounded, ~307-LOC
per-ISA lowerer bucket** and a **+13% kernel** (four primitives 568 B → 644 B), both
**linear in ISA count only** (constant in intents/OS). The aarch64 encoder is validated
**26/26 against LLVM** (no aarch64 host, so byte-measured + encoder-validated, not
executed — Q1's SysV posture). See [`RESULTS.md`](RESULTS.md) for the three-way split,
the ISA-axis leak list (I1–I5), and all numbers.

## Layout
```
spec/ir.rs          neutral IR — reused from Q1, byte-identical (SHARED)
payloads/           the 3 payloads — reused from Q1, byte-identical (SHARED)
lower/a64.rs         AArch64 encoder — PER-ISA (clean-room, LLVM-validated)
lower/common_a64.rs  generic lowering + AAPCS64 placement — PER-ISA
lower/a64_linux.rs   aarch64 Linux (SVC syscalls) — PER-TARGET
lower/a64_win.rs     aarch64 Windows (symbol reach) — PER-TARGET
kernel/prim.rs       four primitives, both ISAs — for the kernel per-ISA delta
kernel/textsize.rs   ELF .text reader (no objdump on host)
main.rs              driver: validate encoder vs LLVM, lower 3 payloads ×2 targets
```

## Run
```powershell
cd research/dynamic-core/isa
rustc --edition 2021 -O -A dead_code main.rs -o out/driver.exe
cd out; ./driver.exe
```
