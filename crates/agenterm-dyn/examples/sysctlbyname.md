# Read `hw.ncpu` with `sysctlbyname`

macOS example. The embedding Rust host binds a writable integer buffer as
`value` and a writable byte-count slot as `len` before evaluation.

```lisp
(dlcall "libSystem.B.dylib" "sysctlbyname" "i32"
  "ptr" name "ptr" value "ptr" len "ptr" 0 "u64" 0)
```

`name` must point to the NUL-terminated `hw.ncpu` C string. A zero result
means the kernel wrote the caller-owned buffer; the host validates its value.
