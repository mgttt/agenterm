# Read Darwin wall-clock time with `gettimeofday`

macOS example. The embedding Rust host binds aligned writable `libc::timeval`
storage as `tv` and retains it through unsafe evaluation. The timezone pointer
is the null literal `0`.

```lisp
(dlcall "libSystem.B.dylib" "gettimeofday" "i32" "ptr" tv "ptr" 0)
```

A zero status initializes the caller-owned structure. Seconds and microseconds
advance between observations, so two sequential `timeval` values are not
expected to be identical. The call opens no caller-owned file descriptor and
returns no Mach right.
