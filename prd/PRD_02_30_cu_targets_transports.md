# `agenterm-cu` targets and transports

Parent: [Computer-use foundation (`agenterm-cu`)](PRD_02_28_agenterm_cu.md)

Delivery truth: `agenterm-cu` is the sole executable and the first runtime
consumer of `libagenterm`. Target selection and transport policy remain product
semantics here; native window, accessibility, input and desktop-host mechanisms
remain behind the ABI/platform boundary.
This module owns the target family, transport selection, and the per-platform
backends that realize the abstract command set from
[29](PRD_02_29_cu_command_surface.md).

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Target family

- [~] `current`, `ssh`, `rdp` and `vnc` are tiers of one family sharing one
  command set. `current` is the **local degenerate tier** — transport is
  in-process — not a temporary prototype to be replaced later. `ssh` first cut
  is OpenSSH `ssh` exec of a remote `agenterm-cu --target current` worker
  (`--ssh <user@host>`; same verbs including actuate; no new verb). `rdp` /
  `vnc` remain planned.
- [~] `current` ships first. Doing so is the cheapest way to pin the interface,
  because adding a remote transport afterwards changes transport only, not the
  commands above it. `ssh` click evidence reuses the #47 con-publish named
  `SEND` Action over loopback `sshd` against a second `agenterm-con` (never
  steal the resident control socket): host
  `cu --ssh send-text --name Command -- SEED` (payload after `--`; not
  `--text`) plants the seed, host `cu --ssh click --name SEND` runs remote
  AT-SPI Action `DoAction`, then host independent
  `cu --ssh get-text --name Command` returns empty (composer cleared on
  submit). Never screenshot, `--coords`, or XTest.
- [ ] a target reference is explicit, addressable and stable for the lifetime of
  its session. Enumerating targets and describing one target's declared
  capabilities are themselves commands.
- [ ] capability differences between tiers are **declared, not discovered by
  failure**. A caller can ask what a target supports before acting, and an
  unsupported command returns typed `Unsupported` rather than a fake success or
  a silent coordinate fallback.

## Platform backends

- [ ] backends consume `agenterm-platform` contracts — screenshot, window,
  input, accessibility tree, process reference, clipboard, filesystem — and do
  not open raw OS APIs. A missing mechanism is added to the platform crate with
  typed `Available`/`Unsupported`/`Failed`, per
  [20 Native platform](PRD_02_20_native_platform.md).
- [ ] Windows, Linux and macOS each reach the same abstract command set through
  their own accessibility/input stacks. Product behavior does not move into the
  adapters: what a click *means* is one shared rule; how a click is delivered is
  per host.
- [ ] a platform whose control-tree access is unavailable or not yet wired
  returns typed `Unsupported` / `Failed`. Coordinate-only or screenshot-only
  operation is always visible in the command result ([29](PRD_02_29_cu_command_surface.md));
  it is never a silent substitute for structured success.
- [ ] first-platform delivery is explicit and does not imply the others. A tier
  or platform is claimed only with its own evidence.

## Platform accessibility backends

This branch is the **native accessibility stack** that backs structured
`tree` observation and `click` / `focus` by node identity. It lives in
`agenterm-platform` (`a11y-tree` and related contracts); `agenterm-cu` selects the stack
for the host OS and target tier. Screenshot capture and coordinate pointer
injection are separate platform mechanisms and remain **degraded fallbacks**
only — never silent replacements when the a11y tree is unavailable.

```text
targets / transports (30)
├── target family
│   ├── current   (local, in-process — ships first)
│   ├── ssh
│   ├── rdp
│   └── vnc
└── platform a11y backends (agenterm-platform)
    ├── Windows: native API + UIA
    ├── macOS: AX (NSAccessibility)
    └── Linux: AT-SPI2
```

Canonical host mapping (approved product vocabulary):

