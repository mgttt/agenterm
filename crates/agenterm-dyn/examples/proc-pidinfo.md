# Read BSD process facts with `proc_pidinfo`

macOS example. Bind a caller-owned `proc_bsdinfo` (or same-layout buffer) as
`info` before evaluation. Supply the current pid, `PROC_PIDTBSDINFO`, a zero
arg, the bound pointer, and the struct byte size.

```lisp
(dlcall "libSystem.B.dylib" "proc_pidinfo" "i32"
  "i32" pid "i32" flavor "u64" 0 "ptr" info "i32" bufsize)
```

A successful result equals `bufsize`. The host then reads `pbi_pid` and
`pbi_ppid` from its own buffer; dyn does not allocate or retain that storage.
