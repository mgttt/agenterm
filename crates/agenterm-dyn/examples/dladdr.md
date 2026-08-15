# Resolve a loaded address with `dladdr`

macOS example. The embedding host binds the address to query as `addr` and
writable `Dl_info` storage as `info` before evaluation.

```lisp
(dlcall "libSystem.B.dylib" "dladdr" "i32" "ptr" addr "ptr" info)
```

A nonzero result means `info` holds borrowed symbol metadata. dyn returns the
status only; it does not allocate or own the structure. The host must treat
filename and symbol pointers as borrowed loader storage, not owned strings.
