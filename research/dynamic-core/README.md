# dynamic-core — 1 layer vs 2 layer, measured

A clean-room, decisive experiment for `plan/design-dynamic-core-experiment.md`.
It builds one dynamic core two ways — **1 layer** (mechanism + platform adaptation
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

## What this experiment does NOT do

The first run stopped at ③ per the §4.4 time-box; a **follow-up run added criterion ④**
(one capability: spawn a subprocess) and nothing else — still **no macOS, no second ISA,
no optimization.** Provenance: built from public technical knowledge only; no
prior/related implementation was read or referenced (clean-room, per
`prd/PRD_02_14_research_provenance.md`).
