# Read the process argument count with `_NSGetArgc`

macOS example. `_NSGetArgc` returns a pointer to runtime-owned `int` storage;
the result is read-only borrowed data and must not be freed.

```lisp
(dlcall "libSystem.B.dylib" "_NSGetArgc" "ptr")
```

Dereference the returned pointer only while the process remains alive. This
probe reads startup state and does not allocate a resource.
