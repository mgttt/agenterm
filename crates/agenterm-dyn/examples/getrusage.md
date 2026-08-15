<!--
CU hand: process — resource accounting may inform a future process view. This
remains a Linux script probe: dyn has no process record, typed rusage owner, or
cu/platform wiring.
-->

# Read this process's resource usage with `getrusage`

Linux example. Before evaluation, the Rust host allocates the native
`libc::rusage`, binds its writable address as `usage`, and keeps that storage
alive through `Dyn::eval`. `RUSAGE_SELF` is `0` on Linux.

```rust
let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
dyn_env.bind("usage", usage.as_mut_ptr().cast())?;

let status = dyn_env.eval(SCRIPT)?;
if status.as_int()? == 0 {
    let usage = unsafe { usage.assume_init() };
    // The host interprets `usage` after successful native initialization.
}
```

```lisp
(do
  (set rusage_self 0)
  (dlcall "libc.so.6" "getrusage" "i32"
    "i32" rusage_self
    "ptr" usage))
```

`0` is native success; `-1` remains the native failure result. The host owns
the allocation, lifetime, and interpretation of `rusage`; this example adds no
new dyn API and does not define cu or platform integration.
