# `agenterm-cu` command surface and layering

Parent: [Computer-use foundation (`agenterm-cu`)](PRD_02_28_agenterm_cu.md)

Delivery truth: the command surface and desktop host are modes of the single
`agenterm-cu` executable. CU is the first runtime `libagenterm` consumer; this
module owns abstract command and action meaning, while the dynamic ABI and
platform adapters own native mechanism. Formal `dist`/Candidate qualification
is still in progress, so implemented command leaves do not make the root
product shipped.
This module owns the abstract command set every target must honor, the layering
contract that keeps it from rotting, the structured control-tree observation
model, and the determinism rules. It does not own transports
([30](PRD_02_30_cu_targets_transports.md)) or authorization
([31](PRD_02_31_cu_authorization_safety.md)).

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Layering contract

- [ ] the surface is layered and **outer may depend on inner only**:

  ```text
  native primitive   平台机制（agenterm-platform 拥有，cu 不实现）
     ↑
  abstract command   目标无关命令集（本模块拥有）
     ↑
  target selector    选目标/transport（30 拥有）
     ↑
  workflow           组合动作、重试、等待编排
     ↑
  shell command      公开 CLI 入口
  ```

- [ ] a layer never reaches past its neighbor. A workflow may not call a native
  primitive; a shell command may not encode target-specific behavior. A
  violation is a structural defect, not a style preference.
- [ ] the abstract command set is target-agnostic by construction: a command
  whose semantics only make sense for one transport does not belong in it.

## Abstract command set

- [ ] the initial set covers observation and actuation:
  screenshot; window enumeration; control-tree enumeration; pointer
  press/release/move/click/drag; wheel; keyboard text and named keys; clipboard
  read/write; file transfer in both directions; **named window placement**
  (`window-place`, owned by [32](PRD_02_32_cu_window_placement.md)).
- [ ] every command carries an explicit target reference and returns a typed
  result. There is no ambient "current target" that a caller can forget to set.
- [ ] verb spellings converge with the existing AgenTerm surfaces where the
  action is the same (`screenshot`, `send-text`, `send-keys`, `send-wheel`,
  pointer verbs). A shared spelling must mean the same product action; where cu
  cannot honor an existing verb it omits it rather than shipping a weaker
  impostor. The workbench CLI contract is
  [15](PRD_02_15_command_line.md); the con contract is
  [26](PRD_02_26_con_control_cli.md).
- [ ] machine-readable output is the primary interface and the human rendering
  is derived from it, never the reverse.

## Structured observation

- [ ] a control-tree observation returns stable per-node identity, role, label,
  state and **exact bounds** — not a bitmap the caller must interpret.
