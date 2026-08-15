# Read the Mach timebase ratio with `mach_timebase_info`

macOS example. Bind caller-owned storage matching two adjacent `u32` fields
(`numer`, then `denom`) and pass its pointer. A zero status means both fields
now describe the conversion from Mach ticks to nanoseconds.

```lisp
(dlcall "libSystem.B.dylib" "mach_timebase_info" "i32" "ptr" ratio)
```

The ratio is a host fact. It is not a timing policy or a portable duration.
