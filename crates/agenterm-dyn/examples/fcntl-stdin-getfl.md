<!--
CU hand: focus — file status flags are host input context near terminal focus
and input ownership. This remains a script probe because dyn has no cu session,
descriptor owner, target routing, or focus command wiring.
-->

# Read file descriptor 0's status flags

Linux and macOS define `F_GETFL` as command `3`. Linux uses `libc.so.6`;
macOS uses `libSystem.B.dylib` with the same fixed call shape used here.

```lisp
(do
  (set stdin_fd 0)
  (set get_fl 3)
  (set first
    (dlcall "libc.so.6" "fcntl" "i32"
      "i32" stdin_fd
      "i32" get_fl))
  (set second
    (dlcall "libc.so.6" "fcntl" "i32"
      "i32" stdin_fd
      "i32" get_fl))
  (if (= first second)
    first
    -1))
```

The result is the stable file status flags, or `-1` if standard input is not a
valid descriptor or the observations disagree. The script does not establish
cu input ownership.
