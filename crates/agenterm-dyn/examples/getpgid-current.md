<!--
CU hand: windows — a process-group ID is process context near window-owner
discovery. This remains a script probe because dyn has no cu target tree,
process model, authorization policy, or windows command wiring.
-->

# Read the current process group ID by PID

Passing `0` asks for the calling process. Linux uses `libc.so.6`; macOS uses
`libSystem.B.dylib` with the same `getpgid(i32) -> i32` signature.

```lisp
(do
  (set current_process 0)
  (set process_group
    (dlcall "libc.so.6" "getpgid" "i32"
      "i32" current_process))
  (if (> process_group 0)
    process_group
    -1))
```

The result is the positive process-group ID or `-1` on native failure. The
script does not turn process grouping into cu target ownership.
