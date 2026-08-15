<!--
CU hand: focus — a session ID is process-session context near terminal focus
and input ownership. This remains a script probe because dyn has no cu session,
input router, target model, or focus command wiring.
-->

# Read the current process session ID

Passing `0` asks for the calling process. Linux uses `libc.so.6`; macOS uses
`libSystem.B.dylib` with the same `getsid(i32) -> i32` signature.

```lisp
(do
  (set current_process 0)
  (set session
    (dlcall "libc.so.6" "getsid" "i32"
      "i32" current_process))
  (if (> session 0)
    session
    -1))
```

The result is the positive session ID or `-1` on native failure. The script
does not establish cu focus or session authority.
