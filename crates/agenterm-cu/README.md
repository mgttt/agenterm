# agenterm-cu

`agenterm-cu` is AgenTerm's computer-use foundation: a target-agnostic command
surface for orchestrator agents to observe and actuate a desktop through
structured data instead of screenshot/OCR coordinate guessing.

## Intended agent loop

Orchestrator agents (not humans staring at pixels) should run:

```text
loop until goal:
  observe structured state (windows, control tree, typed capabilities)
  act by structured identity (window + node path, or window + accessible name)
    click / focus / send-text / paste / send-keys all take --name, so no step parses node ids
  wait on observable conditions with bounded timeouts — never sleep
```

`agenterm-cu` is capability, not judgment: no planner, model, or agent loop ships here.

Named window placement (`window-place`) is in the command enum. Geometry
follows the Spectacle catalog
([PRD 32](../../prd/PRD_02_32_cu_window_placement.md)). Apply uses
`libagenterm` `agt_native_window_*` (runtime dynamic library). Requires `--grant actuate`.

## Native accessibility mapping (按图索骥)

| Concern | Windows | Linux (`current` slice) | macOS (planned) |
|---------|---------|------------------------|-----------------|
| Window list | Win32 `EnumWindows` | X11 `_NET_CLIENT_LIST` | `AXUIElement` application windows |
| Control tree | **UIA** (`IUIAutomation`) | **AT-SPI2** (`org.a11y.atspi.*` on D-Bus) | **AX** (`NSAccessibility`) |
| Node identity | automation id + runtime id + bounds | path id (`/0/2/5`) + role + name + bounds | AX path + role + title + bounds |
| Node click/focus | `InvokePattern` / `LegacyIAccessible` | AT-SPI `Action` (`click`/`press`, else default `DoAction(0)`); no Action → Component `GetExtents` + `GenerateMouseEvent`; focus is `focus` / `Component::grab_focus` | `AXPress` / `AXRaise` |
| Text entry | `ValuePattern` / `SendInput` | AT-SPI `EditableText` (`SetTextContents` / `InsertText`) for `--name`; `Text` + toolkit set-value when EditableText is absent (Chrome AX, WebKitGTK eval helper); `input-inject` only without `--name` | AX value + events |
| Screenshot | GDI native capture | typed `unsupported` (no OCR substitute) | typed `unsupported` (planned) |

Linux `tree` and structured `click` / `focus` use **AT-SPI2 only**. If the
accessibility bus is unavailable (no session bus, headless without a11y), commands
return typed `unsupported` / `failed` — never a silent coordinate fallback.

On this Linux box, start Chrome with `scripts/box-chrome-a11y.sh` so
`--force-renderer-accessibility` is always on (AT-SPI renderer subtree).
Start Reasonix with `scripts/reasonix-desktop-a11y.sh` so WebKit keeps an
AT-SPI subtree (`WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1`) and the eval
helper can implement the missing `EditableText` set-value; otherwise the
web process aborts and `agenterm-cu tree` is only unnamed GTK fillers.
`agenterm-con` registers as an AT-SPI toolkit and publishes inner chrome
(`Command`, `SEND`, `Tabs`, `Session`); do not treat the one-node X11 title
frame as its success path.

Coordinate clicks remain available only with explicit `--degraded` and are
audited separately from AT-SPI actuation.

## Linux `current` slice

