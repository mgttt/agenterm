# Read the program name pointer with `getprogname`

macOS example. The result is a borrowed C-string pointer owned by the process.

```lisp
(dlcall "libSystem.B.dylib" "getprogname" "ptr")
```

The embedding host, not the list language, may inspect the NUL-terminated
bytes before the process exits.
