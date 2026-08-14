<!--
CU hand: focus — an effective user identity is session context near a focused
desktop target. This remains a script probe because dyn has no cu session,
identity envelope, authorization policy, or focus command wiring.
-->

# Read the effective user ID

Linux example; macOS uses `libSystem.B.dylib` with the same `geteuid` symbol
and `u32` result.

```lisp
(do
  (set first (dlcall "libc.so.6" "geteuid" "u32"))
  (set second (dlcall "libc.so.6" "geteuid" "u32"))
  (if (= first second)
    first
    -1))
```

Effective UID zero is valid, so the script checks two reads for equality
instead of treating zero as failure.
