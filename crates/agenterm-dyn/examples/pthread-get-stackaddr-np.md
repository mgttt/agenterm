# Read the current thread stack address with `pthread_get_stackaddr_np`

macOS example. Obtain the current `pthread_t` with `pthread_self()`, bind its
integer representation as `thread`, and call the fixed-width ABI entry point.

```lisp
(dlcall "libSystem.B.dylib" "pthread_get_stackaddr_np" "ptr" "u64" thread)
```

The non-null pointer is borrowed thread metadata. Dyn neither dereferences nor
owns the stack; it is valid only while the represented thread exists.
