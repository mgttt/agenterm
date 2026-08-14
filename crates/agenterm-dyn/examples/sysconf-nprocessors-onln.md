<!--
CU hand: windows — online processor count is host context near process and
window discovery. This remains a script probe because dyn has no cu target
record, scheduling policy, platform facade, or windows command wiring.
-->

# Read the online processor count with `sysconf`

Linux defines `_SC_NPROCESSORS_ONLN` as selector `84`. The selector is
host-specific script data; macOS uses a different value and library.

```lisp
(do
  (set selector 84)
  (set processors
    (dlcall "libc.so.6" "sysconf" "i64" "i32" selector))
  (if (> processors 0)
    processors
    -1))
```

The result is the positive number of currently online processors, or `-1`
when `sysconf` cannot answer. The script does not introduce scheduling or
authorization behavior.