| Host | Native accessibility stack | Structured `tree` source | Structured actuation |
|------|---------------------------|--------------------------|----------------------|
| Windows | native API + **UIA** | `IUIAutomation` control tree | UIA patterns / legacy accessible (`Invoke`, `LegacyIAccessible`) |
| macOS | **AX** (`NSAccessibility`) | accessibility element tree | `AXPress`, `AXRaise`, editable value |
| Linux | **AT-SPI2** (`at-spi2-core` / `org.a11y.atspi.*`) | AT-SPI accessible hierarchy | AT-SPI `Action` / `Component` / `EditableText` |

### Requirements by stack

**Shared (all hosts)**

- [ ] `agenterm-platform` exposes one typed `accessibility_tree` contract:
  flattened nodes with path id, role, name, states, exact bounds, and action
  names. `agenterm-cu` maps this contract to its public JSON without host-specific
  fields leaking upward.
- [ ] when the a11y bus / API is missing (headless without a11y, no registry,
  denied permission), `tree` and node actuation return typed `Unsupported` or
  `Failed` — never coordinate guessing while reporting structured success.
- [ ] screenshot and coordinate pointer paths are explicit degraded modes with
  observable markers in the reply; they do not satisfy a caller that requested
  structured node identity.

**Linux — AT-SPI2 (`current` first evidence)**

- [~] `current` on Linux/X11 enumerates a control tree through AT-SPI2:
  `agenterm-platform` (`a11y-tree`) implements the host stack; `agenterm-cu` consumes
  libagenterm milestone 6 (`agt_a11y_tree_snapshot` / `agt_a11y_node_perform`)
  rather than calling platform accessibility APIs directly. Nodes carry role,
  name, states, screen bounds, and action names; node ids are child-index paths
  from each application root (for example `/3/0/0/1/0`).
  A `--window` snapshot matches AT-SPI application roots by the X11
  `_NET_WM_PID`, that process's descendants (WebKit web process), and
  exact title / `WM_CLASS` / `comm` equality — not PID equality alone.
  Child walks read raw `(bus name, path)` pairs so well-known embed
  destinations (WebKit `org.webkit.app-*.Sandboxed.WebProcess-*`) are
  not dropped by unique-name-only `ObjectRef` parsing. The walker talks
  to the a11y bus only (no atspi P2P handshake — that hangs WebKit/Wails
  sockets), skips dests with no owner, maps empty WebKit `GetRoleName`
  via `GetRole`, and snapshots Accessible name/role/state so a Reasonix
  / MiniBrowser document tree exposes named inner widgets (buttons,
  text, tabs). `agenterm-con` on
  Linux registers as an AT-SPI toolkit (`a11y-publish`) and exposes the
  painted chrome as children (tabs, session, Command input, SEND). A
  toolkit that still never registered (stock `xfce4-terminal` without
  atk-bridge) returns a one-node showing `frame` from the X11 window
  title and bounds so named `wait` / `focus` / `send-keys` can address
  that window; `focus`/`click` on that node raise it. The one-node frame
  is not the success path for `agenterm-con` and is not a screenshot or
  coordinate substitute.
- [~] `click --node <path>` and `focus --node <path>` invoke AT-SPI `Action`
  / `Component` for the resolved node via `agt_a11y_node_perform`. A
  node-addressed click uses a named `click`/`press` when present, otherwise
  the AT-SPI default action (`DoAction(0)`) when the node exposes Action —
  including Chrome controls whose `GetActions` names are empty. A showing
  named node with no Action interface uses the AT-SPI Component path
  (`GetExtents` + registry `GenerateMouseEvent`) and still reports
  `addressing=accessibility-tree`. It never silently becomes `--coords` /
  `--degraded`. Focus stays named-`focus` then `Component::grab_focus`.
  Invalid paths return typed `a11y_node_not_found`.
- [~] `click --window HANDLE --name PAT [--role ROLE]` and the matching
  `focus` form resolve one showing node with the same matcher as
  `wait --node-name-contains`, then call the node-path AT-SPI action above.
  `--name` cannot be combined with `--node` or `--coords`. A miss is typed
  `a11y_node_not_found`. Two or more showing matches are typed
  `a11y_node_ambiguous` with the match count; the command does not pick
  the first. There is no screenshot or degraded-coordinate substitute.
