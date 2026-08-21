# tinyvm host resource table

Owner: [PRD 02.35](../prd/PRD_02_35_agenterm_tinyvm.md)

Status: implemented and executable

`HostResourceTable<T>` is the common `no_std + alloc` owner for native-module
objects that must be named by a standard Wasm `i32`. A host keeps the real
texture, audio stream, platform object or other value in Rust and passes only a
`GuestResourceHandle` token through its versioned function imports.

```text
versioned native module
├── HostResourceTable<platform object>  (host-owned)
│   ├── bounded live slots
│   ├── value drop on close / clear / table drop
│   └── generation advance before slot reuse
└── GuestResourceHandle                 (guest-visible i32 bits)
    ├── non-zero module domain
    ├── non-zero slot
    ├── non-zero generation
    └── stale handle → typed failure
```

The low 12 bits encode `slot + 1`, the next 12 bits encode its generation and
the high 8 bits encode a nonzero native-module domain assigned by the host.
Closing a value increments the generation before the slot can be reused, so an
old token cannot name the replacement. When a slot spends its final generation,
the table retires that slot permanently instead of wrapping and reviving a very
old token. One table supports at most 4,095 live slots and may choose a much
smaller product-specific bound. Up to 255 simultaneous domains keep handles
from different native modules non-interchangeable even when their slot and
generation happen to match.

This is an ownership and lifecycle primitive, not a permission system. Each
versioned native module owns the meaning of its table and the finite-work rules
for operations on its resources. Handles are not native pointers, OS file
descriptors, globally interchangeable identities or evidence that a capability
was authorized. The registry must assign distinct domains to native modules
whose handle types must not be exchanged.

The API is synchronous and contains no executor, queue or hidden thread. It can
therefore be reused by iOS, macOS, Linux, Windows and other hosts without
changing VM execution semantics. `has_capacity()` lets an embedding reject work
before opening an expensive platform resource; `insert` still owns and drops a
supplied value if publication fails.

Executable evidence proves:

- exact bit-preserving `u32` / Wasm `i32` round trips and invalid-zero rejection;
- bounded insertion, mutable access, close, clear and deterministic value drop;
- stale-handle rejection after slot reuse;
- cross-domain rejection for two otherwise identical table positions;
- permanent retirement after all 4,095 generations rather than aliasing;
- an ordinary TinyArcade cartridge creating, reading and closing a host-owned
  resource through three versioned native imports.
