# Read the host UUID with `gethostuuid`

macOS example. The embedding host binds 16-byte UUID storage as `id` and a
`timespec` wait as `wait` before evaluation.

```lisp
(dlcall "libSystem.B.dylib" "gethostuuid" "i32" "ptr" id "ptr" wait)
```

A zero result means `id` holds the host UUID. dyn returns the status only; it
does not allocate or own either buffer.