- [x] `send-text --window HANDLE --name PAT [--role ROLE] [--] <text...>`
  resolves through that same path, then writes via AT-SPI `EditableText`
  (`SetTextContents` / `InsertText`, `agt_a11y_node_set_text`) or, when
  the node exposes `Text` + `editable` but not `EditableText` (Chrome,
  WebKitGTK/Reasonix `<textarea>`), via AT-SPI `Text` plus the toolkit
  set-value, confirmed by
  `GetText`. A named showing node with no writeable text interface
  typed-fails (`a11y_text_unavailable`) and never silently uses XTest /
  `input_inject::type_text`. Resolution failure (miss or ambiguous name)
  aborts before any write. Without `--window`, `send-text` still injects
  into whatever is focused.
- [x] `send-text --window HANDLE` without `--name` writes that same
  AT-SPI `EditableText` / `Text` + toolkit set-value path on the
  showing focused node (innermost Text candidate). Never XTest /
  `input_inject::type_text` when `--window` is set. Independent
  `get-text --window HANDLE` (no `--name`) must equal the typed
  string. Live hosts: agenterm-con named `Command` (native
  `EditableText` on a second con; never steal the resident control
  socket), Chrome `GetTextField` after `focus --name` (renderer on the
  same host AT-SPI bus: `AT_SPI_BUS` / `AT_SPI_BUS_ADDRESS`), and
  Reasonix composer `Message Reasonix…` under
  `scripts/reasonix-desktop-a11y.sh` (AT-SPI `Text` plus the
  eval-helper set-value; no protocol change). Do not mark this leaf
  shipped on worker JSON.
- [x] `copy --window HANDLE --name PAT [--role ROLE]` resolves through
  that same path, then publishes AT-SPI `Text.GetText`
  (`agt_a11y_node_get_text`) onto the native clipboard
  (`agt_clipboard_set_text`) and reports `via=gettext`. On Linux X11 the
  seed is a native CLIPBOARD selection owner, not `xclip`. A named
  showing node with no Text interface typed-fails
  (`a11y_text_unavailable`) and never silently uses XTest / `--coords` /
  screenshot. Resolution failure (miss or ambiguous name) aborts before
  any clipboard write. Live close-the-circuit includes Chrome fixture
  fields and the Reasonix composer (`Message Reasonix…`): after
  `copy --name`, a different `send-text`, then `paste --name` with no
  `--text`, `wait --text-equals` sees independent GetText equal the
  copied source (paste write still uses the WebKit eval-helper set-value
  path).
- [x] `copy --window HANDLE` without `--name` publishes GetText from the
  showing focused node (innermost Text candidate) onto native CLIPBOARD
  (`via=gettext`). Never XTest / `--coords` when `--window` is set. Proof
  is independent seed → focused `copy` → clear → focused `paste` →
  `get-text --window HANDLE` (no `--name`) equal to the seeded string.
  Live hosts: agenterm-con named `Command` after `focus --name`
  (`via=gettext` on copy; paste restore `via=editable-text` on a second
  con; never steal the resident control socket); Chrome `GetTextField`
  after `focus --name` on the host AT-SPI bus (`AT_SPI_BUS` /
  `AT_SPI_BUS_ADDRESS`); Reasonix composer `Message Reasonix…` after
  `focus --name` under `scripts/reasonix-desktop-a11y.sh` (`via=gettext`;
  paste restore uses eval-helper set-value, `via=text`). Without
  `--window` copy is invalid.
- [x] `paste --window HANDLE --name PAT [--role ROLE] [--text TEXT]`
  resolves through that same path, then writes the clipboard via the same
  AT-SPI `EditableText` / `Text` + toolkit set-value path as named
  `send-text` (`agt_a11y_node_set_text`). `--text` only seeds the clipboard
  (`agt_clipboard_set_text`); the field write always reads
  `agt_clipboard_get_text`. On Linux X11 the seed is a native CLIPBOARD
  selection owner, not `xclip`. A named showing node with no writeable
  text interface typed-fails (`a11y_text_unavailable`) and never silently
  uses XTest / `--coords` / screenshot. Resolution failure (miss or
  ambiguous name) aborts before any write or clipboard seed. Reasonix
  composer (`Message Reasonix…`) writes through the same WebKit
  eval-helper set-value path as named `send-text`. A prior `copy --name`
  may seed the clipboard instead of `--text`.
