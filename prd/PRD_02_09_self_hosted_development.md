# Self-hosted development loop

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [x] a running AgenTerm can build and stage the next AgenTerm binaries
  without first terminating the development fleet
- [ ] v0.1.7 exposes running server/UI-host build identity beside staged
  `agenterm.json`, explains when an old process still owns the visible UI, and
  offers only lifecycle actions whose PTY continuity is truthful
- [ ] define and prototype a split server/GUI restart path that preserves
  server PID, tab IDs, PTYs and scrollback; do not claim GUI-only upgrade until
  the version handshake, bootstrap, reconnect and rollback black-box passes
