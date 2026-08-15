# Count loaded images with `_dyld_image_count`

macOS example. `_dyld_image_count(void) -> uint32_t` reports how many images
the dynamic linker currently has loaded.

```lisp
(dlcall "libSystem.B.dylib" "_dyld_image_count" "u32")
```

The result is at least `1` (the main executable). This observes loader state;
it does not enumerate, map, or retain image records.