- [ ] `tree` is sourced from the host's **platform accessibility backend**
  ([30 § Platform accessibility backends](PRD_02_30_cu_targets_transports.md#platform-accessibility-backends)):
  Windows native API + UIA, macOS AX (`NSAccessibility`), Linux AT-SPI2.
  `agenterm-cu` does not implement these stacks; it consumes `agenterm-platform`.
- [ ] node identity is stable enough to be re-addressed across observations, or
  the instability is reported. An agent must never silently act on a node whose
  identity has been recycled.
- [ ] where a target cannot expose a control tree, the response says so
  explicitly with typed `Unsupported` / `Failed`. Coordinate-only or
  screenshot-only operation is always visible in the result, never inferred by
  the caller.
- [ ] structured `click` / `focus` by node id use the same platform a11y
  backend as `tree`. Coordinate `click` is a separate degraded path requiring
  an explicit marker; it never substitutes silently when structured actuation
  was requested.
- [~] structured `click` / `focus` also accept an accessible name
  (`--window` + `--name` + optional `--role`). Resolution reuses the
  `wait --node-name-contains` matcher (showing/visible, case-insensitive
  substring) and then acts on the existing node-path a11y path. A miss is
  typed `a11y_node_not_found`. Two or more showing matches are typed
  `a11y_node_ambiguous` (with the match count); the command must not pick
  the first. Name addressing must not parse tree dumps, take screenshots,
  or fall through to `--coords`. A showing named node with no Action
  still uses the AT-SPI Component path and reports
  `addressing=accessibility-tree`.
- [x] `send-text` accepts the same name addressing (`--window` + `--name` +
  optional `--role`, with `--` ending flag parsing). Named write goes
  through native AT-SPI `EditableText` (`SetTextContents` / `InsertText`)
  when that interface exists, otherwise through AT-SPI `Text` plus the
  toolkit accessibility set-value (Chrome 151 and WebKitGTK 2.52 implement
  `Text` but not `EditableText`). Success is confirmed by `Text.GetText` and reports
  `addressing=accessibility-tree`. A named showing node with no writeable
  text interface typed-fails (`a11y_text_unavailable`) and never falls
  through to XTest / `input_inject::type_text`. A miss or an ambiguous
  name writes nothing and fails typed, so a loop-until caller never
  sprays text at the wrong control.
- [x] `send-text --window HANDLE` without `--name` writes that same
  AT-SPI path on the showing focused node (innermost `Text.GetText`
  candidate — the same node `get-text --window HANDLE` reads). Never
  XTest / `input_inject::type_text` when `--window` is set. Proof is
  independent `get-text --window HANDLE` (no `--name`) equal to the
  typed string, not `send-text` `matched.text`. Live hosts: agenterm-con
  named `Command` (native `EditableText`, `via=editable-text` on a
  second con that never steals the resident control socket), Chrome
  `GetTextField` (`fixtures/cu/311b-chrome-gettext.html`) after
  `focus --name`, and Reasonix composer `Message Reasonix…` under
  `scripts/reasonix-desktop-a11y.sh` (WebKit 2.52 has `Text` but not
  `EditableText`; write is AT-SPI `Text` plus the eval-helper
  set-value). Without `--window` it stays the plain "type into
  whatever is focused" inject. Do not mark this leaf shipped on
  worker JSON.
- [x] `copy --window HANDLE --name PAT [--role ROLE]` copies AT-SPI
  `Text.GetText` (`agt_a11y_node_get_text`) from the unique showing named
  node onto the native clipboard (`agt_clipboard_set_text`) and reports
  `addressing=accessibility-tree` / `via=gettext`. On Linux X11 the seed
  is a native CLIPBOARD selection owner (`SetSelectionOwner`), not
  `xclip` / `xsel`. `--name` is required. A named showing node with no
  Text interface typed-fails (`a11y_text_unavailable`) and never falls
  through to XTest / `--coords` / screenshot. A miss or an ambiguous name
  copies nothing. Close the circuit with `paste --name` (no `--text`)
  then `wait --text-equals`; `copy` `matched.text` does not count. Live
  evidence: Chrome fixture fields and the Reasonix composer
  (`Message Reasonix…`) under `scripts/reasonix-desktop-a11y.sh`.
- [x] `paste --window HANDLE --name PAT [--role ROLE] [--text TEXT]` writes
  the clipboard into the unique showing named field through that same
  native AT-SPI `EditableText` / `Text` path (`agt_a11y_node_set_text`)
  and reports `addressing=accessibility-tree`. `--text` only seeds the
  clipboard (`agt_clipboard_set_text`); the field write always reads
  `agt_clipboard_get_text`. On Linux X11, `--text` seeds CLIPBOARD
  through the native selection owner (not `xclip`). A named showing node
  with no writeable text interface typed-fails (`a11y_text_unavailable`)
  and never falls through to XTest / `--coords` / screenshot. A miss or
  an ambiguous name writes nothing. Close the circuit with
  `wait --text-equals`; `paste` `matched.text` does not count. Live
  Reasonix composer (`Message Reasonix…`) uses the same WebKit
  eval-helper set-value path as named `send-text`. A prior `copy --name`
  may seed the clipboard instead of `--text`.
- [x] `paste --window HANDLE` without `--name` writes that same clipboard
  path on the showing focused node (innermost `Text.GetText` candidate —
  the same node `get-text --window HANDLE` reads). Never XTest /
  `--coords` / screenshot when `--window` is set. Proof is independent
  `get-text --window HANDLE` (no `--name`) equal to the clipboard string,
  not `paste` `matched.text`. Live hosts: Chrome `GetTextField`
  (`fixtures/cu/311b-chrome-gettext.html`) after `focus --name` (optional
  `--text` seeds CLIPBOARD); Reasonix composer and agenterm-con `Command`
  share the same focused write path as focused `send-text`. Without
  `--window` paste is invalid (no plain inject paste). Do not mark this
  leaf shipped on worker JSON.
- [x] `send-keys` accepts that same name addressing (`--window` + `--name` +
  optional `--role`, with `--` ending flag parsing). Named chords go through
  native AT-SPI Device/key events (`DeviceEventListener.NotifyEvent`) and
  report `addressing=accessibility-tree`. A named showing node with no key
  interface typed-fails (`a11y_key_unavailable`) and never falls through to
  XTest / `input_inject::send_keys`. A miss or an ambiguous name sends no
  chord at all.
- [x] `send-keys --window HANDLE` without `--name` targets the showing
  focused node (innermost `Text.GetText` candidate — the same node
  `get-text --window HANDLE` reads). Prefers `DeviceEventListener.NotifyEvent`
  (`via=device-event`). When that interface is absent (con `Command`;
  Chrome renderer entry; WebKitGTK textarea) and the payload is plain
  typeable text, writes through the same AT-SPI `EditableText` / `Text`
  path as focused `send-text`. Never XTest / `input_inject::send_keys`
  when `--window` is set. Proof is independent `get-text --window HANDLE`
  (no `--name`) equal to the typed string. Live hosts: agenterm-con
  named `Command` (native `EditableText`, `via=editable-text` on a
  second con that never steals the resident control socket), Chrome
  `GetTextField` (`fixtures/cu/311b-chrome-gettext.html`) after
  `focus --name` (`via=text`); Reasonix composer `Message Reasonix…`
  after `focus --name` under `scripts/reasonix-desktop-a11y.sh`
  (eval-helper set-value, `via=text`, same as focused `send-text`).
  Special chords without a key interface still typed-fail. Without
  `--window` it stays the plain "send to whatever is focused" inject.
  Do not mark this leaf shipped on worker JSON.
- [x] `wait --window HANDLE --name PAT [--role ROLE] --text-equals TEXT`
  (alias `--node-text-equals`) polls AT-SPI `Text.GetText` on the unique
  showing named node until that independent text equals `TEXT`. Timeout is
  typed `timeout` and reports the last GetText. `send-text` / `paste` /
  `copy` `matched.text`, a sidecar `tree` walk of snapshot `text` fields,
  `last_text_write_via`, and the WebKit eval helper's queued-job `OK` are
  not this condition.
  Never screenshot, XTest, or `--coords`. Live evidence includes Chrome
  `FixtureField` and the Reasonix composer (`Message Reasonix…`).
- [x] `wait --window HANDLE --name PAT [--role ROLE] --text-contains SUB`
  (alias `--node-text-contains`) polls that same independent
  `Text.GetText` until it contains `SUB`. Success publishes `via=gettext`
  and the full GetText (not only the substring). `send-text` / `paste` /
  `copy` `matched.text` do not count. Timeout is typed `timeout` and
  reports the last GetText. Never screenshot, XTest, or `--coords`.
- [x] `scroll --window HANDLE --name PAT [--role ROLE]` is one-shot AT-SPI
  `Component.ScrollTo(TopEdge)` (`agt_a11y_node_scroll`) on the unique
  showing named node and reports `addressing=accessibility-tree` /
  `via=scroll-to`. `--name` is required. Missing / false /
  `UnknownMethod` typed-fails (`a11y_scroll_unavailable`). ScrollTo true
  with no later independent geometry change is `a11y_scroll_no_effect`,
  not `timeout`. Never Action `scroll*`, XTest wheel, `--coords`, or
  screenshot. `matched.extents` / snapshot `node.bounds` do not count.
  WebKitGTK `ScrollTo` returns true without moving geometry; when the
  Reasonix eval helper is present the same verb applies
  `scrollIntoView({block:'start'})` (no ABI change).
- [x] `get-extents --window HANDLE --name PAT [--role ROLE]` reads
  independent AT-SPI `Component.GetExtents(Screen)`
  (`agt_a11y_node_get_extents`) for that unique showing named node.
  Snapshot `node.bounds` (hardcoded `0,0,0,0` during tree walk) do not
  count. Empty extents (w/h <= 0 or call fail) typed-fail
  (`a11y_extents_unavailable`). Never screenshot / XTest / `--coords`.
- [x] `select --window HANDLE --name PAT --start N --end M [--role ROLE]`
  is one-shot AT-SPI `Text.SetSelection(0, start, end)`
  (`agt_a11y_node_set_selection`) on the unique showing named node and
  reports `addressing=accessibility-tree` / `via=set-selection`.
  `--name` is required. Missing Text / `UnknownMethod` typed-fails
  (`a11y_selection_unavailable`). SetSelection false is
  `a11y_selection_no_effect`, not `timeout`. Miss / ambiguous keep the
  existing name codes. Never XTest, mouse-drag, `--coords`, or
  screenshot. The `select` reply is not proof.
- [x] `get-selection --window HANDLE --name PAT [--role ROLE]` reads
  independent AT-SPI `Text.GetNSelections` + `GetSelection(0)`
  (`agt_a11y_node_get_selection`) for that unique showing named node.
  The `select` reply payload does not count. Missing Text typed-fails
  (`a11y_selection_unavailable`). `n == 0` is empty success. Never
  screenshot / XTest / `--coords`. Live Chrome fixture field
  `SelectField` (`fixtures/cu/36-chrome-select.html`). Reasonix
  composer (`Message Reasonix…` under
  `scripts/reasonix-desktop-a11y.sh`) uses the same native
  `Text.SetSelection` / `GetNSelections` / `GetSelection` path — no
  eval-helper select glue (unlike `ScrollTo` / EditableText). Linux
  `agenterm-con` composer `Command` publishes those same Text methods
  (persistent publisher range; same ABI 1.8 verbs, no protocol change).
- [x] `set-caret --window HANDLE --name PAT --offset N [--role ROLE]`
  is one-shot AT-SPI `Text.SetCaretOffset`
  (`agt_a11y_node_set_caret_offset`) on the unique showing named node
  and reports `addressing=accessibility-tree` / `via=set-caret-offset`.
  `--name` is required. Missing Text / `UnknownMethod` typed-fails
  (`a11y_caret_unavailable`). SetCaretOffset false is
  `a11y_caret_no_effect`, not `timeout`. Miss / ambiguous keep the
  existing name codes. Never XTest, `--coords`, or screenshot. The
  `set-caret` reply is not proof. Live Chrome fixture field
  `CaretField` (`fixtures/cu/310-chrome-caret.html`) uses the same
  native `Text.SetCaretOffset` path — no protocol change (unlike
  `ScrollTo` / EditableText helpers). Reasonix composer
  (`Message Reasonix…` under `scripts/reasonix-desktop-a11y.sh`)
  uses that same native path — no eval-helper caret glue.
- [x] `get-caret --window HANDLE --name PAT [--role ROLE]` reads
  independent AT-SPI `Text.CaretOffset` / `GetCaretOffset`
  (`agt_a11y_node_get_caret_offset`) for that unique showing named
  node. The `set-caret` reply payload does not count. Missing Text
  typed-fails (`a11y_caret_unavailable`). Proof is independent
  `get-caret` after `set-caret --offset N` on Chrome `CaretField`
  (unfocused `CaretOffset` may be `-1`; after set it must equal `N`).
  Reasonix composer after `send-text HELLO` reports `CaretOffset=5`;
  after `set-caret --offset 2` independent `get-caret` is `2`.
  Linux `agenterm-con` composer `Command` publishes those same Text
  methods (persistent publisher caret; ABI 1.9 verbs).
- [x] `get-text --window HANDLE --name PAT [--role ROLE]` reads
  independent AT-SPI `Text.GetText` (`agt_a11y_node_get_text`) once
  for that unique showing named node — the same text authority
  `wait --text-equals` polls, without a timeout. `send-text` /
  `paste` / `copy` `matched.text`, `last_text_write_via`, the WebKit
  eval helper's queued-job `OK`, and tree snapshot `text` do not
  count. Missing Text typed-fails (`a11y_text_unavailable`). Never
  XTest / `--coords` / screenshot. Proof is independent `get-text`
  equal to what the field holds: con `Command` after `send-text
  HELLO`, Chrome fixture `GetTextField` (`fixtures/cu/311b-chrome-gettext.html`,
  prefilled), and Reasonix composer `Message Reasonix…` under
  `scripts/reasonix-desktop-a11y.sh` (WebKit 2.52 exposes `Text` on
  the composer `<textarea>` — no eval-helper get-text glue, unlike
  `ScrollTo` / EditableText helpers).
- [~] `get-text --window HANDLE` without `--name` reads the showing
  focused node (innermost `Text.GetText` candidate) and reports
  `via=gettext`. con and Reasonix already close that loop. Chrome
  `GetTextField` still needs the renderer on the same host AT-SPI bus
  the observer uses (`AT_SPI_BUS` / `AT_SPI_BUS_ADDRESS`, not a
  `GetAddress` `,guid=` owner). Do not mark this leaf shipped on
  worker JSON.
- [ ] screenshot, control tree and action results are causally identifiable
  against the same observation instant, so a caller can detect that the target
  changed underneath a plan.
- [ ] AgenTerm's own surfaces are first-class observation targets. Making
  AgenTerm a computer-use target with a real control tree is owned here jointly
  with the surfaces that publish `ui-snapshot`; this module owns the cu-side
  contract, not the publishing surfaces themselves.

## Determinism

- [~] every state transition a caller must observe is waitable with a bounded
  typed timeout. No documented workflow depends on a fixed sleep. Named
  `send-text` / `paste` / `copy` is waitable with `--text-equals` /
  `--node-text-equals` / `--text-contains` / `--node-text-contains`.
- [ ] a wait failure reports what was observed at the deadline, not only that
  the deadline passed.
- [ ] requests, responses, enumerations and transfers are size and time bounded.
  A pathological target cannot make the host allocate without limit or block
  indefinitely.

## Evidence

- [ ] the command set is proven by public black-box journeys against the real
  executable and a real target, waiting on state rather than sleeping, cleaning
  every process and file it owns.
- [ ] pure tests own command parsing, wire limits, identity/bounds
  normalization, degraded-mode selection and typed failure states.
- [ ] a layering test proves no outer layer links an inner-layer primitive
  directly, in the same spirit as the existing platform boundary gate.