| Command | Backend |
|---------|---------|
| `windows` | X11 window enumeration (`libagenterm agt_window_enumerate`) |
| `tree` | AT-SPI2 flattened control tree with role, name, states, bounds, actions |
| `click --node <path>` | AT-SPI2 `Action` (`click` / `press`, else default `DoAction(0)` when the node exposes Action); no Action → Component `GetExtents` + AT-SPI mouse (`addressing` stays `accessibility-tree`) |
| `click --window --name PAT [--role ROLE]` | same showing/visible name matcher as `wait --node-name-contains` (exactly one hit), then the `--node` AT-SPI path (never silent `--coords`) |
| `focus --node <path>` | AT-SPI2 `focus` action or `Component::grab_focus` |
| `focus --window --name PAT [--role ROLE]` | same unique-name matcher, then the `--node` AT-SPI focus path |
| `click --coords X,Y --degraded` | XTest (explicit degraded mode only) |
| `send-text` / `send-keys` without `--window` | XTest keyboard injection into whatever is focused |
| `send-text --window --name PAT [--role ROLE]` | same unique-name matcher, then native AT-SPI `EditableText` (`SetTextContents` / `InsertText`); Chrome/WebKitGTK named fields expose `Text` but not `EditableText` — those write through AT-SPI `Text` + toolkit set-value and are confirmed by `GetText`; no writeable text interface → typed `a11y_text_unavailable` (never XTest) |
| `send-text --window HANDLE` (no `--name`) | same innermost focused Text node `get-text --window` reads, then `agt_a11y_node_set_text`; never XTest when `--window` is set. con `Command` after `focus --name` (`via=editable-text`); Chrome `GetTextField` after `focus --name`; Reasonix composer `Message Reasonix…` after `focus --name` / `click --name` (eval-helper set-value). Proof is independent `get-text --window` (no `--name`) |
| `copy --window --name PAT [--role ROLE]` | same unique-name matcher, then AT-SPI `Text.GetText` published onto the native clipboard (`agt_clipboard_set_text`; Linux X11 `SetSelectionOwner`, not xclip); no Text interface → typed `a11y_text_unavailable` (never XTest / `--coords`) |
| `copy --window HANDLE` (no `--name`) | same innermost focused Text node `get-text --window` reads, then GetText published onto native CLIPBOARD (`agt_clipboard_set_text`, `via=gettext`); never XTest when `--window` is set. con `Command` after `focus --name` (`via=gettext`, second con only — never steal the resident control socket); Chrome `GetTextField` after `focus --name`; Reasonix composer `Message Reasonix…` after `focus --name` under `scripts/reasonix-desktop-a11y.sh` (`via=gettext`). Proof is independent seed → focused copy → clear → focused paste → `get-text --window` (no `--name`) equal to the seeded string |
| `paste --window --name PAT [--role ROLE] [--text TEXT]` | same unique-name matcher, then clipboard (`agt_clipboard_get_text`, optional `--text` seed) written through that same AT-SPI `EditableText` / `Text` path; no writeable text interface → typed `a11y_text_unavailable` (never XTest / `--coords`) |
| `paste --window HANDLE` (no `--name`) | same innermost focused Text node `get-text --window` reads, then clipboard write via `agt_a11y_node_set_text`; never XTest when `--window` is set. con `Command` after `focus --name` (`via=editable-text`, second con only — never steal the resident control socket); Chrome `GetTextField` after `focus --name`; Reasonix composer `Message Reasonix…` after `focus --name` under `scripts/reasonix-desktop-a11y.sh` (eval-helper set-value, `via=text`). Proof is independent `get-text --window` (no `--name`) equal to the clipboard string |
| `send-keys --window --name PAT [--role ROLE]` | same unique-name matcher, then native AT-SPI Device/key events (`DeviceEventListener.NotifyEvent`); no key interface → typed `a11y_key_unavailable` (never XTest) |
| `send-keys --window HANDLE` (no `--name`) | same innermost focused Text node `get-text --window` reads; prefer `DeviceEventListener.NotifyEvent` (`via=device-event`); plain typeable text falls back to AT-SPI `EditableText` / `Text` write when that interface is absent (con `Command` after `focus --name`, `via=editable-text`; Chrome `GetTextField` after `focus --name`, `via=text`; Reasonix composer `Message Reasonix…` after `focus --name`, eval-helper set-value, `via=text`). Never XTest when `--window` is set. Proof is independent `get-text --window` (no `--name`) |
| `scroll --window --name PAT [--role ROLE]` | same unique-name matcher, then one-shot AT-SPI `Component.ScrollTo(TopEdge)`; missing/false/`UnknownMethod` → typed `a11y_scroll_unavailable`; never Action `scroll*` / XTest wheel / `--coords` |
| `get-extents --window --name PAT [--role ROLE]` | same unique-name matcher, then independent AT-SPI `Component.GetExtents(Screen)`; snapshot `node.bounds` do not count; empty extents → typed `a11y_extents_unavailable` |
| `select --window --name PAT --start N --end M [--role ROLE]` | same unique-name matcher, then one-shot AT-SPI `Text.SetSelection`; missing Text/`UnknownMethod` → typed `a11y_selection_unavailable`; SetSelection false → typed `a11y_selection_no_effect`; never XTest / mouse-drag / `--coords` |
| `get-selection --window --name PAT [--role ROLE]` | same unique-name matcher, then independent AT-SPI `GetNSelections` + `GetSelection(0)`; `select` reply does not count; missing Text → typed `a11y_selection_unavailable`; `n==0` is empty success |
| `set-caret --window --name PAT --offset N [--role ROLE]` | same unique-name matcher, then one-shot AT-SPI `Text.SetCaretOffset`; missing Text/`UnknownMethod` → typed `a11y_caret_unavailable`; SetCaretOffset false → typed `a11y_caret_no_effect`; never XTest / `--coords` |
| `get-caret --window --name PAT [--role ROLE]` | same unique-name matcher, then independent AT-SPI `CaretOffset` / `GetCaretOffset`; `set-caret` reply does not count; missing Text → typed `a11y_caret_unavailable` |
| `get-text --window --name PAT [--role ROLE]` | same unique-name matcher, then one-shot independent AT-SPI `Text.GetText` — the same authority `wait --text-equals` polls, without a timeout; `send-text` / `paste` / `copy` `matched.text`, `last_text_write_via`, the WebKit eval helper queued-job `OK`, and tree snapshot `text` do not count; missing Text → typed `a11y_text_unavailable` (never XTest / `--coords` / screenshot) |
| `get-text --window HANDLE` (no `--name`) | innermost showing `focused` node that exposes `Text.GetText`; `via=gettext`. After focused `send-text --window`, this must equal the typed string |
| `screenshot` | typed `unsupported` on Linux native capture |
| `wait` | polls window state, or the AT-SPI tree for `--node-name-contains` (2+ showing hits → `a11y_node_ambiguous`), or AT-SPI `Text.GetText` for `--text-equals` / `--node-text-equals` / `--text-contains` / `--node-text-contains` with `--name` (not `send-text` / `paste` / `copy` `matched.text`, not a sidecar tree `text`, not the WebKit eval helper `OK`) |

