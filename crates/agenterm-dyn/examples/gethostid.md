<!--
CU hand: windows — a host ID is machine context near window-target discovery.
This remains a script probe because dyn has no cu host inventory, target tree,
machine identity contract, or windows command wiring.
-->

# Read the current host ID

Linux 64-bit example. `gethostid(void) -> long` maps to the existing `i64`
dlcall result type on the supported Linux x86_64 and aarch64 cells.

```lisp
(do
  (set first (dlcall "libc.so.6" "gethostid" "i64"))
  (set second (dlcall "libc.so.6" "gethostid" "i64"))
  (if (= first second)
    first
    -1))
```

The script returns the host ID when two immediate reads agree, or `-1`
otherwise. It does not turn that process-visible value into cu machine or
window-target identity.
