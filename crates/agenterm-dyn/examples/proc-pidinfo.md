# Read BSD process facts with `proc_pidinfo`

macOS example. Bind a caller-owned buffer matching `proc_bsdinfo`, then pass
the current PID, `PROC_PIDTBSDINFO`, a zero argument, and the buffer size.

```lisp
(dlcall "libSystem.B.dylib" "proc_pidinfo" "i32" "i32" pid "i32" 3 "u64" 0 "ptr" info "i32" info_size)
```

The return value is the byte count written. The probe only reads process facts
for the selected PID and does not retain a kernel resource.
