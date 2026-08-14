<!--
CU hand: windows — an opened descriptor is process context near window-owner
discovery. This remains a script probe because dyn has no cu target record,
owned-resource wrapper, platform facade, or windows command wiring.
-->

# Open `/dev/null`, check it, and close it

Before evaluation, the Rust host must bind `dev_null` to a NUL-terminated
`/dev/null` C string. Linux `O_RDONLY` is `0`; macOS uses
`libSystem.B.dylib` and its host-specific flag value.

```lisp
(do
  (set read_only 0)
  (set fd
    (dlcall "libc.so.6" "open" "i32"
      "ptr" dev_null
      "i32" read_only))
  (if (< fd 0)
    fd
    (do
      (set tty (dlcall "libc.so.6" "isatty" "i32" "i32" fd))
      (set closed (dlcall "libc.so.6" "close" "i32" "i32" fd))
      (if (= closed 0)
        tty
        -1))))
```

For `/dev/null` the successful result is `0`. An `open` failure is returned
directly; a `close` failure returns `-1`. The script demonstrates sequencing
only and does not establish cu resource ownership.
