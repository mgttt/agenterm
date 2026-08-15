# Read Darwin supplementary groups with `getgroups`

macOS example. The embedding Rust host binds the integer capacity as
`ngroups_max` and aligned writable `gid_t` storage as `gids`, retaining both
through unsafe evaluation.

```lisp
(dlcall "libSystem.B.dylib" "getgroups" "i32" "i32" ngroups_max "ptr" gids)
```

A non-negative result is the number of groups written into the caller-owned
array; `-1` remains the native failure result. dyn does not allocate or retain
that array. The call opens no caller-owned file descriptor and returns no Mach
right.