### Tree JSON shape (UIA-like)

```json
{
  "degraded": false,
  "backend": "at-spi2",
  "addressing": "accessibility-tree",
  "root_id": "/0",
  "nodes": [
    {
      "id": "/3/0/0/1/0",
      "parent_id": "/3/0/0/1",
      "role": "toggle button",
      "name": "Applications",
      "states": ["enabled", "visible"],
      "bounds": {"x": 8, "y": 0, "width": 26, "height": 28},
      "actions": ["Click"],
      "text": null
    }
  ]
}
```

Node ids are **child-index paths** from each application root (`/0`, `/1`, …).
Re-query `tree` after navigation if the UI mutates; actuation resolves the path
at call time and returns `a11y_node_not_found` when the path is stale.

## Authorization and audit

Every command requires an explicit `--target current` or `--ssh <user@host>`
(which implies `--target ssh`). Observation commands need the `observe` grant;
actuation commands need `actuate`. Grants come from `--grant` or
`AGENTERM_CU_GRANT` (comma-separated). Local `current` is not exempt.

The `ssh` tier reuses the same verbs (observe and actuate). Host
`agenterm-cu --ssh` rewrites the command to `target=current` and runs a remote
`agenterm-cu exec --json -` worker over OpenSSH stdio. Click evidence is
loopback `sshd` plus a second `agenterm-con`: host `send-text --window HANDLE
--name Command -- SEED` (payload after `--`; not `--text`) plants the seed,
host `click --window HANDLE --name SEND` runs remote AT-SPI Action `DoAction`
(`addressing=accessibility-tree`), then host independent
`get-text --window HANDLE --name Command` returns empty (composer cleared on
SEND submit). Never screenshot / `--coords` / mouse-drag / XTest. Missing or
ambiguous name typed-fails `a11y_node_not_found` / `a11y_node_ambiguous` on the
remote worker the same as local `current`. `set-caret` / `select` /
`send-keys` / `copy` / `paste --text` / `send-text` over ssh and observe-only
`wait` / `get-text` / `get-selection` / `get-caret` remain valid too.

Unauthorized actuation returns `refused`, distinct from `unsupported` and
mechanism failures. Authorized actuation is appended to a JSONL audit log
(`AGENTERM_CU_AUDIT_PATH`, default `~/.local/share/agenterm/cu-audit.jsonl`).
If the audit path cannot be written, actuation does not execute.

## Examples

