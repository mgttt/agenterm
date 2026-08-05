# Self-hosted development loop

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [x] a running AgenTerm can build and stage the next AgenTerm binaries
  without first terminating the development fleet
- [x] a successfully staged local `build.bat release` records both
  repository-local Cargo target inventories and reclaims `target-release/` and
  `target/` only after artifacts reach `dist/`. Its content-validated Script
  bootstrap worker executes outside Cargo output, so the Windows cleanup cannot
  be blocked by an in-target executable; dev and `release-fast` retain their
  incremental targets.
- [~] native Windows development builds now emit a strict versioned
  whole-root incremental manifest through the stable bootstrap worker used as
  `RUSTC_WRAPPER`, and consume it only after all seven artifacts stage. The
  producer freezes the before snapshot while Cargo's lock is observed,
  serializes parallel wrappers behind an invocation barrier, records exact
  touched roots, and fails closed on missing rustc activity, indirect state,
  identity drift, working sessions, or held locks. Isolated producer/consumer
  tests prove exact untouched-root deletion; two consecutive public builds
  prove stable-worker reuse and a no-rustc hot build authorizes zero root
  deletion. A live development tree has not yet presented an eligible obsolete
  whole root, so production root-count and byte-reclamation evidence remains
  open rather than inferred from the fixture.
- [~] v0.1.7 build inputs embed commit, dirty state, Cargo lock hash, artifact
  manifest hash, and profile without fabricating missing values; protocol and
  instance discovery expose running versus staged identity with
  `same|stale|incompatible|unknown` explanations and public fleet coverage.
  The UI ownership explanation is available through discovery, but a governed
  lifecycle-action surface and end-to-end upgrade qualification remain planned
- [x] public fleet discovery distinguishes same, stale, incompatible, and unknown running-versus-staged identity without guessing missing build fields
- [x] the split server/GUI restart path preserves server PID, tab IDs, PTYs
  and scrollback through version handshake, bootstrap, reconnect and rollback
  black-box evidence
  - [x] v0.1.9 ownership decision: a dedicated headless process owns
    session/runtime state and has no GUI surface; `agenterm.exe` without
    `server` is a replaceable client. The former separate `agenterm-server.exe`
    image alias is removed — authority is `agenterm server` (same PE, new
    process). Windows may lock that PE while Keep Server is active; stop the
    authority before replacing the image.
  - [x] renderer-neutral hello/bootstrap/delta DTOs and typed interaction
    adapters drive the ordinary GUI without duplicated PTY truth. Closing and
    replacing the GUI preserves the same server/tab/PTY marker, and the same
    GUI PID/HWND reconnects across a server epoch restart. Default launch,
    full workbench parity, previous-compatible-GUI rollback and zero-orphan
    cleanup are qualified.
  - [x] ordinary `agenterm.exe` starts or connects to the independent headless authority, acquires the exact interactive lease with an observable additive client-build identity, renders renderer-neutral tab/screen/composer DTOs, routes stable-ID selection/input/resize through the lease, acknowledges applied event positions, detaches without ending the server or PTY, and a replacement GUI recovers the same server PID, active tab and live terminal marker with PNG and orphan-free public evidence
    - [x] GUI-triggered Windows autostart now uses the same platform-facade
      breakaway/no-window contract as CLI autostart. The complete public
      replaceable-UI journey proves `Keep Server Running` survives an owning
      Script Job cleanup and reconnects the replacement GUI to the same
      server/session before explicit shutdown.
- [x] a replaceable GUI whose server disappears invalidates its stale client
  projection immediately, hides terminal input controls, remains locally
  closable without consulting the missing server, starts a bounded replacement
  server only when the configured endpoint has no listener, and reconnects the
  same GUI PID/HWND through a new causal server epoch
  - [x] the public `remote-ui-smoke` Rhai journey force-terminates the isolated
    server as a crash fault through unrestricted `std::process::kill`,
    observes disabled input, opens and cancels the native close confirmation
    while disconnected, requires GUI-owned automatic recovery rather than
    manually launching a server, verifies the new PID/epoch/lease, and finishes
    with zero orphaned processes
  - [x] recovery startup is throttled and never starts a competing server when
    the endpoint is already listening; an incompatible listener remains an
    explicit error instead of being replaced
  - [x] an explicit `shutdown`/`kill-server` records address- and PID-scoped
    intent before the listener exits; an attached GUI suppresses crash recovery
    for that exact server, while a later explicit launch clears the marker and
    can start normally
