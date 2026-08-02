# Optional component lifecycle (`agenterm-softmgr.exe`)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- Product boundary
  - [ ] `agenterm.exe` never downloads, updates, or resolves packages during
    startup; optional network and installation work stays in an explicit
    console sidecar
  - [ ] Bash, SSH, HTTP, SQLite, MCP, scripting runtimes, and future tools
    remain independently versioned sidecars so capability growth does not
    inflate GUI size or first-window latency
  - [ ] the GUI consumes only a small installed-component manifest and
    reports missing or incompatible components without silently fetching
    them
  - [ ] the long-term product may provide an AgenTerm package/application
    catalog and software-distribution market for runtimes, tools and optional
    applications; the market is a discovery and transaction layer over this
    lifecycle, not an authority embedded in GUI startup
  - [ ] `agenterm-rhai.exe` supplies automation primitives and typed package
    tooling contracts, while a future `agenterm-softmgr.exe` exclusively owns
    trust, signature verification, install transactions, rollback and
    elevation; untrusted package manifests are data, never executable policy
  - [ ] optional desktop products begin as Explorer-coexisting companion
    applications. A future shell-replacement mode is a separate high-risk
    product gate with an independent minimal watchdog, lease-bounded
    activation, crash rollback and an always-tested Explorer recovery path
- Supply chain and transaction
  - [ ] a signed, versioned manifest declares platform, component version,
    package class (`runtime`, `tool`, `application`, or `shell-integration`),
    stable role and entry points, URLs, byte size, SHA-256,
    signer/key identity, dependencies, capabilities, and minimum AgenTerm
    protocol/API version
  - [ ] downloads use a staging directory, bounded size/time, signature and
    hash verification, and no execution before verification
  - [ ] install and update use same-volume atomic promotion; interrupted or
    failed activation preserves the last known-good version
  - [ ] rollback, repair, inventory, provenance, and garbage collection are
    explicit commands with machine-readable results
  - [ ] user-scoped installation is the default; elevation is never
    implicit, and PATH or file-association mutation requires explicit
    consent
- Verification
  - [ ] black-box fixtures cover clean install, offline cache, corrupt
    archive, bad signature, incompatible manifest, interrupted promotion,
    rollback, concurrent invocation, and locked executable behavior
  - [ ] release metadata and size/startup gates report each sidecar
    independently; an optional component is not counted as GUI capability
    until its integration and failure-isolation tests pass
