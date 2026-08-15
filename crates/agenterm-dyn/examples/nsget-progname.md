# Read process program-name storage with `_NSGetProgname`

macOS example. `_NSGetProgname(void) -> char**` returns the address of
CRT-owned program-name storage.

```lisp
(dlcall "libSystem.B.dylib" "_NSGetProgname" "ptr")
```

dyn returns only the outer address. The pointee remains borrowed process-global
storage and must neither be freed nor mutated.
