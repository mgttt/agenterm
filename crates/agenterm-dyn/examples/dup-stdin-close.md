<!--
CU hand: focus — duplicating standard input is descriptor lifecycle context
near terminal focus. This remains a script probe because dyn has no cu session,
owned-resource wrapper, input router, or focus command wiring.
-->

# Duplicate file descriptor 0 and close the copy

Linux uses `libc.so.6`; macOS uses `libSystem.B.dylib` with the same
`dup(i32) -> i32` and `close(i32) -> i32` signatures.

```lisp
(do
  (set stdin_fd 0)
  (set duplicate
    (dlcall "libc.so.6" "dup" "i32" "i32" stdin_fd))
  (if (< duplicate 0)
    duplicate
    (do
      (set closed
        (dlcall "libc.so.6" "close" "i32" "i32" duplicate))
      (if (= closed 0)
        duplicate
        -1))))
```

Success returns the now-closed duplicate descriptor number. A `dup` failure is
returned directly; a `close` failure returns `-1`. The script demonstrates
sequencing only and does not establish cu resource ownership.
