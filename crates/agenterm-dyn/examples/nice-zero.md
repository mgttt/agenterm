<!--
CU hand: windows — a process nice value is host scheduling context near process
and window-owner discovery. This remains a script probe because dyn has no cu
target record, scheduler policy, authorization layer, or windows command wiring.
-->

# Observe the current nice value without changing it

Passing increment `0` leaves the nice value unchanged. Linux uses `libc.so.6`;
macOS uses `libSystem.B.dylib` with the same `nice(i32) -> i32` signature.

```lisp
(do
  (set no_change 0)
  (set first
    (dlcall "libc.so.6" "nice" "i32" "i32" no_change))
  (set second
    (dlcall "libc.so.6" "nice" "i32" "i32" no_change))
  (if (= first second)
    first
    -1))
```

The script returns the stable native nice value. Because `-1` can also be a
valid value and dyn has no errno primitive, this example does not claim to
distinguish that value from failure or apply cu scheduling policy.
