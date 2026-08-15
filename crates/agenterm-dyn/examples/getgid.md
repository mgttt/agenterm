<!--
CU hand: windows — a real group identity is process context near window-owner
discovery. This remains a script probe because dyn has no cu target record,
identity envelope, authorization policy, or windows command wiring.
-->

# Read the current group ID

Linux example; macOS uses `libSystem.B.dylib` with the same `getgid` symbol and
`u32` result.

```lisp
(do
  (set first (dlcall "libc.so.6" "getgid" "u32"))
  (set second (dlcall "libc.so.6" "getgid" "u32"))
  (if (= first second)
    first
    -1))
```

GID zero is valid, so the script checks two reads for equality instead of
treating zero as failure. It does not turn group identity into cu authority.
