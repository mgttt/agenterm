<!--
CU hand: windows — process priority is host scheduling context near process and
window-owner discovery. This remains a script probe because dyn has no cu
target record, scheduler policy, authorization layer, or windows command wiring.
-->

# Read the current process priority

Linux and macOS define `PRIO_PROCESS` as `0`; a target ID of `0` selects the
calling process. Linux uses `libc.so.6`; macOS uses `libSystem.B.dylib`.

```lisp
(do
  (set prio_process 0)
  (set current_process 0)
  (set first
    (dlcall "libc.so.6" "getpriority" "i32"
      "u32" prio_process
      "u32" current_process))
  (set second
    (dlcall "libc.so.6" "getpriority" "i32"
      "u32" prio_process
      "u32" current_process))
  (if (= first second)
    first
    -1))
```

The script returns the stable native priority. Because `-1` can also be a
valid priority and dyn has no errno primitive, this example does not claim to
distinguish that value from failure or turn it into cu scheduling policy.
