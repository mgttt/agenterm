# Read process argv storage with `_NSGetArgv`

macOS example. `_NSGetArgv(void) -> char***` returns the address of CRT-owned
argument-vector storage.

```lisp
(dlcall "libSystem.B.dylib" "_NSGetArgv" "ptr")
```

dyn returns the outer address only. The pointee and argument strings remain
borrowed process-global storage; dyn must neither free nor mutate them.
