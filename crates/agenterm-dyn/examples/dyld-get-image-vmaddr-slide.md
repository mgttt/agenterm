# Read an image slide with `_dyld_get_image_vmaddr_slide`

macOS example. `_dyld_get_image_vmaddr_slide(uint32_t) -> intptr_t` returns the
ASLR slide of the image at the given index. Index `0` is the main executable.
The result uses `ptr` because dyn has no dedicated intptr type.

```lisp
(dlcall "libSystem.B.dylib" "_dyld_get_image_vmaddr_slide" "ptr" "u32" 0)
```

dyn returns the slide as an address-sized value. It does not interpret or apply
that slide.
