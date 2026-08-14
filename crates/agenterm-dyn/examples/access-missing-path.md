<!--
CU hand: get-text — a missing native resource must stay an explicit failure,
just like an unavailable text target. This remains a script probe because dyn
has no cu error envelope, filesystem command, target context, or get-text wiring.
-->

# Check a path that does not exist

Before evaluation, the Rust host must bind `missing_path` to a NUL-terminated
synthetic path that the fixture guarantees does not exist. Linux `F_OK` is `0`;
macOS uses `libSystem.B.dylib` with the same mode value and `access` signature.

```lisp
(do
  (set f_ok 0)
  (set status
    (dlcall "libc.so.6" "access" "i32"
      "ptr" missing_path
      "i32" f_ok))
  (if (= status -1)
    -1
    0))
```

The expected result is `-1`. The script preserves the native failure and does
not substitute a fallback path or pretend the target exists.
