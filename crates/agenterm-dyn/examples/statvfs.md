# Read Darwin filesystem facts with `statvfs`

macOS example. The embedding Rust host binds a NUL-terminated path as `root`
and aligned writable `libc::statvfs` storage as `info`, retaining both through
unsafe evaluation.

```lisp
(dlcall "libSystem.B.dylib" "statvfs" "i32"
  "ptr" root
  "ptr" info)
```

A zero status initializes the caller-owned structure. Block availability and
other capacity counters can change between observations; compare stable fields
such as block size, fragment size, and name limit instead of requiring two
complete structures to be byte-identical. The call opens no caller-owned file
descriptor and returns no Mach right.
