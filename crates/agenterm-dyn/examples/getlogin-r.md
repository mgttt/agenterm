# Read the login name with `getlogin_r`

macOS example. Bind a caller-owned writable byte buffer, then pass its address
and capacity. A zero status means the buffer contains a NUL-terminated login
name; `ERANGE` means retry with a larger buffer.

```lisp
(dlcall "libSystem.B.dylib" "getlogin_r" "i32" "ptr" name "u64" 256)
```

This is a process-session fact probe. The caller owns and bounds its buffer.
