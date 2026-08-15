<!--
CU hand: windows — page size is host sizing context near native window buffers.
This remains a script probe because dyn has no cu buffer model, target tree,
allocation policy, or windows command wiring.
-->

# Read the host page size with `sysconf`

Linux defines `_SC_PAGESIZE` as selector `30`. The selector is host-specific
script data; macOS uses a different value and library.

```lisp
(do
  (set selector 30)
  (set page_size
    (dlcall "libc.so.6" "sysconf" "i64" "i32" selector))
  (if (> page_size 0)
    page_size
    -1))
```

The result is the host's positive page size in bytes, or `-1` when `sysconf`
cannot answer. The script does not turn that fact into cu buffer or allocation
policy.
