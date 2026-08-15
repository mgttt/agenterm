# Read the current thread ID with `pthread_threadid_np`

macOS example. Pass a null thread pointer to select the current thread, and
bind caller-owned `u64` storage for the resulting thread ID.

```lisp
(dlcall "libSystem.B.dylib" "pthread_threadid_np" "i32" "ptr" 0 "ptr" thread_id)
```

A zero status means `thread_id` was written. This observes one thread; it does
not allocate a Mach right or change scheduling.
