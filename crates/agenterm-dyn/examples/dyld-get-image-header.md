# Read a loaded image header with `_dyld_get_image_header`

macOS example. `_dyld_get_image_header(uint32_t) -> const struct mach_header*`
returns a borrowed Mach-O header for the image at the given index. Index `0`
is the main executable.

```lisp
(dlcall "libSystem.B.dylib" "_dyld_get_image_header" "ptr" "u32" 0)
```

dyn returns the address only. The pointee remains borrowed loader storage; dyn
must neither free nor mutate it.
