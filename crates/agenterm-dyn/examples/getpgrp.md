<!--
CU hand: focus — a process group is session context near terminal focus and
input ownership. This remains a script probe because dyn has no cu session,
input router, target model, or focus command wiring.
-->

# Read the current process group ID

Linux example; macOS uses `libSystem.B.dylib` with the same `getpgrp` symbol
and `i32` result.

```lisp
(do
  (set first (dlcall "libc.so.6" "getpgrp" "i32"))
  (set second (dlcall "libc.so.6" "getpgrp" "i32"))
  (if (= first second)
    first
    -1))
```

The script returns the stable process-group ID or `-1` if the two immediate
reads disagree. It does not establish cu focus or input authority.
