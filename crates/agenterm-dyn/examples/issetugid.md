# Read set-ID execution state with `issetugid`

macOS example. `issetugid(void) -> int` returns zero or one.

```lisp
(dlcall "libSystem.B.dylib" "issetugid" "i32")
```

This reads an OS fact only; it does not add an authorization policy to dyn.
