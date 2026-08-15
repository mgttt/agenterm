# Read a configuration string with `confstr`

macOS example. Pass `_CS_PATH` as the `name` selector. The embedding Rust host
must bind a writable byte buffer as `buf` and its capacity as `len` before
evaluation.

```lisp
(dlcall "libSystem.B.dylib" "confstr" "u64" "i32" name "ptr" buf "u64" len)
```

A nonzero result is the required size, including the trailing NUL. A zero
result is native failure. dyn does not allocate or own `buf`.
