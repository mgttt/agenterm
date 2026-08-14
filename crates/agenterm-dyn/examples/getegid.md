<!--
CU hand: windows — an effective group identity is process context near window
ownership discovery. This remains a script probe because dyn has no cu target,
identity envelope, authorization policy, or windows command wiring.
-->

# Read the effective group ID

Linux example; macOS uses `libSystem.B.dylib` with the same `getegid` symbol
and `u32` result.

```lisp
(do
  (set first (dlcall "libc.so.6" "getegid" "u32"))
  (set second (dlcall "libc.so.6" "getegid" "u32"))
  (if (= first second)
    first
    -1))
```

Effective GID zero is valid, so the script checks two reads for equality
instead of treating zero as failure.
