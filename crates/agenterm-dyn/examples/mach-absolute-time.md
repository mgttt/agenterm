# Read monotonic ticks with `mach_absolute_time`

macOS example. `mach_absolute_time(void) -> uint64_t` is exposed through the
existing signed integer lane while the value fits in that lane.

```lisp
(dlcall "libSystem.B.dylib" "mach_absolute_time" "i64")
```

Compare two adjacent reads for monotonicity. The raw tick unit is machine time,
not a portable duration or a scheduling policy.
