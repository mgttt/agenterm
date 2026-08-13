# `agenterm-con` control protocol and public CLI

Parent: [Lightweight terminal host (`agenterm-con`)](PRD_02_23_agenterm_con.md)

This module owns the standalone host's automation surface: the public
`agenterm-con cli` command set, the private `ATC1` wire, the bounded JSON
contract, and the snapshot/screenshot evidence products. The workbench's
`agenterm cli` is a separate contract owned by
[command line](PRD_02_15_command_line.md); the two are deliberately not the same
surface.

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Boundary against `agenterm cli`

- [x] `agenterm-con cli` is a GUI-lifetime local control surface, not a client
  of the workbench. It has no server, Fleet, mux, session persistence, remote
  transport, or Agent permission model, and closing the GUI ends the endpoint.
- [x] shared verb spellings (`capture-pane`, `screenshot-pane`, `send-text`,
  `send-keys`, `wait-text`) mean the same product action in both CLIs. Where the
  standalone host cannot honor a workbench verb it omits the verb rather than
  offering a reduced impostor.
- The two CLIs do not share a wire: the workbench uses its documented public
  transport, while con uses the private `ATC1` frame described below.

## Public command set

- [x] `agenterm-con cli list-commands` is an offline, no-window and no-endpoint
  discovery surface. Its exact command set is owned by con's machine-readable
  capability contract and checked against the running executable in CI. The
  catalog is `cancel-pointer`, `capture-pane`, `close-tab`, `close-window`, `list-commands`,
  `list-tabs`, `new-tab`, `perf-stats`, `reset-perf-stats`, `resize-window`,
  `screenshot-pane`, `select-tab`, `send-keys`, `send-mouse`, `send-paste`,
  `send-text`, `send-ui-ime`, `send-ui-keys`, `send-wheel`, `ui-snapshot`, `wait-tab-exit` and
  `wait-text`.
- [x] the fixed GUI-lifetime local CLI uses stable `@N` tab IDs for `list-tabs`,
  `new-tab`, `select-tab`, `close-tab`, `capture-pane`, `screenshot-pane`,
  `send-text`, `send-paste`, `send-keys`, cell-addressed mouse
  press/release/move/click, `send-wheel`, bounded `wait-text`, bounded
  `wait-tab-exit`, logical-client `resize-window`, and GUI-lifetime
  `close-window`.
- [x] `perf-stats` and `reset-perf-stats` expose frame latency plus PTY
  drain/yield counters through the same public CLI for repeatable interactive
  profiling. The counter semantics belong to
  [24](PRD_02_24_con_terminal.md).
- [x] `ui-snapshot` publishes structured UI state — including composer bounds,
  text/focus/submission error, active-terminal IME preedit, scrollback extent,
  pending-wait counts, terminal clipboard-paste state/target/error, and the
  nullable window-scoped control pointer owner — so black-box journeys assert
  state instead of guessing timing or inferring cleanup only from a later
  failure.
- [x] `send-ui-ime enabled|disabled|preedit|commit` injects one bounded
  platform-neutral IME event through the current UI focus owner. Preedit text is
  capped at 64 KiB, its optional character cursor is range-checked, and terminal
  commit success is returned only after the complete PTY write; direct protocol
  and CLI decoding reject malformed action, cursor and payload state.
- [x] `send-wheel` reports the route actually taken (`zoom`, application mouse,
  alternate-screen cursor keys, or local scrollback), the notch count actually
  applied after bounds/clamps, and whether observable state or terminal input
  changed. Application and alternate-screen PTY write failures fail the request
  instead of returning a false-success receipt; physical wheel input remains
  best-effort across concurrent child exit.
- [x] `send-mouse` preserves its compatible `delivered` field and additionally
  reports the route actually taken (`application`, local selection, clipboard,
  or no-op) plus whether the gesture changed state or wrote application input.
  Application mouse write failures fail the CLI request, while physical pointer
  input remains best-effort across concurrent child exit and keeps gesture
  ownership through release.
- [x] a retained exited tab remains locally interactive: public control evidence
  captures its final pane, selects it, performs a local selection click, and
  routes wheel input to bounded scrollback without reopening or writing its PTY.
  Keyboard/text/paste remain explicit failures, so observation does not imply a
  live input channel.
- [x] direct route tests prove application mouse and alternate-screen wheel
  writes propagate a closed PTY instead of constructing success outcomes. A
  failed application report does not commit its last-reported cell or gesture
  ownership, preserving the next physical/control gesture's routing state.
- [x] press/release control gestures have one window-scoped tab owner. A second
  press, cross-tab move/release, or click during an open gesture fails
  explicitly; selecting, closing, or replacing the active tab cancels the old
  gesture and releases native capture. Native capture loss is consumed at the
  window owner before active-session dispatch, so a background control owner
  cannot survive an OS cancellation. Public evidence proves a stale release
  after `select-tab` cannot revive the cancelled selection.
- [x] `cancel-pointer` is an idempotent window-scoped recovery command for an
  automation client that dies or times out between press and release. It
  cancels the control owner and active physical gesture through the same path
  as native capture loss, returns the nullable cancelled owner, releases native
  capture, and makes a later unmatched release fail explicitly.
