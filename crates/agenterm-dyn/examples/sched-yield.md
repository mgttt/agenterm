<!--
CU hand: focus — yielding the current thread is host scheduling context near
bounded focus observation. This remains a script probe because dyn has no cu
wait model, scheduler policy, target session, or focus command wiring.
-->

# Yield the current thread

Linux uses `libc.so.6`; macOS uses `libSystem.B.dylib`. `sched_yield` returns
an `i32` status: `0` on success.

```lisp
(do
  (set attempts 0)
  (repeat 1 (set attempts (+ attempts 1)))
  (if (= attempts 1)
    (dlcall "libc.so.6" "sched_yield" "i32")
    0))
```

Successful evaluation returns `0` after yielding once. The script does not
define cu scheduling or retry policy, and it requires no compiled C shim or
libffi bridge.