- [x] `paste --window HANDLE` without `--name` writes that same clipboard
  path on the showing focused node (innermost Text candidate). Never
  XTest / `--coords` when `--window` is set. Proof is independent
  `get-text --window HANDLE` (no `--name`) equal to the clipboard string.
  Live hosts: agenterm-con named `Command` (native `EditableText`,
  `via=editable-text` on a second con; never steal the resident control
  socket); Chrome `GetTextField` after `focus --name` on the host AT-SPI
  bus (`AT_SPI_BUS` / `AT_SPI_BUS_ADDRESS`); Reasonix composer
  `Message Reasonix…` after `focus --name` under
  `scripts/reasonix-desktop-a11y.sh` (eval-helper set-value, `via=text`).
  Without `--window` paste is invalid.
- [x] `send-keys --window HANDLE --name PAT [--role ROLE] [--] <keys...>`
  resolves through that same path, then delivers the chord via AT-SPI
  `DeviceEventListener` (`NotifyEvent`, `agt_a11y_node_send_keys`). A named
  showing node with no Device/key interface typed-fails
  (`a11y_key_unavailable`) and never silently uses XTest /
  `input_inject::send_keys`. Resolution failure (miss or ambiguous name)
  aborts before any keystroke. After a successful named `send-keys`,
  the same window's AT-SPI tree must still be there for a second named
  command (one process-wide a11y-bus connection; do not drop the bus).
- [x] `send-keys --window HANDLE` without `--name` targets the showing
  focused node (innermost Text candidate). Prefers
  `DeviceEventListener.NotifyEvent` (`via=device-event`). When that
  interface is absent (con `Command`; Chrome renderer entry; WebKitGTK
  textarea) and the payload is plain typeable text, writes through
  AT-SPI `EditableText` / `Text` + toolkit set-value (same path as
  focused `send-text`). Never XTest / `input_inject::send_keys` when
  `--window` is set. Independent `get-text --window HANDLE` (no
  `--name`) must equal the typed string. Live hosts: agenterm-con named
  `Command` (native `EditableText`, `via=editable-text` on a second con;
  never steal the resident control socket), Chrome `GetTextField` after
  `focus --name` on the host AT-SPI bus (`AT_SPI_BUS` /
  `AT_SPI_BUS_ADDRESS`, `via=text`); Reasonix composer
  `Message Reasonix…` after `focus --name` under
  `scripts/reasonix-desktop-a11y.sh` (`via=text`). Special chords
  without a key interface still typed-fail. Do not mark this leaf
  shipped on worker JSON.
- [x] `wait --window HANDLE --name PAT [--role ROLE] --text-equals TEXT`
  (alias `--node-text-equals`) polls `agt_a11y_node_get_text` (`Text.GetText`)
  on that unique showing node until the independent text equals `TEXT`.
  Timeout is typed `timeout`. This is not `send-text` / `paste` / `copy`
  `matched.text`, not a sidecar walk of `agenterm-cu tree` snapshot `text` fields,
  and not the WebKit eval helper's queued-job `OK` (Reasonix composer
  `Message Reasonix…`).
  Never screenshot, XTest, or `--coords`.
- [x] `wait --window HANDLE --name PAT [--role ROLE] --text-contains SUB`
  (alias `--node-text-contains`) polls that same `agt_a11y_node_get_text`
  until the independent GetText contains `SUB`. Success reports
  `via=gettext` and the full GetText. Timeout is typed `timeout` and
  reports the last GetText. `send-text` / `paste` / `copy` `matched.text`
  do not count. Never screenshot, XTest, or `--coords`.
