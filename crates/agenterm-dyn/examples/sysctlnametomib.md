<!--
CU hand: windows — CPU count is host sizing near process/window discovery.
Script probe only: dyn has no cu target or scheduling policy.
-->

# Resolve `hw.ncpu` to a MIB with `sysctlnametomib`

macOS example. The embedding Rust host binds the NUL-terminated name as `name`,
a caller-owned `[libc::c_int; 8]` output as `mib`, and its element capacity as the
mutable `usize` slot `len`.

```lisp
(dlcall "libSystem.B.dylib" "sysctlnametomib" "i32"
  "ptr" name "ptr" mib "ptr" len)
```

A zero result means `mib[..len]` contains the kernel MIB for the requested
name. This example retains no host-global state.
