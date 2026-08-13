# `agenterm-con` workspace and input

Parent: [Lightweight terminal host (`agenterm-con`)](PRD_02_23_agenterm_con.md)

This module owns the standalone host's tab tree, local chrome, external composer
input, scrollbar and divider interaction, selection and clipboard behavior, and
focus ownership. The shared physical VT selection kernel remains owned by
[terminal runtime](PRD_02_01_terminal_runtime.md).

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Tab tree authority

- [x] `Workspace` is the sole authority for tree order, parentage and stable tab
  identity. Closing a parent promotes its direct children instead of terminating
  them; a parent cycle is rejected.
- [x] session ownership is a product-specific compact store rather than a
  general-purpose ordered map: it performs linear id routing over the small
  interactive tab set and may swap entries on removal because its physical order
  is unobservable.
- [x] tree depth is a `Workspace`-owned derived cache aligned with node order.
  Root and child creation append their known depth in O(1); close and direct
  child promotion rebuild through the shared UI-core typed algorithm, which
  remains the sole authority for missing parents, duplicate ids, cycles and
  complete topology resolution. Chrome paint borrows the immutable depth slice
  instead of sorting, allocating and resolving every parent chain per frame.
- [x] geometry, iterative typed tree-depth resolution, tree viewport bounds and
  hit results are pure deterministic contracts covered independently of
  Win32/PTY state. An out-of-range hit or scroll safely becomes background or a
  bounded no-op, and a hit beyond the last row clamps rather than selecting or
  closing an unrelated terminal.
- [x] chrome geometry treats NaN pointer/sidebar values as the minimum safe
  bound and saturates extreme DPI padding and row-coordinate arithmetic.
  Untrusted/extreme dimensions cannot wrap a close target onto another row,
  overflow layout construction, or collapse the sidebar through an unordered
  floating-point comparison.
- [x] terminal selection endpoints are normalized once per raster pass rather
  than once per visible cell, and wide-cell/decoration geometry saturates at
  native numeric limits so malformed resize state cannot panic painting.
- [x] shared iterative tree-depth resolution sorts `(id,index)` pairs and uses
  binary lookup, preserving typed duplicate/missing/cycle failures and the
  20,000-node non-recursive test without randomized hashing. Its index replaces
  generic `slice::sort_unstable` with a shared no-allocation iterative heapsort
  that stays deterministic O(n log n) and preserves second-input duplicate
  diagnostics.

## Local chrome

- [x] the local chrome owns a vertically scrollable left tree with row-level
  close targets and top `z`/`Z` font controls, plus a distinct bottom composer
  input and send action.
- [~] Linux `agenterm-con` publishes that chrome as a real AT-SPI child tree
  (`Tabs`, `Session`, `Command`, `SEND`) so `cu tree --window` is not the
  one-node X11 title frame. winit/softbuffer has no atk-bridge; the process
  registers itself. Inner `cu focus`/`click --name Command` (or `SEND`) uses
  `addressing=accessibility-tree`. Windows/macOS publishers are not claimed.
- [x] the default 15 logical-pixel terminal font corresponds to roughly 11.25 pt
  at 96 DPI and is no smaller than the tree labels.
- [x] the host chrome defaults to high-contrast black/white/gray and the
  terminal default foreground is near-white on black; explicit ANSI application
  colors remain intact.
- [x] chrome repaint allocates no joined strings for tree labels, composer
  destination, committed input, IME preedit and cursor. One product-local text
  raster pass consumes borrowed segments and stack-formatted tab digits under a
  shared clip limit, with a CJK/non-cell-aligned pixel oracle proving exact
  parity.
- Visual styling may intentionally differ from the workbench, while validated
  terminal, interaction and robustness mechanisms are promoted to shared typed
  layers instead of copying server, Fleet or script policy into this binary.

## External composer input

- [x] while focused, the composer owns Space and all keyboard events instead of
  leaking ignored keys into the PTY. `Ctrl+A/C/V/X` provide select-all, copy,
  bounded single-line paste and cut semantics. Every keyboard, IME, paste and
  accessibility insertion shares a 64 KiB total-buffer ceiling and truncates
  only at UTF-8 boundaries. Its explicit send action is the only path that
  writes composed text to the active terminal.
- [x] composer focus isolation is owned by a Windows black-box journey: it
  obtains the current input bounds from `ui-snapshot`, performs a native client
  click, routes Space and `Ctrl+A/C/X/V` through `send-ui-keys`, proves the
  terminal is unchanged before Enter, then proves the composed command reaches
  the PTY.
- [x] a physical composer click does not call native focus from inside its
  pointer callback: Win32 has already activated the receiving top-level window,
  and the redundant `SetForegroundWindow`/`SetFocus` chain could reenter dispatch
  and disturb presentation. A synchronous native-click plus character probe kept
  the HWND alive and visible and localized 5,921 of 6,049 changed pixels to the
  composer band.

## Scrollbar and divider

- [x] a high-contrast vertical scrollbar stays visible at the right edge of
  every terminal viewport. Its column is excluded from PTY grid sizing; the
  thumb maps bottom to the live view and upward to older history. Track clicks
  move one visible page, thumb drags retain their grab offset, and capture loss
  cancels the drag without emitting terminal mouse input. Structured snapshots
  expose both current and maximum scrollback.
- [x] the tab-tree divider exposes a horizontal-resize cursor on hover and a
  bounded capture-safe drag that retains the terminal's minimum usable width.
- [x] divider drag stays visually responsive without synchronously resizing the
  PTY for every pointer event: chrome follows the pointer immediately while the
  latest PTY/VT grid geometry is applied through the shared trailing-edge resize
  path.

## Selection and clipboard

- The auto-copy rule itself — a completed non-empty selection copies normalized
  text, while a rangeless click, application-owned gesture, scrollbar drag or
  divider resize must not mutate the clipboard — is shared and owned by
  [terminal runtime](PRD_02_01_terminal_runtime.md).
- [x] Windows selection auto-copy counts UTF-16 units, performs one checked
  movable `GlobalAlloc`, and encodes directly into the locked system allocation
  instead of first collecting a Rust vector and copying it. The caller frees
  every allocation before a successful `SetClipboardData`; only that call
  transfers ownership, and the final UTF-16 NUL is explicit.
- [x] `send-paste` reaches the same bracketed-paste-aware path as clipboard
  input, so scripted and human paste share one contract.

## Configuration

- [x] the user configuration root is selected through the platform runtime
  facade: Windows calls `SHGetFolderPathW(CSIDL_APPDATA)` into caller-owned
  UTF-16 storage, avoiding both environment-path policy in the product and the
  COM allocation required by `SHGetKnownFolderPath`; Linux/macOS retain the
  documented `~/.config` location.
- [x] configuration input does not construct the output-side `JsonValue` DOM.
  One bounded single-pass scanner validates every unknown value, escape,
  surrogate pair, duplicate key and nesting budget while decoding only
  `font_size`, `cols` and `rows`; escaped spellings of known keys retain their
  JSON meaning.

## Native adoption gate

- [~] the feature-gated native Win32 pixel host proved the platform boundary can
  remove winit/softbuffer from the linked con path without changing product
  state, and is now the Windows default while Linux/macOS select the portable
  host. Native IME preedit/commit from IMM32, candidate anchoring in documented
  client coordinates, matched pointer capture/loss cancellation, and DPI
  suggested rectangles are wired; human Chinese-IME keyboard acceptance remains
  required before the IME surface is claimed complete.
