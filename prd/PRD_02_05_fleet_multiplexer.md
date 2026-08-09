# Fleet multiplexer (`agenterm cli mux`)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- Architecture
  - [x] tmux/RMUX-compatible fleet control entry point
    (`agenterm cli mux`; no separate `agenterm-mux` PE)
  - [x] thin console frontend over AgenTerm IPC; internal
    `agenterm server` (main-program subcommand) is the PTY/workspace authority, while the replaceable
    `agenterm.exe` GUI is only a client. Mux consumes the same typed operations
    and does not depend on GUI presence.
  - [x] automatically discovers the live AgenTerm instance from the tab
    environment, with explicit `--address` and `--session` overrides
  - [x] server bind, inherited addresses, and explicit client overrides are
    centrally restricted to numeric loopback IPs
  - [x] if no server exists, server-start behavior is explicit and mirrors
    supported tmux/RMUX semantics without creating a hidden second fleet
  - [~] shared parser and command catalog with `agenterm cli`; mux aliases map
    to typed internal operations, not shelling out to `agenterm cli`
- Compatibility surface
  - [x] sessions map to AgenTerm workspaces and windows map to tree tabs;
    one tab remains one pane until split panes are genuinely implemented
  - [~] support tmux/RMUX aliases, `-t` targets, `-F` formats, stable IDs,
    exit codes, stdout/stderr separation, and unsupported-command errors
  - [x] expose shipped native AgenTerm tree, composer, screenshot, wait, and
    agent extensions under an unambiguous namespace
  - [ ] expose future scripting commands through that same native namespace
    without masquerading as tmux features
  - [x] `agenterm cli` remains the richer machine API; `agenterm cli mux` is the
    compatibility UX and migration path (no separate mux PE)
- Conformance
  - [x] machine-readable compatibility matrix generated from the command
    registry and exposed through `agenterm cli mux compatibility`
  - [~] black-box argv/output/exit-code corpus runs against AgenTerm and,
    where practical, reference tmux and RMUX versions
  - [x] behavioral differences are explicit, especially persistence,
    process ownership, confirmation, single-pane tabs, and server lifetime
  - [ ] function-key, mouse, nested RMUX, and Byobu-style flows remain in
    the public regression suite
