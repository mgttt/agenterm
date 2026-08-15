# Read the process argument count with `_NSGetArgc`

macOS example. `_NSGetArgc(void) -> int*` returns a pointer into CRT storage.

```lisp
(dlcall "libSystem.B.dylib" "_NSGetArgc" "ptr")
```

The pointer is non-null. The host dereferences it and observes `*argc >= 1`.
dyn returns the address only; it does not copy or own the integer.
