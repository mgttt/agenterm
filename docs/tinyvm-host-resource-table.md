# tinyvm host resource table

Owner: [PRD 02.35](../prd/PRD_02_35_agenterm_tinyvm.md)

Status: implemented and executable

`HostResourceTable<T>` is the common `no_std + alloc` owner for native-module
objects that must be named by a standard Wasm `i32`. A host keeps the real
texture, audio stream, platform object or other value in Rust and passes only a
`GuestResourceHandle` token through its versioned function imports.

```text
versioned native module in one runtime
├── HostResourceTable<platform object>  (host-owned, runtime-local)
│   ├── bounded live slots
│   ├── value drop on close / clear / table drop
│   └── generation advance before slot reuse
└── GuestResourceHandle                 (guest-visible i32 bits)
    ├── non-zero table-instance domain
    ├── non-zero slot
    ├── non-zero generation
    └── stale handle → typed failure
```

The low 10 bits encode `slot + 1`, the next 10 bits encode its generation and
the high 12 bits encode a nonzero resource-table-instance domain assigned by
the host.
Closing a value increments the generation before the slot can be reused, so an
old token cannot name the replacement. When a slot spends its final generation,
the table retires that slot permanently instead of wrapping and reviving a very
old token. One table supports at most 1,023 live slots and may choose a much
smaller product-specific bound. Each slot has 1,023 generations before
retirement. One allocator issues up to 4,095 domains without reuse, keeping
handles from sibling modules and replacement runtime instances
non-interchangeable even when their slot and generation happen to match.

This is an ownership and lifecycle primitive, not a permission system. Each
versioned native module owns the meaning of its table and the finite-work rules
for operations on its resources. Handles are not native pointers, OS file
descriptors, globally interchangeable identities or evidence that a capability
was authorized. `NativeModuleRegistry::resource_table` atomically creates at
most one table for each canonical versioned module in one registry. It claims
the table domain from a shared, long-lived `ResourceDomainAllocator`; a
replacement registry using that allocator therefore cannot recreate the old
token space. Ordinary function registration does not allocate a table or enter
one into the converter host profile. Invalid limits and duplicate table
configuration fail explicitly; allocator exhaustion never wraps a domain. A
registry is consumed when constructing `GameRuntime`, so its runtime-local
tables cannot accidentally be installed into a second instance.

These tokens are runtime-local and nonportable. A guest must close native
resources before taking a portable state snapshot, and an embedding must not
interpret an old snapshot's raw token as a restored platform object. The shared
allocator closes aliases among runtimes in its lifetime; it is not a durable
cross-process identity store. A product that persists guest memory must pair it
with an explicit native-resource quiescence/restore protocol rather than
serializing these `i32` values as resources. Registry-created tables share a
type-erased live counter with their one `GameRuntime`; `suspend` runs the guest's
cleanup lifecycle and then fails closed without emitting a snapshot if any
tracked table remains live. Standalone tables remain reusable primitives, but
the runtime cannot observe them unless they were created by its registry.

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
- non-reused shared allocation across sibling modules and replacement runtimes;
- explicit allocator exhaustion after 4,095 table instances;
- permanent retirement after all 1,023 generations rather than aliasing;
- one-shot registry consumption and suspend rejection while a tracked table is live;
- an ordinary TinyArcade cartridge creating, reading and closing a host-owned
  resource through three versioned native imports, then producing a snapshot;
- the same cartridge latching failure when it attempts to snapshot a live resource.
