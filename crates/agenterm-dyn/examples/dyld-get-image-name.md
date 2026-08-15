# Read a loaded image name with `_dyld_get_image_name`

macOS example. `_dyld_get_image_name(uint32_t) -> const char*` returns a
borrowed name for the image at the given index. Index `0` is the main
executable.

```lisp
(dlcall "libSystem.B.dylib" "_dyld_get_image_name" "ptr" "u32" 0)
```

dyn returns the address only. The pointee remains borrowed loader storage; dyn
must neither free nor mutate it.
