# Read the Mach-O execute header with `_NSGetMachExecuteHeader`

macOS example. `_NSGetMachExecuteHeader(void) -> void*` returns a pointer into
the main executable's Mach-O header.

```lisp
(dlcall "libSystem.B.dylib" "_NSGetMachExecuteHeader" "ptr")
```

The pointer is non-null. dyn returns the address only; it does not copy or own
the header.
