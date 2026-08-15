# Read the Darwin domain name with `getdomainname`

macOS declares `getdomainname(char *name, int namelen) -> int`; its length is
an `i32`, not the `size_t` used by some other Unix hosts. Before evaluation,
the embedding Rust host binds a writable byte buffer as `domain` and keeps it
alive for the call.

```lisp
(dlcall "libSystem.B.dylib" "getdomainname" "i32"
  "ptr" domain
  "i32" 256)
```

A zero status means the caller-owned bounded buffer contains the domain name;
an empty domain is valid. dyn does not allocate or retain that buffer.
