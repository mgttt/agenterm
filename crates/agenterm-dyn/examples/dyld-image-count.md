# Count loaded images with `_dyld_image_count`

macOS example. This fixed-arity call returns the current dynamic-loader image
count as `u32`.

```lisp
(dlcall "libSystem.B.dylib" "_dyld_image_count" "u32")
```

The count is an instantaneous loader fact: it is at least one, but may change
when the process loads or unloads images.
