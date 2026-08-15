# Read the current thread stack size with `pthread_get_stacksize_np`

macOS example. Obtain the current `pthread_t` with `pthread_self()`, bind its
integer representation as `thread`, and call the fixed-width ABI entry point.

```lisp
(dlcall "libSystem.B.dylib" "pthread_get_stacksize_np" "u64" "u64" thread)
```

The positive result is the current thread's stack allocation size in bytes.
Dyn does not retain the thread handle or acquire any stack ownership.
