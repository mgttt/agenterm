# Fill a caller-owned entropy buffer with `getentropy`

macOS example. Bind a writable 16-byte buffer as `bytes`, then pass its address
and exact capacity.

```lisp
(dlcall "libSystem.B.dylib" "getentropy" "i32" "ptr" bytes "u64" 16)
```

A successful result is `0`. The buffer stays caller-owned; this probe does not
print, compare, or retain its random contents.
