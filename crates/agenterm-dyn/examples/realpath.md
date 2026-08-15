# Resolve a Darwin path with `realpath`

macOS example. The embedding Rust host binds a NUL-terminated path as `path`
and a writable `PATH_MAX` byte buffer as `buf`, retaining both through unsafe
evaluation.

```lisp
(dlcall "libSystem.B.dylib" "realpath" "ptr" "ptr" path "ptr" buf)
```

A non-null result is the bound buffer pointer and holds the resolved
NUL-terminated path. A null result stays `0`; the script does not invent a
path or hide the native failure. dyn does not allocate that buffer. The call
opens no caller-owned file descriptor and returns no Mach right.
