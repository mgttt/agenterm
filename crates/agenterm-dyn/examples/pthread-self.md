# Read the current pthread handle with `pthread_self`

On macOS, `pthread_self` returns the current thread's opaque handle. The
fixed-width ABI carries it as `u64` for an immediate comparison or a later
thread API call.

```lisp
(dlcall "libSystem.B.dylib" "pthread_self" "u64")
```
