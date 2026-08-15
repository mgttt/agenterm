# Read the current thread role with `pthread_main_np`

macOS example. `pthread_main_np(void) -> int` reports whether the current
thread is the process main thread.

```lisp
(dlcall "libSystem.B.dylib" "pthread_main_np" "i32")
```

The result is `0` or `1`. It observes the current thread; it does not schedule
work or grant a thread-affinity capability.