- [x] public `wait-text` delegates exact per-visible-row UTF-8 containment to one
  allocation-free control kernel. It preserves the existing authority: no row
  joining, newline insertion, cross-row match, hidden-scrollback scan, Unicode
  normalization, or case folding, and an empty needle still matches a visible
  row. x86_64 uses a bounded inline-assembly byte loop while Windows aarch64 and
  Unix use the same scalar contract; matrix, CJK and emoji oracle tests prove
  parity with byte-window search.
- [x] stable `@TAB_ID` values from list, new, select and close pass through one
  concrete non-inlined optional-id formatter, preserving the exact string/null
  JSON schema; ids remain `u64` workspace identities carried as typed values in
  the exact `"@N"` grammar until final serialization, and nullable parents remain
  JSON null.
- [x] fixed-schema unsigned CLI values share one 93-byte allocation-free ASCII
  decimal parser with checked overflow and target-width conversion. `u64`,
  `usize`, `u16` and `@TAB_ID` preserve leading-plus, leading-zero, invalid,
  overflow and existing per-flag error behavior; signed `i16` deliberately
  remains on standard `FromStr`.
- [x] the CLI cursor borrows argument text from the process-owned `String` slice
  and allocates only when a value enters an owned command field. Verbs, flags,
  numeric values, tab ids and mouse tags do not clone on every cursor advance;
  syntax, error text, stable tab ids and wire bytes are unchanged.
- [x] native shell parsing intentionally does not claim equivalence for
  ambiguous hand-crafted quote sequences. Standard launcher quoting, the offline
  CLI, `-e` passthrough and GUI-lifetime control startup are the supported
  evidence.

## Transport and bounds

- [x] the CLI connects only to an explicitly configured local pipe or
  Unix-socket endpoint. A platform-owned `IpcEndpoint::from_native_address`
  constructor accepts only those named-pipe/Unix-socket mechanisms without
  routing through the generic TCP authority parser or endpoint formatter.
- [x] the private transport is a versioned, length-prefixed typed `ATC1` frame
  rather than a nullable JSON envelope. Opcodes and field order are fixed;
  invalid UTF-8, values, lengths, trailing bytes, or versions fail only that
  request.
- [x] mouse action and button wire tags have one non-generic enum-owned mapping
  shared by `ATC1` encode and decode. Opcode 10, all numeric tags, unknown-tag
  failures and the move/none pairing rule remain byte compatible.
- [x] requests and responses are size and time bounded. A malformed request or a
  failed child may only fail that request or session; it never terminates
  unrelated sessions or the window. `send-text`, `send-paste`, `send-keys` and
  terminal-routed `send-ui-keys` reject a retained exited tab with
  `terminal process has exited`; direct text/paste and each encoded key PTY
  write failure are returned to the CLI rather than reported as successful
  delivery. Physical keys remain best-effort across concurrent child exit, and
  local shortcuts, IME suppression and viewport scrolling remain successful
  without pretending they emitted PTY bytes. Capture, local selection and
  scrollback remain available after exit.
- [x] terminal paste normalizes and frames before one checked write, then snaps
  to the live viewport only after that write succeeds. A closed PTY preserves
  the caller's scrollback position instead of presenting a failed paste as a
  delivered input-side state change.
- [x] `send-paste` remains deterministic direct-payload injection, while
  terminal-routed `send-keys` or `send-ui-keys` with `Ctrl+Shift+V` queues the
  same bounded asynchronous OS clipboard read as physical input. A second read
  fails explicitly while one is pending, and completion cannot retarget itself
  after tab or focus changes.
- [x] the control endpoint uses a fixed worker pool with bounded connection and
  request queues. Its multi-tab journey floods one PTY with oversized CSI
  parameters while issuing concurrent `capture-pane`, `list-tabs` and
  `perf-stats` calls, then proves both the noisy tab and an unaffected sibling
  remain controllable. GUI dispatch handles at most two requests per event
  callback and reposts Wake while backlog remains; public request/yield counters
  expose field evidence, while a deterministic queue test proves wake coalescing,
  the fixed batch limit and backlog reporting without scheduler timing assumptions.
  One exception remains bounded and order-preserving: when the ordinary batch
  contains only `resize-window`, it absorbs the immediately contiguous resize
  tail and submits the last geometry once while replying to every caller. It
  never crosses input, screenshot, wait, or other command work.
  Saturation reports `control server is busy`, while a closed GUI reports
  `terminal window is closing`; both paths drop the rejected reply owner
  immediately instead of consuming the GUI response timeout.
- [x] closing a tab immediately cancels its pending text/exit waits and
  screenshot reply with a typed target-close error. `ui-snapshot` exposes
  pending counts, so a black-box journey proves registration, cancellation,
  worker release and clean final-host exit without timing guesses.
- [x] pending text/exit deadlines have a fixed ten-minute upper bound. A larger
  syntactically valid `u64` timeout fails only that request, preserves its reply
  owner for the normal dispatch error path, and registers no latent wait instead
  of occupying one of the 32 bounded slots for an effectively unbounded period.
