<!--
CU hand: process — descriptor limits may inform a future process view. This
remains a Linux script probe: dyn has no resource-limit owner or cu/platform
wiring.
-->

# Read the descriptor resource limit with `getrlimit`

Linux example. Before evaluation, the Rust host allocates native
`libc::rlimit` storage, binds its writable address as `limits`, and keeps that
storage alive through `Dyn::eval`. `RLIMIT_NOFILE` is selector `7` on Linux.

```rust
let mut limits = std::mem::MaybeUninit::<libc::rlimit>::uninit();
dyn_env.bind("limits", limits.as_mut_ptr().cast())?;

let status = dyn_env.eval(SCRIPT)?;
if status.as_int()? == 0 {
    let limits = unsafe { limits.assume_init() };
    // The host interprets limits.rlim_cur and limits.rlim_max here.
}
```

```lisp
(do
  (set rlimit_nofile 7)
  (dlcall "libc.so.6" "getrlimit" "i32"
    "i32" rlimit_nofile
    "ptr" limits))
```

`0` is native success; `-1` remains the native failure result. This is a
read-only probe: it does not call `setrlimit`. The embedding Rust host owns the
`rlimit` allocation, lifetime, and interpretation; this example adds no dyn,
cu, or platform API.
