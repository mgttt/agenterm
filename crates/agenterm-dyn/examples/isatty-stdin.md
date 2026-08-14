<!--
CU hand: focus — whether standard input is a terminal is host input context
near focus routing. This remains a script probe because dyn has no cu session,
input owner, target routing, or focus command wiring.
-->

# Check whether file descriptor 0 is a terminal

Linux example; macOS uses `libSystem.B.dylib` with the same `isatty` symbol
and `i32` signature.

```lisp
(do
  (set first (dlcall "libc.so.6" "isatty" "i32" "i32" 0))
  (set second (dlcall "libc.so.6" "isatty" "i32" "i32" 0))
  (if (= first second)
    first
    -1))
```

The stable result is `1` for a terminal and `0` otherwise. The script reports
`-1` only if the two immediate observations disagree; it does not turn this
host fact into cu focus policy.
