# Query an allocator size class with `malloc_good_size`

On macOS, `malloc_good_size` returns an allocation size at least as large as
the requested fixed-width byte count. It performs no allocation.

```lisp
(dlcall "libSystem.B.dylib" "malloc_good_size" "u64" "u64" 4097)
```
