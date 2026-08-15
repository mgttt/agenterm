# Read process rusage with `proc_pid_rusage`

macOS example. Bind caller-owned storage matching `rusage_info_v4` as `info`.
Pass the current process id and `RUSAGE_INFO_V4` (decimal `4`).

```lisp
(dlcall "libSystem.B.dylib" "proc_pid_rusage" "i32"
  "i32" pid "i32" 4 "ptr" info)
```

A zero status fills the caller-owned buffer. The host controls that allocation and
may compare stable process identity fields with a later direct native call; dyn
neither allocates nor retains the structure.
