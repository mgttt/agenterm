# Read a bounded Darwin random word with `arc4random_uniform`

macOS example. `arc4random_uniform(uint32_t upper_bound) -> uint32_t` uses
the fixed unsigned integer lane.

```lisp
(dlcall "libSystem.B.dylib" "arc4random_uniform" "u32" "u32" 17)
```

For a nonzero upper bound, the result is less than that bound. Calls consume
system randomness, so two sequential calls are not expected to return the same
value. This probe owns no descriptor, allocation, or Mach right.
