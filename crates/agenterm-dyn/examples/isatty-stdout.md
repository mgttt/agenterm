<!--
CU hand: get-text — whether standard output is a terminal is host output
context near text observation. This remains a script probe because dyn has no
cu session, output owner, target routing, or get-text command wiring.
-->

# Check whether file descriptor 1 is a terminal

Linux example; macOS uses `libSystem.B.dylib` with the same `isatty` symbol
and `i32` signature.

```lisp
(do
  (set stdout_fd 1)
  (set first
    (dlcall "libc.so.6" "isatty" "i32" "i32" stdout_fd))
  (set second
    (dlcall "libc.so.6" "isatty" "i32" "i32" stdout_fd))
  (if (= first second)
    first
    -1))
```

The stable result is `1` for a terminal and `0` otherwise. The script reports
`-1` only if the immediate observations disagree; it does not establish cu
text-routing policy.