- [x] the GUI handoff preserves concurrent request workers without linking two
  generic channel instances: a mutex-owned FIFO carries requests and a one-shot
  Condvar slot carries each reply. Closing the GUI atomically rejects new queue
  entries, drops pending senders and wakes reply waiters rather than making them
  consume the full timeout.
- [x] six synchronous control commands share non-generic session lookup and
  terminal-cell validation helpers while preserving each command's local
  `Result` propagation, screenshot and wait reply ownership, and active-tab
  ordering.
- [x] con reader/waiter, control listener/request and the Windows ConPTY output
  pump submit boxed tasks through one non-generic named-thread trampoline
  instead of repeated product glue. Thread names and spawn failures remain
  observable, and a task panic remains contained by the unwind-enabled
  `JoinHandle` boundary.

## JSON contract

- [x] configuration, scripted interaction, atomic snapshots and public JSON
  output share one strict bounded JSON codec instead of four serde/ad-hoc paths.
  It accepts UTF-8, JSON escapes and valid surrogate pairs while bounding input
  bytes, nesting, nodes, object fields and strings; duplicate keys, isolated
  surrogates, malformed or non-finite numbers and trailing data fail locally.
  `serde_json` remains a dev-only interoperability oracle and is absent from the
  production graph.
- [x] the bounded JSON object constructor owns a field `Vec` at one non-generic
  boundary instead of monomorphizing its complete iterator/collection path per
  array length; all snapshot and control-result construction sites retain the
  same schema and ordering.
- [x] eleven fixed one-field control replies share one concrete non-inlined JSON
  constructor instead of repeating the generic object path at every command and
  wait boundary, with field names, values and ordering byte compatible.
- [x] fixed schema keys encode as borrowed static literals rather than
  allocating a `String` per field before immediate serialization. Dynamic
  terminal titles, captured text and paths remain owned values, and the typed
  configuration parser never retains arbitrary input keys.
- [x] fixed-schema numeric values remain typed `u64`/`i64` until the final
  response buffer instead of allocating a decimal `String` per field. Arbitrary
  raw decimal text is test-only, while fractional configuration keeps its
  dedicated bounded parser. Con uses its direct `itoa` dependency for final
  formatting.

## Evidence publication

- [x] screenshot success is returned only after atomic PNG output has crossed
  the platform durability barrier.
- [x] every asynchronous PNG completion path shares one panic-containment
  boundary, including worker initialization failure and queue-full fallback on
  the caller thread; a faulty completion cannot unwind into GUI dispatch or
  terminate the fixed encoder worker.
- [x] screenshot ownership is global and bounded across the pending-render and
  background-encode phases. PNG encoding and atomic publication run on one fixed
  worker rather than the GUI thread, and concurrent cross-tab requests receive a
  typed busy result instead of switching active tabs and stranding earlier
  replies. Ownership is global rather than attached to active-tab rendering:
  capture temporarily selects its target only inside render, then restores the
  latest user-selected tab. The public four-tab flood journey proves exactly one
  owner, bounded completion, valid PNG publication, stable active context across
  a select/capture race, and zero retained requests afterward. The matching
  present-side rule for the discarded scratch frame is owned by
  [24](PRD_02_24_con_terminal.md).
- [x] the screenshot writer is one platform-owned contract with target
  adapters: Windows passes a validated clipped pointer and original XRGB stride
  directly to the system GDI+ PNG codec, while Linux/macOS retain the portable
  Rust PNG encoder. The Windows production graph no longer contains a private
  stored-DEFLATE, Adler-32, IEEE CRC-32, 64 KiB block buffering, or XRGB-to-RGB
  copy path. The independent `png` dev decoder owns color/format
  interoperability and the GUI black box owns rendered screenshot evidence.
- [x] snapshot and screenshot publication use platform-owned writer/path atomic
  file contracts: an exclusively created sibling is filled, revalidated as a
  regular non-link file, synchronized, then replaced with Windows
  `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)` or Unix rename plus
  parent-directory fsync. Concurrent readers observe only a complete old or new
  value; pre-publication failures remove the sibling, while a post-replacement
  durability failure explicitly reports that publication already occurred.
- [x] publication distinguishes arbitrary caller-owned staging paths from
  sibling temporaries exclusively created by the platform. The public path keeps
  physical-parent, symlink and distinct-entry validation; the owned sibling path
  revalidates the completed regular file and destination type without
  rediscovering the same canonical parent before replacement. The Windows
  adapter passes those prepared paths directly to `MoveFileExW` with
  write-through and bounded sharing-violation retries instead of canonicalizing
  both paths again. Owned writers freeze their destination through a platform
  facade distinct from the public caller-owned publisher: Windows uses bounded
  `GetFullPathNameW` plus `GetFileAttributesW` directory validation before
  creating the sibling temporary, while Unix retains canonical parent
  resolution.
- [x] test-only journey JSON invokes the public commands and is not linked into
  the product.
