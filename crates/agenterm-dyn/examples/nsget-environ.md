# Read process environment storage with `_NSGetEnviron`

macOS example. `_NSGetEnviron(void) -> char***` returns the address of
CRT-owned environment-vector storage.

```lisp
(dlcall "libSystem.B.dylib" "_NSGetEnviron" "ptr")
```

dyn returns the outer address only. The pointee and environment strings remain
borrowed process-global storage; dyn must neither free nor mutate them.
