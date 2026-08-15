# Compare two pthread handles with `pthread_equal`

macOS example. The embedding host supplies two opaque `pthread_t` values as
fixed-width `u64` inputs.

```lisp
(dlcall "libSystem.B.dylib" "pthread_equal" "i32" "u64" first "u64" second)
```

A nonzero result denotes equal threads; callers must treat the exact nonzero
value as opaque.
