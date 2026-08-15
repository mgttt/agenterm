<!--
CU hand: windows — host uptime is sizing context near session lifetime.
Script probe only: dyn has no cu clock or target model.
-->

# Read uptime nanoseconds with `clock_gettime_nsec_np`

macOS example. `clock_gettime_nsec_np(clockid_t) -> uint64_t` is the
Darwin-only nanosecond clock door. Pass `CLOCK_UPTIME_RAW` as an `i32`.

```lisp
(dlcall "libSystem.B.dylib" "clock_gettime_nsec_np" "u64" "i32" 8)
```

Compare two adjacent reads for monotonicity. The value is machine uptime in
nanoseconds, not a portable wall-clock timestamp.
