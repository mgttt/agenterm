<!--
CU hand: windows — the process file-creation mask is host context near process
and window-owner discovery. This remains a script probe because dyn has no cu
target record, filesystem policy, owned-resource guard, or windows command wiring.
-->

# Read and immediately restore the process umask

Linux uses `libc.so.6`; macOS uses `libSystem.B.dylib` with the same
`umask(u32) -> u32` signature. `umask(0)` changes process-global state while
returning the previous mask, so the restore call must immediately follow it.

```lisp
(do
  (set temporary 0)
  (set previous
    (dlcall "libc.so.6" "umask" "u32" "u32" temporary))
  (set displaced
    (dlcall "libc.so.6" "umask" "u32" "u32" previous))
  (if (= displaced temporary)
    previous
    -1))
```

Success returns the restored mask. The second call should return `0`, the
temporary mask it displaced. Keep the two calls uninterrupted: the language
has no finally or RAII guard, and this script is not a cu/platform filesystem
policy or a general resource-safety wrapper.
