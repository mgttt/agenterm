<!--
CU hand: windows — a parent PID is process ancestry context near window-owner
discovery. This remains a script probe because dyn has no cu target tree,
process model, authorization policy, or windows command wiring.
-->

# Read the parent process ID

Linux example; macOS uses `libSystem.B.dylib` with the same `getppid` symbol
and `i32` result.

```lisp
(do
  (set first (dlcall "libc.so.6" "getppid" "i32"))
  (set second (dlcall "libc.so.6" "getppid" "i32"))
  (if (= first second)
    first
    -1))
```

The script returns the stable parent PID or `-1` if the two immediate reads
disagree. It does not turn process ancestry into cu target ownership.