- [x] `scroll --window HANDLE --name PAT [--role ROLE]` resolves through
  that same path, then one-shot AT-SPI `Component.ScrollTo(TopEdge)`
  (`agt_a11y_node_scroll`). Success is `via=scroll-to`. Missing / false /
  `UnknownMethod` typed-fails (`a11y_scroll_unavailable`). Never Action
  `scroll*`, XTest wheel, `--coords`, or screenshot. Geometric proof is
  independent `get-extents`, not `matched.extents`. WebKitGTK
  `Component.GetExtents(Screen)` is already the independent observe
  sibling; `ScrollTo` is a no-op true, so Reasonix launched via
  `scripts/reasonix-desktop-a11y.sh` applies `scrollIntoView` through
  the same eval helper as named set-value (hello `A11YSCROLL1`, no ABI
  change). Linux `agenterm-con` publishes a real `ScrollTo` that moves
  named `OffscreenField` (Session child); same verbs, no ABI change.
- [x] `get-extents --window HANDLE --name PAT [--role ROLE]` resolves
  through that same path, then independent AT-SPI
  `Component.GetExtents(Screen)` (`agt_a11y_node_get_extents`). Snapshot
  `node.bounds` do not count. Empty extents typed-fail
  (`a11y_extents_unavailable`).
- [x] `select --window HANDLE --name PAT --start N --end M [--role ROLE]`
  resolves through that same path, then one-shot AT-SPI
  `Text.SetSelection(0, start, end)` (`agt_a11y_node_set_selection`).
  Success is `via=set-selection`. Missing Text / `UnknownMethod`
  typed-fails (`a11y_selection_unavailable`). SetSelection false
  typed-fails (`a11y_selection_no_effect`). Never XTest, mouse-drag,
  `--coords`, or screenshot. Proof is independent `get-selection`, not
  the `select` reply.
- [x] `get-selection --window HANDLE --name PAT [--role ROLE]` resolves
  through that same path, then independent AT-SPI `Text.GetNSelections`
  + `GetSelection(0)` (`agt_a11y_node_get_selection`). The `select`
  reply payload does not count. Missing Text typed-fails
  (`a11y_selection_unavailable`). `n == 0` is empty success.
  Reasonix composer (`Message Reasonix…` under
  `scripts/reasonix-desktop-a11y.sh`) uses that same native Text
  path; WebKit 2.52 already implements SetSelection/GetSelection
  (no `A11YSELECT1` eval helper — unlike ScrollTo). Linux
  `agenterm-con` composer `Command` publishes real
  `SetSelection` / `GetNSelections` / `GetSelection` (same verbs,
  no ABI change).
- [x] `set-caret --window HANDLE --name PAT --offset N [--role ROLE]`
  resolves through that same path, then one-shot AT-SPI
  `Text.SetCaretOffset` (`agt_a11y_node_set_caret_offset`).
  Success is `via=set-caret-offset`. Missing Text / `UnknownMethod`
  typed-fails (`a11y_caret_unavailable`). SetCaretOffset false
  typed-fails (`a11y_caret_no_effect`). Never XTest, `--coords`, or
  screenshot. Proof is independent `get-caret`, not the `set-caret`
  reply. Live Chrome fixture field `CaretField`
  (`fixtures/cu/310-chrome-caret.html`) uses that same native Text
  path (no ABI / eval-helper change). Reasonix composer
  (`Message Reasonix…` under `scripts/reasonix-desktop-a11y.sh`)
  uses that same native path; WebKit 2.52 already implements
  SetCaretOffset / CaretOffset (no `A11YCARET1` eval helper).
- [x] `get-caret --window HANDLE --name PAT [--role ROLE]` resolves
  through that same path, then independent AT-SPI `Text.CaretOffset`
  / `GetCaretOffset` (`agt_a11y_node_get_caret_offset`). The
  `set-caret` reply payload does not count. Missing Text typed-fails
  (`a11y_caret_unavailable`). Chrome `CaretField` unfocused
  `CaretOffset` may be `-1`; after `set-caret --offset N` independent
  readback equals `N`. Reasonix composer after `send-text HELLO`
  reports `CaretOffset=5`; after `set-caret --offset 2` independent
  `get-caret` is `2`. Linux `agenterm-con` composer `Command`
  publishes real `SetCaretOffset` / `CaretOffset` (ABI 1.9 verbs).