```bash
# Declare capabilities (observe grant)
agenterm-cu --target current --grant observe capabilities

# Same verbs over OpenSSH (remote agenterm-cu --target current worker).
# Click path: send-text SEED → click --name SEND → get-text --name Command == "".
agenterm-cu --ssh user@127.0.0.1 --ssh-port 2222 --ssh-identity ~/.ssh/id_ed25519 \
  --grant observe,actuate send-text --window HANDLE --name Command -- SEED
agenterm-cu --ssh user@127.0.0.1 --ssh-port 2222 --ssh-identity ~/.ssh/id_ed25519 \
  --grant actuate click --window HANDLE --name SEND
agenterm-cu --ssh user@127.0.0.1 --ssh-port 2222 --grant observe \
  get-text --window HANDLE --name Command

# List top-level windows
agenterm-cu --target current --grant observe windows

# AT-SPI control tree (all application roots)
agenterm-cu --target current --grant observe tree

# Scoped tree for one X11 window handle
agenterm-cu --target current --grant observe tree --window 0x3c00007

# Structured click by node path (AT-SPI)
agenterm-cu --target current --grant actuate click --node /3/0/0/1/0

# Structured click / focus by accessible name — no tree-dump parsing, no --coords.
# Two or more showing hits fail typed (`a11y_node_ambiguous`) instead of picking the first.
agenterm-cu --target current --grant observe,act click --window 25165828 --name Reload
agenterm-cu --target current --grant observe,act focus --window 25165828 --name Reload --role button

# Structured focus
agenterm-cu --target current --grant actuate focus --node /3/0/0/1/0

# Type into a control by accessible name — focuses that node first, then types.
# `--` ends flag parsing so the text may start with a dash.
agenterm-cu --target current --grant observe,act send-text --window 25165828 \
  --name "Address and search bar" -- hello

# Send a chord to a control by accessible name — same matcher, focus, then keys.
agenterm-cu --target current --grant observe,act send-keys --window 25165828 \
  --name "Address and search bar" -- enter

# Wait for at least one window, 3s max
agenterm-cu --target current --grant observe wait --timeout-ms 3000 --window-count-gte 1

# Wait for a control to appear in one window's accessibility tree (no screenshot).
# The handle is the decimal `handle` from `agenterm-cu windows`; a match needs a showing
# (or visible) node, and a timeout is a typed `ok:false` / `error.code=timeout`.
agenterm-cu --target current --grant observe wait --timeout-ms 4000 --window 25165828 \
  --node-name-contains Reload --node-role button

# Copy a named field's AT-SPI GetText onto the native clipboard.
# Linux X11 uses SetSelectionOwner (not xclip). Never XTest / --coords.
# Close the circuit with paste --name (no --text) then wait --text-equals.
# Chrome fixture and Reasonix composer (name contains "Message Reasonix")
# both use the same GetText → CLIPBOARD path.
agenterm-cu --target current --grant observe,act copy --window 25165828 \
  --name FixtureSource
agenterm-cu --target current --grant observe,act copy --window 4194318 \
  --name "Message Reasonix"

# Paste clipboard text into a named field via AT-SPI EditableText / Text.
# --text seeds the clipboard (Linux X11: native CLIPBOARD owner, not xclip);
# the field write always reads the clipboard. Never XTest / --coords.
# Close the circuit with wait --text-equals GetText; paste matched.text
# does not count. Reasonix composer name contains "Message Reasonix"
# (WebKit Text-without-EditableText uses the eval-helper set-value path).
# After a prior copy --name, omit --text so ConvertSelection supplies SRC.
agenterm-cu --target current --grant observe,act paste --window 25165828 \
  --name FixtureField --text hello
agenterm-cu --target current --grant observe,act paste --window 4194318 \
  --name "Message Reasonix"

# After send-text / paste / copy --name, wait until AT-SPI GetText equals the source
# or contains a substring. Independent of send-text / paste / copy matched.text, of
# a sidecar tree walk, and of the WebKit eval helper's queued-job OK
# (Reasonix composer: Message Reasonix…).
agenterm-cu --target current --grant observe wait --timeout-ms 4000 --window 25165828 \
  --name FixtureField --text-equals hello
agenterm-cu --target current --grant observe wait --timeout-ms 4000 --window 25165828 \
  --name FixtureField --text-contains GATE
agenterm-cu --target current --grant observe wait --timeout-ms 4000 --window 4194318 \
  --name "Message Reasonix" --text-equals hello

# Select a range on a named Text node (AT-SPI SetSelection). Observe with
# independent get-selection (GetNSelections + GetSelection), not the
# select reply. Chrome fixture and Reasonix composer both use native
# Text methods — no eval-helper select path, never mouse-drag / --coords.
cu --target current --grant actuate select --window 25165828 \
  --name SelectField --start 0 --end 4
cu --target current --grant observe get-selection --window 25165828 \
  --name SelectField
cu --target current --grant actuate select --window 4194318 \
  --name "Message Reasonix" --start 0 --end 4
cu --target current --grant observe get-selection --window 4194318 \
  --name "Message Reasonix"

# Place the caret on a named Text node (AT-SPI SetCaretOffset). Observe
# with independent get-caret (CaretOffset / GetCaretOffset), not the
# set-caret reply. Chrome fixture and Reasonix composer both use native
# Text methods — no eval-helper caret path, never --coords / XTest.
cu --target current --grant actuate set-caret --window 25165828 \
  --name CaretField --offset 2
cu --target current --grant observe get-caret --window 25165828 \
  --name CaretField
cu --target current --grant actuate set-caret --window 4194318 \
  --name "Message Reasonix" --offset 2
cu --target current --grant observe get-caret --window 4194318 \
  --name "Message Reasonix"

# One-shot independent readback of a named Text node (AT-SPI GetText) —
# the same authority wait --text-equals polls, without a timeout.
# send-text / paste / copy matched.text and tree snapshot text do not
# count. Chrome fixture and Reasonix composer both use native Text
# GetText — no eval-helper get-text path, never XTest / --coords.
cu --target current --grant observe get-text --window 25165828 \
  --name GetTextField
cu --target current --grant observe get-text --window 4194318 \
  --name "Message Reasonix"

# Place the focused window (Spectacle catalog)
agenterm-cu --target current --grant actuate window-place --action left-half

# Refused without actuate grant
agenterm-cu --target current --grant observe send-text hello

# Audited coordinate click (explicit degraded mode only)
agenterm-cu --target current --grant actuate click --coords 100,200 --degraded

# JSON command envelope
agenterm-cu exec --grant observe,actuate --json '{"verb":"windows","target":"current"}'
```

