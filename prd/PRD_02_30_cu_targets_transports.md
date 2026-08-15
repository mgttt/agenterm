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

Branch status (cut 3.48 — Linux `tree` cross-tier conformance; 3.47 per-target
`capabilities`; 3.46 RDP placeholder; 3.45 macOS AX observe stub unchanged):

| Target | Status | Notes |
|--------|--------|-------|
| `current` | **[x]** | Local in-process; Linux/Windows evidence held; `capabilities` names `data.target:"current"` + live libagenterm status; Linux `tree` cross-tier proven (3.48) |
| `ssh` | **[x]** | OpenSSH exec of remote `--target current`; `capabilities` restores public/`data.target:"ssh"`; Linux `tree` cross-tier proven (3.48) |
| `vnc` | **[x]** | RFB + local `--target current` worker; `capabilities` restores public/`data.target:"vnc"`; Linux `tree` cross-tier proven (3.48) |
| `rdp` | **[~]** | PLACEHOLDER: parseable; `capabilities` declares transport placeholder/unavailable + `tree` unsupported with zero I/O; other authorized commands typed `rdp_unavailable`. Transport/session/live evidence empty |

- [x] `current`, `ssh`, and `vnc` are tiers of one family sharing one
  command set. `current` is the **local degenerate tier** — transport is
  in-process — not a temporary prototype to be replaced later. `ssh` first cut
  is OpenSSH `ssh` exec of a remote `agenterm-cu --target current` worker
  (`--ssh <user@host>`; same verbs including actuate; no new verb). `vnc`
  first cut is RFB handshake to `--vnc <host[:port]>` (security type None /
  `x11vnc -nopw`) then a local `agenterm-cu --target current` worker against
  the shared session (`DISPLAY` / AT-SPI env; same verbs; no new verb).
- [~] `rdp` (cut 3.46 PLACEHOLDER + cut 3.47 declaration) is parseable and
  fail-closed for live work. Purpose: remote Windows desktops (and other
  RDP endpoints). Public surface: `TargetRef::Rdp`, `as_str() == "rdp"`,
  `--rdp HOST[:PORT]` (implies `--target rdp`; default port 3389 is
  syntax-only), and explicit `--target rdp`. **`capabilities` (observe)**
  succeeds with a static declaration: `data.target:"rdp"`, transport
  `status:"placeholder"` / `available:false` / `reason:"rdp_unavailable"`,
  verb `capabilities` available, verb `tree` **unsupported** with the same
  reason — **no** socket connect, DNS, TLS/CredSSP, UIA, screenshot, or
  `--coords`. Every other authorized RDP command (reserved first *live*
  observe: `tree --window HANDLE`) returns `ok:false`, `target:"rdp"`, the
  original command name, and `error.code:"rdp_unavailable"` with **no**
  socket connect, TLS/CredSSP/NLA, credential flag, screenshot,
  `--coords`, or silent `ssh`/`vnc`/`current` reuse. `--target rdp`
  without `--rdp`: `capabilities` still declares the tier; other verbs
  return the same typed `rdp_unavailable` family (precise
  missing-endpoint message), not a generic usage/`current` fallback.
  Malformed ports remain `invalid_input`. Transport, session lifecycle,
  authentication, and live UIA-over-RDP evidence are **[ ]** — owned by a
  later Windows agent (see Evidence handoff below). Windows UIA on
  `current` is a separate line and is not promoted by this placeholder.
