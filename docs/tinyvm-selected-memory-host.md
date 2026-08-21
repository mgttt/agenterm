# tinyvm selected-memory host callbacks

Owner: [PRD 02.35](../prd/PRD_02_35_agenterm_tinyvm.md)

Status: implemented and executable

`bind_import_typed_in_place_with_memories` is the reusable native-module door
for a standard multi-memory guest. It preserves the existing typed, bounded,
allocation-free result path while replacing the implicit memory-zero slice with
a call-scoped `WasmHostMemories` context.

```text
standard function import
└── synchronous typed callback
    ├── borrowed parameters
    ├── exact bounded result destination
    └── WasmHostMemories
        ├── len / is_empty
        ├── memory(standard index) -> read guard
        └── memory_mut(standard index) -> mutable guard
```

The context follows the module's standard memory index space, including
imported and internally defined memories. An absent index returns `None`.
Imported indexes that bind the same `WasmMemory` retain one shared identity;
mutation through one index is visible through the other after its mutable guard
is released.

The context owns no guest bytes and performs no whole-memory copy. Its views are
tied to the synchronous callback lifetime, so host code cannot retain a pointer
after execution resumes. A mutable view also keeps the context exclusively
borrowed, preventing another indexed access until that guard is dropped. Shared
imported memory adds a runtime borrow check for aliased handles rather than
using `unsafe` to evade Rust ownership.

The existing `bind_import`, `bind_import_typed` and
`bind_import_typed_in_place` memory-zero forms remain source-compatible. They
are still appropriate when an embedding contract, such as TinyArcade core v1,
deliberately requires exactly one memory. Native module conventions may opt
into selected memory explicitly; a module import does not gain ambient access
to memory merely by existing.

Executable evidence covers both sides of the abstraction:

- a standard module with two internally defined memories exposes the exact
  requested index and rejects an absent index;
- two imported memory indexes bound to one host memory preserve alias identity
  across mutable and read guards;
- typed parameters/results still pass the existing post-callback type gate.
