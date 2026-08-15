# Read a clock resolution with `clock_getres`

macOS example. Pass `CLOCK_MONOTONIC` as the `clock` selector. The embedding
Rust host must bind a writable `timespec` as `ts` before evaluation.

```lisp
(dlcall "libSystem.B.dylib" "clock_getres" "i32" "i32" clock "ptr" ts)
```

A zero result means `ts` holds the clock resolution. dyn returns the status
only; it does not allocate or own the structure.