## macOS hotkeys host (`AgentermCu`)

Replace Spectacle with `./scripts/install-cu-hotkeys.sh` (macOS). That installs
`~/Applications/AgentermCu.app`, a LaunchAgent (`com.agenterm.cu.hotkeys`), a
menu-bar extra, and Spectacle-default global shortcuts. Geometry still goes
through `window-place` + platform AX set-rect.

### Accessibility is signature + process

- System Settings showing **AgentermCu** ON is not enough. Runtime trust is
  `AXIsProcessTrusted()` for the **launchd** process against the **current**
  code signature (ad-hoc installs use a cdhash requirement).
- Reinstall re-signs. `install-cu-hotkeys.sh` runs
  `tccutil reset Accessibility com.agenterm.cu` so a stale ON cannot outlive
  a new signature. Enable **AgentermCu** once after each reinstall. Prefer that
  row over a legacy path entry named `agenterm-cu`.
- A successful `agenterm-cu window-place` from Terminal does **not** prove hotkeys work:
  the CLI may borrow Terminal’s Accessibility grant. Check the host:

```bash
cat ~/.local/share/agenterm/ax-status   # expect trusted=1 after grant
grep ax_trusted ~/.local/share/agenterm/cu-hotkeys.log
# optional: codesign -d -r- ~/Applications/AgentermCu.app
```

### UX rules

- No popup card and no background TCC poll.
- Menu first item is Accessibility status; it is refreshed when the menu opens
  and opens Settings when clicked.

```bash
./scripts/install-cu-hotkeys.sh
# then enable AgentermCu in Accessibility once; try ⌥⌘←
```

Engineering detail:
[`docs/agenterm-rust-cheatsheet.md`](../../docs/agenterm-rust-cheatsheet.md)
(section *macOS Accessibility trust is signature + process*).

## Black-box evidence

From the repository root on a host with `DISPLAY` set (X11 or Xvfb) and a
running AT-SPI registry (`at-spi2-registryd`):

```bash
./scripts/cu-linux-smoke.sh
```

## Layering

```text
native primitive     libagenterm dynamic library (agenterm.dll — `agt_*` exports)
    ↑
abstract command     agenterm-cu library (`Command`, typed `CuReply`)
    ↑
current transport    in-process `Executor` for target `current`
    ↑
shell + host         single `agenterm-cu` binary
```

`agenterm-cu` never opens raw OS APIs. Every call goes through the shared
libagenterm dynamic library; mechanisms report typed `Available` /
`Unsupported` / `Failed`.

## Executable and desktop host

`agenterm-cu` is the only executable. CLI commands and `agenterm-cu host` are
modes of that binary; there is no separate `cu` product surface. CU is the first
runtime consumer of the `libagenterm` dynamic library.

macOS hosts the placement catalog in `AgentermCu.app`. Windows desktop-host ABI
1.7 now implements a notification-area menu, `RegisterHotKey`, event polling
and cleanup for 18 placement actions plus Quit. Native `target/abi-dev`
`host --self-test --json` reports `actions=19` and `cleaned_up=true`. Formal
`dist` staging and Candidate qualification are still in progress, so Windows
delivery remains partial.