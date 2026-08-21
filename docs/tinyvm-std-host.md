# tinyvm capability-based std host

Owner: [PRD 02.35](../prd/PRD_02_35_agenterm_tinyvm.md)

Status: implemented optional backend; Simulator wiring proven, physical iPhone open

The `std-host` Cargo feature provides one `StdHostBackend` inside the unified
`agenterm-tinyvm` crate. It implements the platform-neutral `HostBackend`
contract with `std` clocks and sleep, system random, bounded opaque resources
and `cap-std` directory capabilities. It does not change the default
`no_std + alloc` core or enable WASI implicitly.

## Ownership and path model

```text
embedding-chosen directory path
└── open_ambient_preopen (one explicit ambient operation)
    └── cap-std Dir stored behind opaque HostHandle
        └── HostContext virtual preopen, for example /save
            └── guest-relative path operations only
```

The embedding chooses a real directory and opens it once. After that point,
guest operations never receive a real path and the backend does not rebuild an
ambient path with string joins. `cap-std` resolves file operations relative to
the held directory capability on Unix, Windows and Apple platforms.

On iOS the embedding must supply an App-owned container directory such as a
Documents or Caches path. The backend does not discover a container, grant the
whole sandbox, expose process paths, spawn processes or add network access.

## Construction

```rust
use agenterm_tinyvm::{
    DescriptorRights, HostContext, HostLimits, StdHostBackend, StdHostLimits,
};

let mut backend = StdHostBackend::new(StdHostLimits { max_handles: 64 });
let native = backend.open_ambient_preopen(app_owned_directory)?;
let mut host = HostContext::new(backend, HostLimits::default());
let rights = DescriptorRights::PATH_OPEN
    .union(DescriptorRights::PATH_UNLINK)
    .union(DescriptorRights::READ)
    .union(DescriptorRights::WRITE)
    .union(DescriptorRights::SEEK)
    .union(DescriptorRights::STAT);
host.register_preopen(native, "/save".to_owned(), rights)?;
```

The backend resource limit and `HostContext` guest-descriptor limit are
separate, explicit bounds. Closing a guest descriptor drops its backend file or
directory resource. `exit` records an outcome for the embedding; it never
terminates the host process.

## Current evidence

`tests/std_host_backend.rs` runs a real macOS temporary-directory lifecycle:
preopen, create, write, seek, read, stat, close and unlink. A sibling-directory
escape attempt cannot modify its sentinel. A second case covers clocks, system
random, exit outcome and backend handle exhaustion. A WAT-compiled standard
WASI binary drives the complete filesystem lifecycle through tinyvm,
`WasiPreview1`, `HostContext` and `StdHostBackend` and verifies both guest memory
and the resulting host filesystem state.

The same library feature graph compiles for Linux musl, Windows GNU/LLVM and
arm64 iOS. The separate optional iOS WASI host runs the real path through a
booted Simulator container. Physical-iPhone container I/O remains a consumer
integration gate, not something a cross-compile or Simulator can prove.
