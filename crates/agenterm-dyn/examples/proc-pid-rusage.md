# Read process resource accounting with `proc_pid_rusage`

macOS example. Bind a caller-owned `rusage_info_v4` (or same-layout buffer) as
`ri` before evaluation. Supply the current pid and `RUSAGE_INFO_V4` (`4`).

```lisp
(dlcall "libSystem.B.dylib" "proc_pid_rusage" "i32"
  "i32" pid "i32" 4 "ptr" ri)
```

A successful result is `0`. The host then reads identity and size fields from
its own buffer; dyn does not allocate or retain that storage.
