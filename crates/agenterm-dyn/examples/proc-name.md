# Read the current process name with `proc_name`

macOS example. Bind a caller-owned NUL-initialized byte buffer as `name` and
pass the current process ID plus the exact unsigned 32-bit capacity.

```lisp
(dlcall "libSystem.B.dylib" "proc_name" "i32"
  "i32" pid "ptr" name "u32" name_len)
```

A positive result means that `name` contains a C string. The embedding host
owns the buffer and must not treat this process-local fact as stable after exit.
