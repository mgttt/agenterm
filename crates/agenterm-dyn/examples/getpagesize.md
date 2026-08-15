<!--
CU hand: windows — page size is host sizing context near native window buffers.
This remains a script probe because dyn has no cu buffer model, target tree,
allocation policy, or windows command wiring.
-->

# Read the host page size with `getpagesize`

Linux example; `getpagesize(void) -> int` maps directly to the existing `i32`
dlcall result type.

```lisp
(do
  (set first (dlcall "libc.so.6" "getpagesize" "i32"))
  (set second (dlcall "libc.so.6" "getpagesize" "i32"))
  (if (and (> first 0) (= first second))
    first
    -1))
```

The script returns the positive page size in bytes when two immediate reads
agree, or `-1` otherwise. It does not turn that fact into cu buffer or
allocation policy.