- [x] `get-text --window HANDLE --name PAT [--role ROLE]` resolves
  through that same path, then one-shot independent AT-SPI
  `Text.GetText` (`agt_a11y_node_get_text`) — the same authority
  `wait --text-equals` polls, without a timeout. `send-text` /
  `paste` / `copy` `matched.text` and tree snapshot `text` do not
  count. Missing Text typed-fails (`a11y_text_unavailable`). Never
  XTest / `--coords` / screenshot. Live Chrome fixture field
  `GetTextField` (`fixtures/cu/311b-chrome-gettext.html`) and
  Reasonix composer (`Message Reasonix…` under
  `scripts/reasonix-desktop-a11y.sh`) both use that same native Text
  path; WebKit 2.52 already implements GetText on the composer
  `<textarea>` (no `A11YGETTEXT1` eval helper). Linux `agenterm-con`
  composer `Command` publishes real `Text.GetText` (same verb, no
  ABI change).
- [~] `get-text --window HANDLE` without `--name` uses that same
  GetText authority on the showing focused node (innermost Text
  candidate). Linux connect prefers `AT_SPI_BUS_ADDRESS` then
  `AT_SPI_BUS`, strips a `GetAddress` `,guid=` suffix, and only then
  asks `org.a11y.Bus`. `scripts/box-chrome-a11y.sh` writes the
  standard `$XDG_RUNTIME_DIR/at-spi/bus` file after box-chrome's
  XDG rewrite so the renderer joins that same host socket. A
  one-node synthetic X11 `frame` is not a Chrome document tree.
- [~] `windows` / `screenshot` / coordinate-degraded input on `current` still
  use `agenterm-platform` until `agt_window_enumerate` / unified screenshot /
  `agt_input_inject` milestones ship; capability JSON documents the gap.
- [ ] AT-SPI unavailable at runtime (no session bus, registry absent) → typed
  `Unsupported` / `Failed`; no silent fallback to XTest coordinates.
- [ ] black-box evidence: `scripts/cu-linux-smoke.sh` against the real `agenterm-cu`
  binary on a host with `DISPLAY` and `at-spi2-registryd`.

**Windows — native API + UIA**

- [~] Windows `current` now reaches the UIA accessibility facade through the
  runtime `agenterm.dll` boundary: `agenterm-cu` `Command`/`Executor` owns
  target resolution and product meaning, while the ABI and
  `agenterm-platform` own UIA tree, Value, Invoke and Focus mechanisms. The
  owning evidence is five pure tests plus two real Win32 UIA fixture tests.
  The staged public `cu-windows-smoke` also passes all seven declared evidence
  checks through the colocated `agenterm-cu.exe` + `agenterm.dll`; Candidate
  qualification and release remain open.
- [x] `tree` uses the UIA Control View and returns bounded node identity,
  parent relationships, role, name, text, state, bounds and actions. Node IDs
  encode UIA RuntimeId paths, but every Value, Invoke, Focus or key operation
  resolves that path again from the requested HWND (or the bounded desktop
  root for `None`). A RuntimeId is never treated as a retained COM object, and
  no COM interface pointer is cached across calls or apartments.
- [x] Each UIA operation initializes an MTA-capable COM session with owned RAII
  for interfaces, BSTR, SAFEARRAY and VARIANT values, configures
  `SetAutoSetFocus(FALSE)`, a 500 ms connection timeout and a 250 ms transaction
  timeout, and also enforces 5 s snapshot / 2 s action wall-clock budgets plus
  strict node, depth and string limits. Window loss, access denial, timeout and
  recycled nodes are typed failures.
- [x] Structured Focus calls UIA `SetFocus`; text writes use Value and reads use
  Value/Text patterns; click prefers Invoke, SelectionItem, Toggle and the
  legacy default action. Missing patterns fail typed. No UIA node operation
  silently degrades to coordinates; node key delivery is explicitly reported
  as `uia-focus+send-input` after UIA focus.
- [x] Win32 window enumeration uses the runtime library's two-stage
  required-size/fill contract. Desktop churn can increase `required` after the
  caller allocated `capacity`; `required > capacity` triggers a bounded retry
  with a fresh capacity instead of truncation, out-of-bounds writes, false
  success or an unbounded loop. Exhaustion is typed failure.
