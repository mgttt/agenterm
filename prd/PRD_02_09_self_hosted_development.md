# Self-hosted development loop

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [x] a running AgenTerm can build and stage the next AgenTerm binaries
  without first terminating the development fleet
- [~] v0.1.7 build inputs embed commit, dirty state, Cargo lock hash, artifact
  manifest hash, and profile without fabricating missing values; protocol and
  instance discovery expose running versus staged identity with
  `same|stale|incompatible|unknown` explanations and public fleet coverage.
  The UI ownership explanation is available through discovery, but a governed
  lifecycle-action surface and end-to-end upgrade qualification remain planned
- [x] public fleet discovery distinguishes same, stale, incompatible, and unknown running-versus-staged identity without guessing missing build fields
- [ ] define and prototype a split server/GUI restart path that preserves
  server PID, tab IDs, PTYs and scrollback; do not claim GUI-only upgrade until
  the version handshake, bootstrap, reconnect and rollback black-box passes
  - [x] v0.1.9 ownership decision: a dedicated internal
    `agenterm-server.exe` owns session/runtime state and has no GUI surface;
    `agenterm.exe` is a replaceable client. This separate image is intentional:
    Windows must not keep the replaceable GUI executable locked merely because
    the stable server is alive.
  - [ ] extract the current combined state machine behind renderer-neutral
    server snapshot/delta and typed command adapters before moving ownership;
    no duplicated tab/PTY truth is allowed during transition.
