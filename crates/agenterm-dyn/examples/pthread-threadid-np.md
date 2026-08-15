# Read the current thread id with `pthread_threadid_np`

macOS example. Bind a caller-owned `u64` slot as `tid`. Pass a null thread
pointer as `0` so the call names the current thread; do not spell `pthread_t`
in the S-expr.

```lisp
(dlcall "libSystem.B.dylib" "pthread_threadid_np" "i32" "ptr" 0 "ptr" tid)
```

A zero status means `tid` now holds a non-zero kernel thread id. The host owns
that integer slot; dyn does not retain it.
