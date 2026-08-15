<!--
CU hand: process — CPU accounting can inform a future process view. This remains
a Linux script probe: dyn has no process record, typed `tms` owner, or cu wiring.
-->

# Read process CPU ticks with `times`

Linux example. Before evaluation, the Rust host allocates a native `libc::tms`,
binds its writable address as `tms`, and keeps it alive until `Dyn::eval`
returns. `times(struct tms *) -> clock_t` uses the existing `i64` result type on
the supported Linux targets.

```rust
let mut tms = libc::tms {
    tms_utime: 0,
    tms_stime: 0,
    tms_cutime: 0,
    tms_cstime: 0,
};
dyn_env.bind("tms", (&mut tms as *mut libc::tms).cast())?;
```

```lisp
(do
  (set ticks
    (dlcall "libc.so.6" "times" "i64" "ptr" tms))
  (if (>= ticks 0)
    ticks
    -1))
```

A non-negative result is the elapsed clock-tick count; `tms` receives the
caller-owned process and waited-child CPU tick fields. `-1` remains the native
failure result. The embedding Rust host interprets the structure; this example
does not define cu, platform, or cross-platform process accounting.
