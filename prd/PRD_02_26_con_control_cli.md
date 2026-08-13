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
  catalog is `capture-pane`, `close-tab`, `close-window`, `list-commands`,
  `list-tabs`, `new-tab`, `perf-stats`, `reset-perf-stats`, `resize-window`,
  `screenshot-pane`, `select-tab`, `send-keys`, `send-mouse`, `send-paste`,
  `send-text`, `send-ui-keys`, `send-wheel`, `ui-snapshot`, `wait-tab-exit` and
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
  scrollback extent and pending-wait counts — so black-box journeys assert state
  instead of guessing timing.
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
  unrelated sessions or the window.
- [x] the control endpoint uses a fixed worker pool with bounded connection and
  request queues. Its multi-tab journey floods one PTY with oversized CSI
  parameters while issuing concurrent `capture-pane`, `list-tabs` and
  `perf-stats` calls, then proves both the noisy tab and an unaffected sibling
  remain controllable. GUI dispatch handles at most two requests per event
  callback and reposts Wake while backlog remains; public request/yield counters
  expose field evidence, while a deterministic queue test proves wake coalescing,
  the fixed batch limit and backlog reporting without scheduler timing assumptions.
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
