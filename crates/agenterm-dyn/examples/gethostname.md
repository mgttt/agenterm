# Read the host name with `gethostname`

macOS example. The embedding Rust host must bind a writable byte buffer as
`name` and its capacity as `namelen` before evaluation.

```lisp
(dlcall "libSystem.B.dylib" "gethostname" "i32" "ptr" name "u64" namelen)
```

A zero result means `name` holds a NUL-terminated host name. dyn returns the
status only; it does not allocate or own the buffer.
