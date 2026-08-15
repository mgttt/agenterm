# Read the executable path with `_NSGetExecutablePath`

macOS example. The embedding Rust host binds a writable byte buffer as `path`
and a writable `u32` capacity slot as `len` before evaluation.

```lisp
(dlcall "libSystem.B.dylib" "_NSGetExecutablePath" "i32"
  "ptr" path "ptr" len)
```

A zero result means `path` holds a NUL-terminated executable path. A nonzero
result means the host must enlarge the caller-owned buffer using the updated
length; dyn does not allocate or retain that buffer.
