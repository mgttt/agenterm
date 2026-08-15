# Detect extra threads with `pthread_is_threaded_np`

macOS example. `pthread_is_threaded_np(void) -> int` reports whether the
process has more than one thread.

```lisp
(dlcall "libSystem.B.dylib" "pthread_is_threaded_np" "i32")
```

The result is `0` or `1`. It observes the current process; it does not create
threads or grant a concurrency capability.