- [~] `current` ships first. Doing so is the cheapest way to pin the interface,
  because adding a remote transport afterwards changes transport only, not the
  commands above it. `ssh` get-selection evidence reuses the #50 con-publish
  selection observe path over loopback `sshd` against a second `agenterm-con`
  (never steal the resident control socket): host
  `cu --ssh send-text --name Command -- SEED` (payload after `--`; not
  `--text`) plants the seed, host `cu --ssh select --name Command --start N
  --end M` runs remote AT-SPI `Text.SetSelection`, then host independent
  `cu --ssh get-selection --name Command` returns that range
  (`via=get-selection`; start/end equal the selected slice of the seed, or
  the seed when the range is the whole field). Native AT-SPI
  `GetNSelections` + `GetSelection`. Never screenshot, `--coords`, or XTest.
  `vnc` first get-selection evidence reuses the #50 con-publish selection
  observe path over a **gate-owned** loopback `x11vnc` (not the resident
  `:2` listener alone) against a second `agenterm-con` with a unique
  title: `Command` holds a known ASCII seed and a known non-empty
  selection `START..END` (gate precondition via already-landed
  `send-text` + `select`; not this cut's verb), then host independent
  `cu --vnc 127.0.0.1:<port> get-selection --window HANDLE --name Command`
  returns that range (`via=get-selection`; native AT-SPI
  `GetNSelections` + `GetSelection(0)`; `n == 1` and integer `start` /
  `end` equal the precondition range so `seed[start:end] == expected`).
  Never screenshot, `--coords`, mouse-drag, RFB framebuffer OCR, cached
  setter reply, or steal the resident control socket. `get-extents`
  (3.43), `get-caret` (3.42), `tree` (3.41), `focus` (3.40), `scroll`
  (3.39), `click` (3.38), `set-caret` (3.37), `select` (3.36),
  `send-keys` (3.35), `copy` (3.34), `paste --text` (3.33), and
  `send-text` (3.32) over vnc still hold.
- [ ] a target reference is explicit, addressable and stable for the lifetime of
  its session. Enumerating targets remains planned; describing one target's
  declared capabilities is the existing `capabilities` observe verb (cut 3.47).
- [~] capability differences between tiers are **declared, not discovered by
  failure** (cut 3.47). A caller can ask `capabilities` what a target supports
  before acting: `current` reports live libagenterm status;
  `ssh`/`vnc` restore public/`data.target` to the requested tier while
  retaining worker mechanism facts; `rdp` declares transport placeholder and
  does **not** claim `tree` supported. Discovery still requires the observe
  grant and grants no actuation right. Target enumeration remains **[ ]**.
  Linux `current` / `ssh` / `vnc` **`tree` semantic cross-tier conformance is
  proven** (cut 3.48; harness `scripts/cu-linux-cross-tier-tree.sh`); RDP,
  macOS AX, Windows-over-RDP, and other verbs remain open. An unsupported
  command returns typed `Unsupported` / `rdp_unavailable` rather than a fake
  success or a silent coordinate fallback.

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
targets / transports (30)                    legend: [x] shipped  [~] partial  [ ] planned
├── target family
│   ├── current  [x]  local in-process (Linux/Windows live evidence held)
│   ├── ssh      [x]  OpenSSH exec → remote --target current
│   ├── vnc      [x]  RFB handshake → local --target current worker
│   └── rdp      [~]  PLACEHOLDER: capabilities declares unavailable; other verbs rdp_unavailable; live evidence [ ]
└── platform a11y backends (agenterm-platform)
    ├── Linux AT-SPI2                 [x] live evidence (cu-linux-smoke + named journeys)
    ├── Windows UIA                   [~] existing tree evidence (cu-windows-smoke); separate from rdp
    ├── macOS AX current tree         [~] PLACEHOLDER cut 3.45 (code present; live NOT claimed)
    ├── macOS AX actuation            [ ] click / focus / value — not started
    └── RDP remote desktop transport  [ ] not started (session + UIA-over-RDP later Windows cut)
```

**Cut 3.45 boundary:** only the macOS AX **observe** path for
`agenterm-cu --target current tree` is stubbed in `agenterm-platform`
(`adapters/macos/accessibility_tree.rs`, backend string `"ax"`). Typed
failures: `a11y_permission_denied`, `unsupported`, `a11y_tree_timeout`,
bound exhaustion (`a11y_node_limit` / `a11y_depth_limit` / string limits).
No screenshot, no `--coords`, no CGEvent fallback, no silent AT-SPI/UIA
reuse. A unit mock is **not** a live gate. A later **macOS agent** owns
live fixture evidence.

Canonical host mapping (approved product vocabulary):

| Host | Native accessibility stack | Structured `tree` source | Structured actuation |
|------|---------------------------|--------------------------|----------------------|
| Windows | native API + **UIA** | `IUIAutomation` control tree | UIA patterns / legacy accessible (`Invoke`, `LegacyIAccessible`) |
| macOS | **AX** (`NSAccessibility`) | accessibility element tree — **PLACEHOLDER walk this cut** | `AXPress` / `AXRaise` / editable value — **not started** |
| Linux | **AT-SPI2** (`at-spi2-core` / `org.a11y.atspi.*`) | AT-SPI accessible hierarchy — **live** | AT-SPI `Action` / `Component` / `EditableText` — **live** |

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

- [~] **PLACEHOLDER (cut 3.45):** AX-backed `tree` on `current` through
  `agenterm-platform` (`adapters/macos/accessibility_tree.rs`). The adapter
  resolves a `CGWindowID` handle to an application AX element, walks
  `AXChildren` with node/depth/string/time bounds, and returns
  `backend: "ax"` with path ids, role, name, states, bounds, text, and
  declared actions. Missing Accessibility permission fails typed
  (`a11y_permission_denied`); timeout and bound exhaustion fail typed;
  unsupported AX availability is typed `Unsupported`. **Live black-box
  evidence is NOT claimed** on this cut (Linux builder has no Mac window).
  A unit mock does not count. No screenshot / `--coords` / CGEvent fallback
  and no silent AT-SPI or UIA reuse.
- [ ] AX structured `click` / `focus` / value actuation on `current`
  (`AXPress` / `AXRaise` / editable value). Explicitly **not** this cut.

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
- [~] Linux `vnc` first cut: host `agenterm-cu --vnc 127.0.0.1:<port>` against
  a gate-owned loopback `x11vnc` (RFB security type None / `-nopw`; not the
  resident `:2` listener alone) handshakes RFB then runs a local
  `agenterm-cu --target current` session worker. Get-selection observe path:
  second `agenterm-con` `Command` field holds a known ASCII seed and a known
  non-empty selection `START..END` (gate precondition via already-landed
  `send-text` + `select`; not this cut's verb; unique title; never steal
  `unix:/tmp/run-box/agenterm-con.sock`); host independent
  `get-selection --window HANDLE --name Command` returns that range
  (`via=get-selection`; native AT-SPI `GetNSelections` + `GetSelection(0)`;
  `n == 1` and integer `start` / `end` equal the precondition range so
  `seed[start:end] == expected`). Never screenshot / `--coords` /
  mouse-drag / RFB framebuffer OCR / cached setter reply. Missing Text
  typed-fails `a11y_selection_unavailable` on the session worker the same
  as local `current`. `get-extents` / `get-caret` / `tree` / `focus` /
  `scroll` / `click` / `set-caret` / `select` / `send-keys` / `copy` /
  `paste --text` / `send-text` over vnc and observe-only `windows` /
  `get-text` / `wait --text-equals` still hold. Worker JSON does not
  count; CEO owns the official gate. Connect / protocol / auth failures
  are typed (`vnc_unavailable` / `vnc_transport_failed` / `vnc_auth_failed` /
  `invalid_input`).
- [~] Linux `ssh` first cut: host `agenterm-cu --ssh` against loopback OpenSSH
  runs remote `agenterm-cu --target current`. Get-selection observe path: host
  `send-text --window HANDLE --name Command -- SEED` (payload after `--`; not
  `--text`) plants a seed on a second `agenterm-con` `Command` field; host
  `select --window HANDLE --name Command --start N --end M` runs remote
  AT-SPI `Text.SetSelection`; host independent
  `get-selection --window HANDLE --name Command` returns that range
  (`via=get-selection`; start/end equal the selected slice of the seed, or
  the seed when the range is the whole field). Native AT-SPI
  `GetNSelections` + `GetSelection`. Never screenshot / `--coords` /
  mouse-drag / XTest. Missing Text typed-fails `a11y_selection_unavailable`
  on the remote worker the same as local `current`. `get-extents` /
  `get-caret` / `tree` / `focus` / `scroll` / `click` / `set-caret` /
  `select` / `send-keys` / `copy` / `paste --text` / `send-text` over ssh
  and observe-only `wait` / `get-text` still hold. Worker JSON does not
  count; CEO owns the official gate. Auth failure and missing destination
  are typed (`ssh_unavailable` / `ssh_transport_failed` / `invalid_input`).
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
- [~] **macOS AX `current tree` PLACEHOLDER (cut 3.45) — live evidence NOT
  claimed.** Code path exists; a later macOS agent must live-test. Handoff
  recipe (also in `scripts/cu-macos-smoke.sh`, not-yet-run on Darwin):

  1. On a real macOS host, grant Accessibility to the process that will run
     `agenterm-cu` (`AXIsProcessTrusted()` must be true).
  2. Launch a cut-owned native fixture with a unique title and stable PID.
     Hierarchy: one window, one labeled editable text seeded with
     **`345AXTREE`**, one button named **`Fixture Press`**.
  3. Resolve `HANDLE` via existing window enumeration
     (`agenterm-cu --target current --grant observe windows`).
  4. Canonical argv (observe only; no new verb):

     ```sh
     agenterm-cu --target current --grant observe tree --window "$HANDLE"
     ```

  5. Required reply fields: `ok: true`, `target: "current"`,
     `command: "tree"`, data `backend: "ax"`, scoped to `HANDLE`, with
     exactly one fixture text control whose text is `345AXTREE` and one
     `Fixture Press` button under the fixture window.
  6. Permission denial must be typed (`a11y_permission_denied`); timeout /
     bound exhaustion remain typed. Never screenshot, OCR, CGEvent, or
     AT-SPI/UIA reuse. Worker JSON from a Linux box does not count.

- [~] **RDP target PLACEHOLDER (cut 3.46) + declared `capabilities` (cut
  3.47) — live RDP / Windows evidence NOT claimed.** Linux static proof:
  `TargetRef::parse("rdp") == Some(Rdp)`, `as_str() == "rdp"`, `--rdp` /
  `--target rdp` CLI; `capabilities` returns `ok:true` with transport
  placeholder/`rdp_unavailable` and `tree` unsupported without dialing
  (sentinel receives zero connections); every other authorized RDP command
  returns `error.code:"rdp_unavailable"` without dialing. Worker JSON does
  not count; CEO owns the official check. Transport, authentication,
  session lifecycle, and UIA-over-RDP remain empty. **Handoff to a later
  Windows agent** (new cut, not 3.46/3.47):

  1. Choose and document the real boundary: native RDP client/session API
     or an explicitly managed external client; NLA/CredSSP and certificate
     validation; username/domain/secret acquisition without argv leakage;
     desktop/session lifecycle and teardown; cancellation/timeouts; and
     whether a remote session worker is deployed or UIA runs inside the
     logged-on Windows session. Do **not** silently map RDP to SSH, VNC,
     local `current`, screenshots, or coordinates. Do not invent password
     flags until secret-input policy is designed for that cut.
  2. Controlled Windows host/VM with RDP enabled, dedicated test
     account/session, and a cut-owned native fixture (unique title/PID;
     seeded editable text e.g. `346RDPTREE`; named button e.g.
     `Fixture Press`) visible to the remote interactive desktop's UIA.
  3. Resolve `HANDLE` through the RDP target's own window enumeration, then
     reuse the reserved first-observe argv:

     ```sh
     agenterm-cu --rdp "WINDOWS_HOST:3389" \
       --grant observe tree --window "$HANDLE"
     ```

  4. Positive live acceptance: `ok:true`, `target:"rdp"`, `command:"tree"`,
     `backend:"uia"`, unique native fixture nodes; session uniquely owned
     by the gate; no screenshot/OCR/`--coords`/SSH/VNC/`current` fallback.
  5. Negative typed failures (distinct codes as the surface lands): closed
     endpoint, bad credentials, certificate rejection, lost session,
     missing UIA, bogus handle, timeout, bound exhaustion. Until live
     evidence lands, every RDP command **except** the static
     `capabilities` declaration remains `rdp_unavailable` and the PRD
     branch stays `[~]` placeholder.

- [x] **Linux `tree` cross-tier conformance (cut 3.48).** Same abstract
  `tree --window HANDLE` on one cut-owned second `agenterm-con` through
  `current`, loopback SSH, and dedicated loopback VNC. Semantic compare
  (not byte-for-byte): public target restored per tier; `backend:"at-spi2"`;
  same window / root / node count; exactly one showing `Command`, `SEND`,
  and `OffscreenField`; same role/name/text/actions/parent/stable path;
  bounds equal absent movement (bounded full-set retry on desktop churn);
  volatile focus states may differ by observation timing. Harness:
  `scripts/cu-linux-cross-tier-tree.sh` → `live/348-cross-tier-tree.json`.
  **Not claimed:** RDP tree, macOS AX live, Windows UIA-over-RDP, other
  verbs, screenshots/`--coords`. A mismatch is
  `cross_tier_conformance_failed`.
- [~] **Per-target `capabilities` declaration (cut 3.47).** Existing observe
  verb only (no new verb, no target enumeration). Load-bearing:
  `--rdp … capabilities` → `ok:true` / `target:"rdp"` / transport
  placeholder unavailable / `tree` not supported / zero sentinel
  connections; `--rdp … tree` still `rdp_unavailable`; `current` /
  loopback `ssh` / dedicated loopback `vnc` each name their public tier
  on both reply and `data.target` (no `data.target:"current"` leak for
  ssh/vnc requests); no tier declares unproven macOS AX or live RDP as
  available; missing observe grant remains `refused`. Full
  declare-vs-reality conformance across every verb remains **[ ]**: a
  target that declares support and then fails the command is a defect,
  not a runtime condition. Linux `tree` is the first verb closed under
  cut 3.48.
