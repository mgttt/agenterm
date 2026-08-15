<!--
CU hand: focus — a descriptor offset is host input context near terminal focus
and input ownership. This remains a script probe because dyn has no cu session,
descriptor owner, target routing, or focus command wiring.
-->

# Read file descriptor 0's current offset

Linux and macOS define `SEEK_CUR` as `1`. Linux uses `libc.so.6`; macOS uses
`libSystem.B.dylib` with the same `lseek(i32, i64, i32) -> i64` signature.

```lisp
(do
  (set stdin_fd 0)
  (set no_offset 0)
  (set seek_cur 1)
  (set first
    (dlcall "libc.so.6" "lseek" "i64"
      "i32" stdin_fd
      "i64" no_offset
      "i32" seek_cur))
  (set second
    (dlcall "libc.so.6" "lseek" "i64"
      "i32" stdin_fd
      "i64" no_offset
      "i32" seek_cur))
  (if (= first second)
    first
    -1))
```

The result is the stable current offset. A terminal or pipe commonly returns
`-1` because it is not seekable; the script preserves that native result and
does not turn descriptor state into cu input policy.
