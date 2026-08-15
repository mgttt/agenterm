# Read a Darwin random word with `arc4random`

macOS example. `arc4random(void) -> uint32_t` is represented by the bounded
unsigned return lane.

```lisp
(dlcall "libSystem.B.dylib" "arc4random" "u32")
```

Each call returns one unsigned 32-bit value. This is a native fact probe, not a
randomness policy or a general entropy API.
