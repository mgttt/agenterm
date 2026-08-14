<!--
CU hand: focus — the effective desktop session eventually needs an explicit
local-user identity alongside its focused target. This remains a script probe
because dyn has no cu session model, identity envelope, or focus command wiring.
-->

# Read the current user ID

Linux example; macOS uses `libSystem.B.dylib` with the same `getuid` symbol and
`u32` result.

```lisp
(do
  (set first (dlcall "libc.so.6" "getuid" "u32"))
  (set second (dlcall "libc.so.6" "getuid" "u32"))
  (if (= first second)
    first
    -1))
```

UID zero is valid, so the script checks two reads for equality instead of
treating zero as failure.
