# Query JIT write-protect availability

macOS example. `pthread_jit_write_protect_supported_np` is a read-only
capability query returning `0` or `1`; it does not switch write protection.

```lisp
(dlcall "libSystem.B.dylib" "pthread_jit_write_protect_supported_np" "i32")
```
