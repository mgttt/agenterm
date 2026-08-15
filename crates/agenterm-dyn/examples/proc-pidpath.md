# Read a process path with `proc_pidpath`

macOS example. The embedding Rust host binds a writable byte buffer as `path`
and supplies the buffer capacity as an unsigned 32-bit value.

```lisp
(dlcall "libSystem.B.dylib" "proc_pidpath" "i32"
  "i32" pid "ptr" path "u32" path_len)
```

A positive result is the number of bytes written to the caller-owned buffer.
The host reads its NUL-terminated contents; dyn does not model process identity
or retain the pointer.
