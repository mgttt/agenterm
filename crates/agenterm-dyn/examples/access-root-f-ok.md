<!--
CU hand: windows — path existence is host context near process and window-owner
discovery. This remains a script probe because dyn has no cu target record,
filesystem command, authorization policy, or windows command wiring.
-->

# Check that the root directory exists

Before evaluation, the Rust host must bind `root_path` to a NUL-terminated `/`
C string. Linux `F_OK` is `0`; macOS uses `libSystem.B.dylib` with the same
mode value and `access` signature.

```lisp
(do
  (set f_ok 0)
  (set status
    (dlcall "libc.so.6" "access" "i32"
      "ptr" root_path
      "i32" f_ok))
  (if (= status 0)
    0
    -1))
```

The expected result is `0`. The script observes native existence only; it does
not grant access or turn the path into cu policy.
