# Terminal runtime

Parent: [AgenTerm product tree](../PRD.md#product-tree)

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

- [x] Win32/GDI window without GPU or OpenGL requirements
- [~] Linux/macOS GUI window without GPU requirements via `winit` +
  `softbuffer` software raster (shared theme/geometry/selection/vt100);
  Linux/macOS share `unix_app`: live POSIX PTY tabs, terminal workbench toolbar,
  composer, settings, wheel/scrollbar, paste, and word/row/drag selection
  with edge autoscroll; status-bar CWD editor, window-close confirm, and tabs
  resize grip on Unix; proxy editor and professional selection remain later
- [x] one ConPTY-backed process per tab on Windows; the platform adapter directly
  owns ConPTY pipes, process/job lifecycle, resize, wait and native console input
- [x] shared PTY backend facade: Windows keeps those implementation details behind
  one adapter; Unix uses POSIX `openpty` + fork/exec; `terminal_runtime` consumes
  one API
- [x] VT100 parsing, ANSI colors, scrollback, resize, keyboard and mouse
- [x] Backspace emits ConPTY VT `DEL` and deletes exactly one input
  character in the default `cmd.exe` line editor
- [x] mouse wheel navigates ordinary terminal history and raw full-screen applications;
  a visible draggable scrollbar navigates ordinary history, track clicks page,
  and dragging to the bottom restores the
  live viewport. Live v0.1.12 dogfood found alternate-screen harnesses whose
  zero local scrollback makes wheel/PageUp ineffective. Byte-level diagnosis on
  pre-passthrough ConPTY proved that `1049h/l` is erased and replaced by an
  indistinguishable full-frame repaint, so `alternate_screen=false` is not an
  authoritative normal-screen fact and repaint/max-scrollback heuristics are
  forbidden. The Windows PTY facade now queries typed child input ownership:
  cooked line input consumes no wheel key, while RawVt/RawNative receives native
  logical Up/Down records and ConHost itself selects CSI/SS3 from its retained
  cursor mode. Linux/macOS keep parser-owned alternate-grid byte input. The
  owning Windows journey retains ordinary scrollbar/wheel evidence and uses a
  real raw full-screen PowerShell PTY; native up/down wheel messages each arrive
  as three complete `ESC O A` / `ESC O B` sequences. The integrated 169.9-second
  journey passed selection, recovery and orphan cleanup as well. Future
  application raw-mouse reporting and Shift local-selection override remain a
  separate professional-input slice rather than weakening this shipped paging
  contract.
- [x] basic Windows visible-cell dragging selects terminal text and a completed
  non-empty selection owns Ctrl+C Copy. Prepared/dragging/completed state,
  exact native capture ownership and paint-owned highlight pixel bounds are
  projected together in `ui-snapshot`; paint consumes the same bounds. The
  owning journey advances the PTY generation after pointer down, observes
  prepared/dragging capture, proves a same-event-position PNG change, releases
  capture on completion, and verifies direct Ctrl+C updates the clipboard
  without adding an ETX byte to the PTY. System-menu Copy remains equivalent.
  Capture acquire/release/query failures clear copyability and surface a typed
  error instead of pretending selection completed. A click that never drags
  remains non-copying and available to existing terminal/RMUX behavior.
- [x] window-icon system menu exposes focus-aware Copy and Paste: native
  edit controls receive their standard messages, while terminal Copy uses
  the active cell selection and terminal Paste uses the active PTY
- v0.1.8 professional-selection slice (P0), informed by the reviewed PuTTY
  terminal model
  - [ ] professional selection extends the shipped basic state machine with
    every tab/modal/shutdown/capture-loss cancellation surface and public
    physical evidence; a click that never becomes a drag retains its existing
    terminal/RMUX click behavior
  - [ ] while an owned drag remains above or below the terminal viewport, the
    GUI-owned timer scrolls at a bounded rate, clamps every endpoint to a valid
    terminal cell, and stops immediately on completion or cancellation
  - [ ] capture loss, tab change, modal opening, terminal replacement, and
    window or server close cancel an unfinished gesture without leaving mouse
    capture, timer activity, input ownership, or suspended rendering behind
  - [ ] double-click selects a Unicode-aware terminal-cell word with an
    explicit punctuation table; triple-click selects one visible terminal row,
    not a logical line joined across automatic wrapping
  - [ ] forward and reverse endpoints, wrapped and multiline text, CRLF copy,
    CJK double-width cells, and wide-cell continuations normalize to the same
    bounded cell selection and clipboard result
  - [ ] physical-input public tests cover drag-outside auto-scroll,
    double-click, triple-click, capture loss, tab change, forward/reverse CJK
    selection, and concurrent PTY output; pure tests own endpoint, word,
    visual-row, continuation-cell, boundary-clamp, and timer progression
  - [ ] input, resize, ANSI, CJK, wide-character, scaling, minimize/restore,
    scrollbar, and long-output qualification proves selection, cell dump,
    bounded capture, `ui-snapshot`, and PNG describe the same visible cells
    and that PTY output continues while selection and auto-scroll are active
- Professional-selection non-goals for v0.1.8
  - [ ] application-requested raw mouse arbitration and its documented Shift
    local-selection override remain a later independently accepted slice
  - [ ] rectangular selection remains later work; v0.1.8 does not infer it
    from word, visual-row, or drag selection
- [x] terminal paste reads bounded Unicode clipboard text off the GUI thread,
  normalizes newlines, filters unsafe controls, and honors bracketed-paste mode.
  The Windows public `remote-ui-smoke` proves ordinary asynchronous delivery and
  exact `ESC[200~...ESC[201~` PTY bytes; Unix uses the same framing helper and
  rejects stale tab/focus/modal completions instead of pasting into a new target.
  The reusable Linux/macOS adapters preserve caller deadlines and stable
  `Unsupported`/`Failed` clipboard causes. The matching-host Unix workbench
  journey owns native clipboard-to-PTY delivery, `terminal.pasted`, and delayed
  stale-target cancellation. Exact-SHA `b4f1622` CI run `30724960474` passed
  that complete journey on Linux x86_64 and both macOS architectures.
- [x] dirty-frame rendering and GDI double buffering exist; live v0.1.12
  dogfood reports sustained terminal-content and native-frame flicker. White-box
  analysis found that the replaceable Windows GUI cleared and repainted directly
  on the window HDC and treated lease heartbeat as visible change. The current
  repair makes lease maintenance non-visual by type and composes a complete
  client frame in a compatible memory DC before one `BitBlt`, with bounded
  dimensions. Back-buffer allocation or presentation failure is a typed native
  error that closes the affected replaceable window; it does not silently fall
  back to partial direct painting and reintroduce the flicker path. Same-grid
  and duplicate in-flight resize requests are now suppressed across the typed IPC boundary, keyed by
  server epoch and stable tab ID, and redundant class-wide resize redraw flags
  are removed. White-box comparison with the pre-platform-extraction host then
  found a concrete regression: the new parent window had lost
  `WS_CLIPCHILDREN`, so every full-client `BitBlt` could overwrite native
  EDIT/BUTTON pixels before each child repainted. The platform host again clips
  child HWND regions, and unchanged child bounds/visibility now skip redundant
  `MoveWindow`/`ShowWindow` paint churn. Style and geometry contracts cover both
  invariants. A same-window/same-modal synchronous screenshot A/B measured
  Dark/Light at 528/572 ms and 663/553 ms; Light is not a distinct 4x paint
  path. The full smoke applies Light immediately before its IPC-heavy CWD,
  hierarchy, dense-tab, 80-line scroll, selection, and recovery half, explaining
  a strong visual correlation without dismissing remaining temporal flicker.
  Timestamp reconstruction then confirmed the user's observation precisely:
  three runs spent 15.0--18.3 seconds before Light and 108.6--122.5 seconds
  afterward, while like-for-like snapshot intervals rose about 1.8--2.1x. The
  dominant cause was not the palette but the smoke harness reparsing and
  pretty-rewriting its entire growing `commands.json` after every CLI call, an
  O(n²) recorder whose per-50-command median grew from 213--243 ms to
  815--1000 ms. Command evidence now appends one bounded JSONL record, keeps a
  bounded immediate checkpoint, and seals one compact schema-compatible JSON
  array at cleanup. Explicit observed-sequence barriers replace accidental
  delays the old logger had hidden. The same complete journey now passes in
  36.787 seconds versus 169.9 seconds before, a 4.62x improvement, while still
  applying Light and retaining all 15 evidence IDs.
  Focused structural tests pass. The native host now exposes monotonic redraw,
  parent-paint, child-layout, and child-visibility counters through an explicit
  test-only sample message; the sample is latched into `ui-snapshot` so observing
  it cannot form a repaint feedback loop. The owning Windows journey sampled 19
  native z/Z operations at 23 redraw requests and 8 parent paints, with zero real
  child bounds/visibility updates and increasing no-op coalescing counts. A
  subsequent 500 ms idle observation measured one redraw and one parent paint,
  again with zero child updates. The existing Light-theme 80-line PTY burst now
  waits until the GUI lease has observed the server position and then samples
  after a 250 ms paint-queue settle; it measured four redraw requests and four
  parent paints with zero child updates. This closes the automated idle, zoom,
  and high-output repaint-storm diagnostics; sustained high-output visual
  dogfood on the new binary was accepted by the user as the v0.1.12 visual
  result; future visual regressions remain ordinary maintenance work.
  The clean `78eac9e` dev artifact repeated the complete owning journey in
  63.4 seconds: counterbalanced Dark/Light totals were 1349/1237 ms with
  identical 10 redraws and 8 paints, zoom measured 23/7, 500 ms idle 1/1,
  and the Light-theme high-output burst 3/3. The journey continued through
  selection/copy, ordinary and bracketed paste, GUI detach/reconnect to the
  same server/PTY, server recovery, and orphan-free explicit shutdown. This
  strengthens the automated temporal evidence without substituting for the
  outstanding sustained-output visual acceptance.
- [x] ordinary terminal keys and modifiers are encoded for the active PTY;
  live v0.1.12 dogfood found `Shift+Tab` dropped while terminal focus was
  active. The current repair introduces one shared xterm named-key modifier
  encoder for Tab, navigation, Insert/Delete, paging and F1–F12; Unix preserves
  normalized modifiers and Windows owns the matching virtual-key plus
  WM_KEYDOWN/WM_CHAR de-duplication path. Unit contracts cover shared bytes and
  Windows mapping. The owning Windows journey now sends a Shift+Tab window
  shortcut while terminal focus is active and observes exactly three additional
  bytes at the public pane boundary; the byte contract fixes those bytes as
  `ESC [ Z`. This is deliberately automation rather than physical-key evidence:
  Win32 `GetKeyState` reports the modifier state associated with keyboard input
  retrieved by the target thread, `SetKeyboardState` changes only the caller's
  input-state table, and `SendInput` targets the global foreground input stream.
  Taking foreground focus would violate the smoke-wide `AGENTERM_NO_ACTIVATE=1`
  contract. A real keyboard Shift+Tab in the latest dogfood binary was accepted
  by the user as the v0.1.12 human result. The owning journey now repeats that
  exact GUI Shift+Tab route after
  18 native z/Z operations and settled PTY geometry, requires exactly three
  additional input bytes, and then continues through a live shell marker,
  selection/copy, paste and detach/reconnect. This closes the combined
  focus/resize/GUI-dispatch regression without mislabeling synthetic input as a
  physical-key receipt.
- [~] Windows terminal focus survives immediate native toolbar actions. Live
  dogfood found the font `z/Z` child buttons retained Win32 keyboard focus while
  the terminal input path accepts keys only for the top-level HWND. Font, locale
  and Tabs actions now restore the terminal HWND; actions that open a modal or
  Control Center deliberately do not. Native focus automation reports child
  control focus as neither terminal, composer nor Tabs, and the owning remote UI
  smoke checks focus immediately after the font click. GDI painting restores the
  previously selected font/background mode before an old RAII font can be
  destroyed. The complete replaceable-UI smoke then continues through PTY input,
  font inheritance, GUI detach, same-server/session reconnect and explicit Stop
  Server cleanup. A deeper dogfood failure was also found: one transient native
  PTY resize error used to poison the terminal's fatal I/O state, so all later
  input was rejected while the GUI remained alive, and the server nevertheless
  published a false successful resize. Resize now returns a typed failure,
  commits parser geometry and the resize journal only after native acceptance,
  and leaves the terminal writable after rejection. Remote PTY resize is now
  serialized by an owned worker with a latest-only pending slot, so the Win32
  event thread never waits on the bounded IPC round trip and stale
  lease/epoch/tab/grid results are discarded. Selection PNG evidence also waits
  boundedly for the independently scheduled `WM_PAINT` after structured state
  reaches `dragging`; unchanged pixels for the whole deadline still fail. This
  prevents a published-state/paint race from weakening the requirement that a
  live selection is visibly highlighted.
- [x] GUI shell appears before the initial ConPTY/cmd process is ready
- [x] initial terminal loads asynchronously with visible starting feedback
- [x] exited process retains its final screen and exit code
- [~] `agenterm-con` evolves as a single-GUI-process, lightweight terminal
  host: each tree tab owns an independent PTY, parser, viewport and failure
  state; closing a parent promotes its direct children instead of terminating
  them. It is explicitly not a mux, persistent workspace, Fleet authority or
  script runtime. Its fixed GUI-lifetime local CLI uses stable `@N` tab IDs for
  `list-tabs`, `new-tab`, `select-tab`, `close-tab`, `capture-pane`,
  `screenshot-pane`, `send-text`, `send-keys`, cell-addressed mouse
  press/release/move/click, `send-wheel`, and bounded `wait-text`. The CLI
  connects only to an explicitly configured local pipe or Unix-socket endpoint;
  its private transport uses a versioned, length-prefixed typed frame rather
  than a nullable JSON envelope. Opcodes and field order are fixed; invalid
  UTF-8, values, lengths, trailing bytes, or versions fail only that request.
  Requests and responses are size/time bounded, screenshot success is returned
  only after atomic PNG output has crossed the platform durability barrier, and
  a malformed request or failed child may
  only fail that request/session, never terminate unrelated sessions or the
  window. The owning suite now includes 88 binary unit tests plus a Windows
  public-CLI black-box journey covering isolated parent/child PTYs, raw text,
  key, mouse and wheel delivery, bounded wait failure, capture isolation,
  renderer-owned PNG evidence, parent promotion and orphan-free cleanup.
  PTY delivery uses a platform-owned fixed 1 MiB byte ring per session instead
  of allocating a `Vec` for every native read. Each read commits atomically or
  waits for capacity; close wakes blocked producers while preserving committed
  tail bytes for draining. Parsing remains bounded to 128 KiB per GUI turn;
  reader wakes are coalesced, inactive tabs are drained without forcing
  unrelated active-tab paints, and remaining backlog yields to input before
  self-scheduling another turn. `perf-stats` and
  `reset-perf-stats` expose frame latency plus PTY drain/yield counters through
  the same public CLI for repeatable interactive profiling. The same counters expose conservative full/partial raster-candidate frames, dirty/frame pixels, platform-owned native-present count, latency, requested/completed pixels, and host direct/copy frame and pixel counts. Vendored `vt100` now emits allocation-free conservative row damage from mutation sites; exact visible-cell comparison is its collision-free test oracle, while unknown callbacks, viewport changes, resize and alternate-screen transitions fail safely to full. PTY Wake drains this evidence before invalidation, so ordinary output requests only the affected terminal rows and old/new cursor overlays instead of first invalidating the complete Windows client. The pixel-window contract identifies retained versus transient host backing and requires each frame to commit `None`, `Full`, or a bounded partial rectangle. Windows con rasterizes directly into the retained native XRGB buffer, forces full raster after allocation/resize/DPI invalidation, and removes the former product-to-host full-frame copy; Unix/macOS retain the product-owned bounded frame and full-copy it into explicitly transient softbuffer frames. Windows maps typed physical damage to `InvalidateRect` and uses `PAINTSTRUCT.rcPaint` for top-down `StretchDIBits` partial present. The native ledger times `StretchDIBits` or softbuffer `present` itself without a GUI-thread lock. Windows pairs every successful `BeginPaint` with exactly one `EndPaint`, rejects short scanline copies, and never presents a renderer error; Unix catches application callback panics at the event-loop boundary and converts them to typed failure. A paired Windows release probe measured seven idle partial frames at 895 us average before direct backing versus eight at 360 us after it (59.8% lower); a 50-step send/wait journey measured 244 frames at 1,310 us average versus 250 at 992 us (24.3% lower), with all 250 new frames direct and zero copied frames/pixels. A post-row-damage Windows release probe produced 33/33 partial raster candidates with 70,560 dirty pixels over 17,476,272 frame pixels (about 0.40%), 33/33 successful native presents, a 1,589,501-byte PNG and captured command text before explicit `@1` close. The native ledger separately reported 529,584 full-expose pixels and 70,560 partial pixels, preserving OS expose authority rather than relabeling it as product damage. Earlier pre-row-damage probes observed 2/5 partial candidates at about 60.0% dirty pixels for blink/idle, falling to 2/13 and about 84.6% after mixed PTY output; those figures remain historical directional evidence, not release qualification. The Win32 native host now keeps `GWLP_USERDATA` behind a reentrancy-checked owner, defers callback-issued synchronous User32/IMM commands until application and framebuffer borrows end, snapshots stateful reentrant messages without retaining pointer-backed parameters, validates and reschedules nested paint, and fails closed on bounded-queue overflow or nonconvergence. The main control-window host now consumes the same platform-internal bounded queue contract with per-HWND userdata instead of a thread-global raw `(WPARAM, LPARAM)` backlog, so native FFI experience is shared without merging product event policy. Platform all-feature tests pass 313/313 after the migration. The native pixel host then consolidated seven source-level unwind sites to three explicit boundaries and replaced duplicate stateless/stateful matchers with one typed message classification; the aborting release-fast build fell from 622,080 to 621,568 bytes, entirely through a 512-byte raw `.text` reduction while `.rsrc` stayed unchanged. That artifact could not satisfy the claimed containment contract because Cargo test used unwind while every delivery profile inherited `panic = "abort"`. The official build now gives con its own `con-dev`, `con-release-fast`, and `con-release` unwind dependency graphs, merges only the resulting executable into the ordinary staging directory, and leaves the workbench profiles aborting. The staged source, merged profile, and `dist` bytes are identical; release-fast unwind is currently 849,920 bytes and passes 87 unit tests, 16 black-box tests with two existing ignores, one isolated multitab control test, Clippy, aarch64 compilation, and a release-profile synthetic panic containment test. The official con build now pins `rust-src` and uses an explicit target plus a subprocess-scoped Rust 1.97 build-std boundary with `backtrace-trace-only`; it retains unwind, with a 790,016-byte custom-std baseline and a current 790,528-byte GDI+ screenshot artifact. Exact-profile tests, x64 Clippy, and Windows aarch64 compilation pass; all-six-platform Candidate and sealed-byte reproducibility remain release-gate evidence rather than local claims. The 512 KiB target remains active and must be recovered without reverting to abort. A public 100-title OSC stress completed 883/883 direct frames with zero host copies and zero present failures while producing a valid 1,589,501-byte screenshot. Its local chrome
  now owns a vertically scrollable left tree with row-level close targets and
  top `z`/`Z` font controls, plus a distinct bottom composer input and send
  action. The default 15 logical-pixel terminal font corresponds to roughly
  11.25 pt at 96 DPI and is no smaller than the tree labels. While focused,
  the composer owns Space and all keyboard events instead of leaking ignored
  keys into the PTY; `Ctrl+A/C/V/X` provide select-all, copy, bounded single-line
  paste and cut semantics. The host chrome defaults to high-contrast
  black/white/gray and the terminal default foreground is near-white on black;
  explicit ANSI application colors remain intact. Geometry, iterative typed tree-depth resolution, tree viewport
  bounds and hit results are pure deterministic contracts covered independently
  of Win32/PTY state; an out-of-range hit or scroll safely becomes background or
  a bounded no-op. A feature-gated native Win32 pixel host now proves the
  platform boundary can remove winit/softbuffer from the linked con path without
  changing product state. The independently owned `agenterm-con` package now
  selects that host by default on Windows while Linux/macOS select the portable
  host; its x86_64 release PE is currently 585,216 bytes versus 1,046,528 bytes
  for the original portable host. Its screenshot writer is one platform-owned
  contract with target adapters: Windows passes a validated clipped pointer and
  original XRGB stride directly to the system GDI+ PNG codec, while Linux/macOS
  retain the portable Rust PNG encoder. The Windows con production graph no
  longer contains its private stored-DEFLATE, Adler-32, IEEE CRC-32, 64 KiB
  block buffering, or XRGB-to-RGB copy path. The independent `png` dev decoder
  owns color/format interoperability and the GUI black box owns rendered
  screenshot evidence. Snapshot and screenshot publication use platform-owned
  writer/path atomic-file contracts: an exclusively created sibling is filled,
  revalidated as a regular non-link file, synchronized, then replaced with
  Windows `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)` or Unix rename plus
  parent-directory fsync. Concurrent readers observe only a complete old or new
  value; pre-publication failures remove the sibling, while a post-replacement
  durability failure explicitly reports that publication already occurred.
  The shared native-codec replacement changes the unwind/trace-only Windows
  release-fast PE from 790,016 to 790,528 bytes: one 512-byte `.text` alignment
  block, with `.rdata`, `.pdata`, and `.rsrc` unchanged. The product accepts that
  measured 0.06% cost for one shared contract and system-compressed PNG rather
  than claiming native FFI is automatically smaller.
  Windows native key delivery uses one platform `ConsoleGuard` to serialize `FreeConsole` /
  `AttachConsole`, retries the documented already-attached case, opens `CONIN$`,
  and writes an exact key-down/key-up `INPUT_RECORD` pair with
  `WriteConsoleInputW`. The real `cmd.exe` cursor journey and alternate-screen
  `less` arrow/wheel journey are now default black-box tests instead of ignored
  known gaps. Exact-profile evidence is 87 unit, 18 black-box and one isolated
  control test with zero ignored or failed tests. This repair changes the
  unwind/trace-only release-fast PE from 790,528 to 791,552 bytes; the measured
  1,024-byte cost is retained for behavior and single native ownership, not
  presented as a size optimization.
  The next structural step removes `rmux-pty` from the Windows production graph
  entirely. The direct adapter owns synchronous ConPTY endpoints, a cancellable
  overlapped writer, a drain-safe output pump, build-gated passthrough fallback,
  PowerShell DSR fragments, suspended process creation, exact wait/exit status,
  and a `KILL_ON_JOB_CLOSE` Job Object. It never resumes an unassigned child and
  never reuses a ConPTY whose failed first child may have closed its output pump.
  A platform real-child `COMSPEC /D /C echo` regression and all 18 con black-box
  journeys plus the isolated multitab control journey pass. The same
  unwind/trace-only release-fast artifact falls from 791,552 to 761,856 bytes,
  a measured 29,696-byte reduction while retaining unwind containment.
  Windows glyph selection and gray8 coverage now execute behind the platform
  `RasterGlyph` contract through bounded GDI calls with deterministic DC/font
  cleanup; con no longer opens or parses font files, and ab_glyph/ttf_parser
  are absent from its Windows production graph. Linux/macOS retain equivalent
  file-font behavior inside a shared platform portable adapter. The current
  GDI leaf accepts one UTF-16 unit and returns a missing glyph for supplementary
  scalars rather than splitting a surrogate pair; broader emoji fallback is an
  explicit later DirectWrite tradeoff, not a false shipped claim.
  Configuration, scripted interaction, atomic snapshots and public JSON output
  now share one strict bounded JSON codec instead of four serde/ad-hoc paths. It
  accepts UTF-8, JSON escapes and valid surrogate pairs while bounding input
  bytes, nesting, nodes, object fields and strings; duplicate keys, isolated
  surrogates, malformed/non-finite numbers and trailing data fail locally.
  `serde_json` remains a dev-only interoperability oracle and is absent from the
  production graph. The private local control wire instead uses the versioned,
  length-prefixed typed `ATC1` frame described above. Its GUI handoff preserves
  concurrent request workers without linking two generic `mpsc` instances: a
  mutex-owned FIFO carries requests and a one-shot Condvar slot carries each
  reply. Closing the GUI atomically rejects new queue entries, drops pending
  senders, and wakes reply waiters rather than making them consume the full
  timeout. The official release-fast PE falls from 733,184 to 714,752 bytes;
  90 binary unit tests, 18 Windows black-box tests and the isolated multitab
  control journey pass, including explicit sender-drop and closed-queue tests.
  Thread creation is likewise a shared platform mechanism rather than repeated
  product glue. Con reader/waiter, control listener/request and the Windows
  ConPTY output pump submit boxed tasks through one non-generic named-thread
  trampoline; the same API owns general detached-child reapers on every host.
  Thread names and spawn failures remain observable, and a task panic remains
  contained by Rust's unwind-enabled JoinHandle boundary. Dedicated tests prove
  both name preservation and panic containment, while all 90 con unit tests,
  18 Windows black-box tests and the isolated multitab control journey pass.
  The official release-fast PE falls again from 714,752 to 698,880 bytes.
  On Windows that detached contract now calls `CreateThread` directly through
  one raw platform FFI entry, closes the creator's handle immediately, and sets
  the debugger-visible name with `SetThreadDescription`. Creation failure drops
  the boxed context on the caller; the native entry catches every unwind before
  it can cross the system ABI. Linux and macOS retain the same public contract
  over their std adapter pending equivalent pthread evidence. Native tests read
  back the OS thread description and prove panic-driven task destruction, while
  the full con suite proves the real PTY/control/child paths. The official
  release-fast PE falls from 698,880 to 688,128 bytes without a new dependency.
  The child waiter now publishes its one completion bit directly instead of
  instantiating a final production `mpsc` channel for one `()` value. Release
  stores the state before the existing window wake; the GUI consumes it once
  with an acquire-release swap. This keeps process-exit authority independent
  from ConPTY pipe EOF while removing channel allocation and disconnect states.
  Normal, failed and fast command exits plus multitab control remain covered by
  90 unit, 18 black-box and one isolated control test. The official release-fast
  PE falls from 688,128 to 667,648 bytes.
  Session ownership is now a product-specific compact store rather than a
  general-purpose ordered map. `Workspace` remains the sole authority for tree
  order, parentage and stable tab identity; the store only performs linear id
  routing over the small interactive tab set and may swap entries on removal
  because its physical order is unobservable. This removes ordered-tree node
  allocation and rebalancing machinery: the isolated PE falls from 652.0 to
  638.5 KiB, `.text` from 429.0 to 419.0 KiB, and the official release-fast PE
  from 667,648 to 653,824 bytes. The same 90 unit, 18 black-box and one isolated
  multitab control test remain green. Future assembly or native FFI work must
  likewise start from linked-symbol or disassembly evidence rather than from
  mechanism preference.
  Its Windows resource retains the existing icon's 16/32/64 PNG frames while
  removing redundant mip sizes: `.rsrc` falls from 90,112 to 8,704 bytes, the
  source ICO is capped at 16 KiB by the build script, and Windows shell icon
  extraction succeeds. The measured release PE is 8,704 bytes below its new
  512 KiB x86_64 artifact budget; the no-LTO release-fast PE is separately
  reported as 543,744 bytes and is not release-size evidence.
  Evidence is 73 binary unit tests, 185 platform library tests, the isolated
  public-control GUI journey, and the Windows terminal black box at 16 passed,
  2 pre-existing ignored gaps, 0 failed. It remains non-default until native IME
  preedit/commit from IMM32, candidate anchoring in documented client
  coordinates, matched pointer capture/loss cancellation, and DPI suggested
  rectangles are wired. Human Chinese-IME keyboard acceptance remains required.
  Its 59-line resolved dependency graph contains no
  winit, softbuffer, Rhai, HTTP/TLS or script-engine dependency; 512 KiB remains
  a target, not a shipped claim.
  clamps rather than selecting/closing an unrelated terminal. Visual styling
  may intentionally differ from the workbench, while validated terminal,
  interaction and robustness mechanisms are promoted to shared typed layers
  instead of copying server, Fleet or script policy into this binary.
- [x] explicit tab/server close cancels I/O, closes ConPTY ownership, and
  waits within a 750 ms bound for the process-wait and reader workers; success
  is reported only after both workers finish, while an incomplete shutdown
  returns a typed error instead of pretending the terminal was closed
- [~] robust CJK double-cell layout; broader visual regression is needed
- [ ] sustained high-throughput and long-output performance qualification

`agenterm-con` keeps a high-contrast vertical scrollbar visible at the right
edge of every terminal viewport. Its column is excluded from PTY grid sizing;
the thumb maps bottom to the live view and upward to older history. Track clicks
move one visible page, thumb drags retain their grab offset, and capture loss
cancels the drag without emitting terminal mouse input. Structured snapshots
expose both current and maximum scrollback. The tab-tree divider exposes a
horizontal-resize cursor on hover and a bounded capture-safe drag that retains
the terminal's minimum usable width.

Tab-divider drag must remain visually responsive without synchronously resizing
the PTY for every pointer event. Chrome follows the pointer immediately while
the latest PTY/VT grid geometry is applied through the shared trailing-edge
resize path. Glyph rows and screenshot channel packing use shared bit-exact
architecture pixel kernels; unsupported architectures retain identical scalar
output. Rectangle fills share one clipped stride-aware UI-core contract across
con and the main Unix renderer while relying on compiler-vectorized
`slice::fill`; reducing dirty rows/frames remains the next rendering optimization
rather than maintaining an unmeasured fill-specific ISA fork.

Completing a non-empty local terminal selection copies its normalized text to
the system clipboard. A click without a range, application-owned mouse gesture,
scrollbar drag, or tab-divider resize must not mutate clipboard contents.
