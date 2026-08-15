<!--
CU hand: windows — CPU count is host sizing near process/window discovery.
Script probe only: dyn has no cu target or scheduling policy.
-->

# Read `hw.ncpu` with `sysctl`

macOS example. The embedding Rust host binds an `i32` MIB pair
(`[CTL_HW, HW_NCPU]`), a writable integer buffer as `oldp`, and a writable
byte-count slot as `oldlenp` before evaluation. Six-argument `dlcall` max.

```lisp
(dlcall "libSystem.B.dylib" "sysctl" "i32"
  "ptr" mib "u32" 2 "ptr" oldp "ptr" oldlenp "ptr" 0 "u64" 0)
```

A zero result means the kernel wrote the caller-owned buffer; the host
validates that the CPU count is at least one.
