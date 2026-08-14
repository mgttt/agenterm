<!--
CU hand: focus — DISPLAY identifies the X display on which focus discovery
would begin. This remains a script probe because dyn has no X11/AT-SPI session,
cu focus command, target routing, or returned C-string decoder.
-->

# Read the `DISPLAY` environment pointer

Before evaluation, the Rust host must bind `display_key` to a NUL-terminated
`DISPLAY` C string. `getenv` returns either the borrowed value pointer or null;
the script reduces that to a readable present/absent result.

```lisp
(do
  (set display
    (dlcall "libc.so.6" "getenv" "ptr" "ptr" display_key))
  (if (and display (not (= 0 1)))
    1
    0))
```

The result is `1` when `DISPLAY` is present and `0` when it is absent. The
pointer remains owned by the process environment.
