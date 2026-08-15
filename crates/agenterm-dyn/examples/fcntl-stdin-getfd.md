<!--
CU hand: focus — descriptor flags are host input context near terminal focus
and input ownership. This remains a script probe because dyn has no cu session,
descriptor owner, target routing, or focus command wiring.
-->

# Read the flags for file descriptor 0

Linux and macOS define `F_GETFD` as command `1`. Linux uses `libc.so.6`;
macOS uses `libSystem.B.dylib` with the same fixed call shape used here.

```lisp
(do
  (set stdin_fd 0)
  (set get_fd 1)
  (set first
    (dlcall "libc.so.6" "fcntl" "i32"
      "i32" stdin_fd
      "i32" get_fd))
  (set second
    (dlcall "libc.so.6" "fcntl" "i32"
      "i32" stdin_fd
      "i32" get_fd))
  (if (= first second)
    first
    -1))
```

The result is the stable descriptor flags, or `-1` if standard input is not a
valid descriptor or the observations disagree. The script does not establish
cu input ownership.
