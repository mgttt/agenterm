# Read the current CPU with `pthread_cpu_number_np`

On macOS, bind caller-owned zeroed `u64` storage as `cpu` before invoking this
status-returning function. Scheduler migration means repeated observations need
not be equal.

```lisp
(dlcall "libSystem.B.dylib" "pthread_cpu_number_np" "i32" "ptr" cpu)
```