- [~] Screenshot and coordinate/input injection remain separate platform
  contracts consumed through the runtime library; they do not replace UIA
  structured success.

**macOS — AX (NSAccessibility)**

- [ ] AX-backed `tree` and structured `click` / `focus` on `current` through
  `agenterm-platform`. Not claimed in the Linux-first slice.

### Degraded fallbacks (never silent)

- [ ] `screenshot` may exist as an observation command but does not replace
  `tree`. When native window capture is unavailable, the command returns typed
  `unsupported`.
- [ ] coordinate `click` requires an explicit degraded marker in the command
  and reply (`addressing: degraded-coordinates`). It is audited separately from
  AT-SPI / UIA / AX actuation.

## Process and session model

- [ ] a session's ownership, lifetime and teardown are explicit. Closing a
  session releases its native resources and its target reference within a
  bounded deadline, and reports incomplete teardown as a typed error rather than
  pretending success.
- [ ] one session's failure, flood or resource exhaustion cannot corrupt another
  session or abort the host.
- [ ] if a backend requires a helper process, its lifecycle, identity and
  failure semantics are owned here and it is never an undeclared background
  authority. Binary-role registration belongs to
  [02 Executable family](PRD_02_02_executable_family.md).

## Reference assets

- [ ] existing reference implementations (the sibling monorepo
  `skills/computer-use/` Windows UIA/CDP and RDP work, the macOS AX/CGEvent
  helper split, the Linux AT-SPI2 bridge) are **design input only**. They inform
  the command set and backend shape; no code, runtime or dependency from them
  enters the product graph. Source review, licensing and independent
  implementation are governed by
  [14 Research provenance](PRD_02_14_research_provenance.md).

## Evidence

- [ ] each tier is proven by a public black-box journey against a real target of
  that tier. A tier proven only in simulation is not claimed.
- [~] Linux `ssh` first cut: host `agenterm-cu --ssh` against loopback OpenSSH
  runs remote `agenterm-cu --target current`. Click path: host
  `send-text --window HANDLE --name Command -- SEED` (payload after `--`; not
  `--text`) plants a seed on a second `agenterm-con` `Command` field; host
  `click --window HANDLE --name SEND` runs remote AT-SPI Action `DoAction`
  (`addressing=accessibility-tree`); host independent
  `get-text --window HANDLE --name Command` returns empty (composer cleared on
  SEND submit). Never screenshot / `--coords` / mouse-drag / XTest. Missing or
  ambiguous name typed-fails `a11y_node_not_found` / `a11y_node_ambiguous` on
  the remote worker the same as local `current`. `set-caret` / `select` /
  `send-keys` / `copy` / `paste --text` / `send-text` over ssh and
  observe-only `wait` / `get-text` / `get-selection` / `get-caret` still hold.
  Worker JSON does not count; CEO owns the official gate. Auth failure and
  missing destination are typed (`ssh_unavailable` / `ssh_transport_failed` /
  `invalid_input`).
- [~] Linux `current` / AT-SPI2: `scripts/cu-linux-smoke.sh` (real `agenterm-cu`, X11
  `DISPLAY`, running `at-spi2-registryd`) proves `tree`, refused unauthorized
  actuation, audited degraded coordinate click, invalid node path failure, and
  structured AT-SPI click when a clickable node exists.
- [x] Windows `current` staged public `cu-windows-smoke` passes its seven
  declared receipts: `cu.windows-host-self-test`,
  `cu.libagenterm-load-cleanup`, `cu.windows-uia-window-identity`,
  `cu.windows-uia-tree`, `cu.windows-uia-name-actuation`,
  `cu.windows-uia-value-wait`, and `cu.windows-uia-cleanup`. This proves the
  staged host/DLL load and cleanup, exact window identity, public UIA tree,
  name-addressed Value/GetText/Invoke journeys and bounded fixture cleanup; it
  does not prove Candidate qualification or release.
- [ ] a cross-tier conformance test proves the same abstract command produces
  equivalent observable results on every tier that declares support for it.
- [ ] capability declaration is tested against reality: a target that declares
  support and then fails the command is a defect, not a runtime condition.
