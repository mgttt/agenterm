# Control Center (`agenterm-cc`)

Parent: [AgenTerm product tree](../PRD.md#product-tree)

This module owns the independent Control Center product surface, its process
lifecycle, information architecture, renderer-neutral presentation model, and
its projections of capabilities owned elsewhere. It does not own Fleet truth,
workflow execution, package transactions, decentralized-network lifecycle, or
the main terminal workspace.

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Product outcome

- [ ] `agenterm-cc` provides one optional, independently replaceable secondary
  workspace for complex fleet views without expanding or destabilizing the
  daily terminal GUI.
- [ ] opening, closing, crashing, upgrading, or losing the renderer of Control
  Center cannot terminate a PTY, mutate a workspace implicitly, block terminal
  rendering, or become a second Fleet authority.
- [ ] every visible fact and action is sourced from a versioned public contract
  with truthful availability, causal identity, and typed failure; an empty
  navigation shell never implies that the underlying capability has shipped.

## Product tree

```text
AgenTerm Control Center
├─ process shell
│  ├─ launch / focus / no-activate
│  ├─ authority selection and reconnect
│  ├─ navigation, focus and local drafts
│  └─ native renderer / future system-WebView renderer
├─ Cockpit
│  ├─ server identity, epoch and sequence
│  ├─ Fleet and tab summaries
│  └─ typed inspect / select entry points
├─ Workflows
│  ├─ Definitions
│  ├─ Runs
│  ├─ Designer
│  └─ Evidence
├─ Extensions
│  ├─ PluginHub
│  ├─ AppHub
│  ├─ Installed / Updates
│  └─ Sources
├─ InfoHub
│  ├─ sources and subscriptions
│  ├─ normalized items and provenance
│  └─ explicit routes to notifications, drafts or workflow inputs
└─ diagnostics
   ├─ component availability
   ├─ connection / renderer state
   ├─ bounded evidence and failures
   └─ exact-owner native PNG capture
```

`Fleet Hub` is a historical planning name for part of this product. It does
not remain as a separate overlay or competing information architecture.
Control Center is the product name; the executable family uses
`agenterm-cc.exe` on Windows and `agenterm-cc` on Unix.

## Process and authority boundary

- [ ] Control Center owns only its window, navigation, focus, renderer
  lifecycle, bounded caches, uncommitted UI drafts, diagnostics, and
  projection state.
- [ ] `agenterm-server` remains the sole authority for workspace, tab tree,
  PTYs, Composer state, event epoch/sequence, receipts, and stable Fleet IDs.
  Control Center consumes snapshots, journals, waits, and actions through the
  same typed public control plane as other clients.
- [ ] workflow definitions, durable runs, retries, cancellation, compensation,
  and recovery remain owned by the orchestration module. Control Center may
  edit or project them but cannot silently substitute a local UI task list for
  a durable workflow authority.
- [ ] package inventory, signature verification, install, update, repair,
  rollback, and elevation remain owned by `agenterm-softmgr`. Control Center
  may browse catalogs and request an explicit transaction; it never performs a
  hidden install.
- [ ] peer identity, transport, content blocks, pins, caches, and decentralized
  network lifecycle remain owned by the
  [decentralized-network module](PRD_02_22_decentralized_network.md). InfoHub
  consumes an advertised source/provider contract rather than embedding a
  node.
- [ ] the unrestricted Script Runtime may supply task catalogs and automation
  primitives, but Control Center does not introduce Script permissions or
  become an Agent approval/credential authority.
- [ ] by default there is at most one compatible interactive Control Center
  process per user configuration domain. It may observe multiple discovered
  servers, while an open request from a terminal focuses the existing window
  and selects that caller's logical instance/server context. `--no-activate`
  ensures existence and context selection without stealing foreground focus.
- [x] loss or restart of the selected server produces explicit Offline,
  Recovering, Restarted, or Incompatible state. A new epoch requires a fresh
  baseline; stale projection data is never displayed as live continuity. The
  v0.1.11 public smoke removes the selected authority, observes explicit
  `server_unreachable` without terminating Control Center, starts a replacement
  at the same endpoint, and requires the same projection PID to adopt the new
  authority identity and epoch before showing connected facts.
- [~] the native projection now owns server reads in one bounded background
  worker: `ui-bootstrap`/protocol refresh and typed `read-events` probes never
  run on the renderer loop. A one-request/one-latest-update mailbox, context
  generations, Condvar deadlines and a 50 ms to 1 s bounded backoff reject
  late state and avoid fixed sleep polling. Events, journal gaps, restart and
  offline results refresh the projection; worker failure becomes typed
  `projection_worker_unavailable` without affecting the server or PTYs. The
  focused unit contract and the public no-activate causal journey are covered:
  a real tab mutation plus server loss/new epoch refresh the same CC PID without
  an `open` request or `.context.json`/`.focus` change, while preserving PTY and
  orphan-free cleanup. Broader three-platform renderer evidence remains open.

## Main-workspace entry

- [ ] the Human workspace exposes one stable `open-control-center` action in
  the terminal-owned top toolbar. Visible placement, responsive geometry,
  compact `CC` fallback, keyboard access, and snapshot geometry are owned by
  the [Human workspace](PRD_02_06_human_workspace.md).
- [ ] the toolbar, CLI, Script automation, and future platform surfaces invoke
  the same action identity and authority resolver rather than constructing
  process arguments or local endpoints independently.
- [~] a missing, incompatible, or failed `agenterm-cc` reports a non-blocking
  typed result. It does not show a modal dialog, start another Fleet authority,
  or make the terminal workspace unavailable. Missing-binary launch is
  black-box covered by an isolated copy of `agenterm-cli` with no sibling
  Control Center: the request fails boundedly with
  `control_center_unavailable`, creates no registry, and starts no server.
  A live incompatible registry now fails closed with
  `control_center_registry_incompatible_live`; an unparseable registry fails
  closed with `control_center_registry_unparseable`. `open`, `status`, and
  `close` preserve those records rather than deleting an owner that cannot be
  verified and accidentally launching a second process. Identity-mismatched
  stale compatible/incompatible records remain recoverable. Frozen older
  binary protocol coverage remains open.

## Information architecture

### Cockpit

- [~] Cockpit is the first useful read-only slice: it shows the selected server
  build and identity, logical instance, process health, event epoch/sequence,
  explicit total/running/dead tab counts, active stable ID/title, and component
  availability from current typed facts. Build commit/profile/cleanliness is
  visible without exposing a second build authority. The renderer-neutral snapshot and
  native shell consume the same facts; richer renderer navigation remains open.
- [x] public `agenterm-cc inspect --tab @ID` and `select --tab @ID` entry
  points accept only canonical stable IDs. Inspect preserves selection; select
  delegates to the server-owned `select-window` control operation, requires a
  matching control receipt, and re-reads the same PID/epoch before returning
  typed inspected-tab facts with explicit verified post-state. Missing targets
  fail without changing selection or creating Control Center GUI authority.
  The Windows public smoke proves inspection, selection, receipt identity,
  independent post-state, missing-target failure, and absence of GUI registry
  ownership against a headless server.
- [~] Native Cockpit pointer/keyboard interaction now has a stable platform-
  neutral input contract, Win32 and winit normalization, adapter-owned text-line
  hit testing, a three-row tab cursor, arrows/Home/End, and Enter/primary-click
  selection through the existing typed server navigation operation. Navigation
  runs off the native message loop and busy/failure states render explicitly.
  A single last-input-wins queue now retains one navigation arriving while the
  previous typed select is still being verified, so a pointer click is not lost
  in the interval between server active-tab progress and local worker receipt.
  The Windows public smoke derives pointer coordinates from the actual bounded
  three-row viewport and proves keyboard then pointer selection against one
  stable CC/server/epoch. Ordinary CI run `30716123255` proved macOS ARM64
  keyboard selection but timed out on the following pointer selection because
  the journey mixed independent renderer and WindowServer scale sources.
  Commit `7a52d87` derives the point-to-pixel conversion from the observed
  WindowServer frame width and exact renderer framebuffer width, while retaining
  bounded geometry and no-coordinate-retry requirements. Final Linux/macOS
  positive journey evidence remains open. Windows qualification evidence and
  Linux/macOS matching-host evidence are registered in separate gate manifests
  but participate in one exact PRD alignment parity check; cross-target compile
  evidence is never counted as a host-native runtime receipt. A missing server
  still says that no Fleet authority is connected instead of fabricating a
  zero-sized fleet.

### Workflows

- [ ] a Workflow is a versioned graph definition; a Run is one concrete,
  inspectable execution; a Pipeline is a linear or dataflow projection of that
  definition or run, not a second execution model.
- [ ] navigation separates Definitions, Runs, Designer, and Evidence. Until
  their owning orchestration contracts ship, each view exposes a truthful
  planned/unavailable state instead of promoting Rhai tasks to durable flows.

### Extensions

- [ ] PluginHub and AppHub share catalog search, source identities, inventory,
  compatibility, update visibility, and the future `agenterm-softmgr`
  transaction boundary.
- [ ] PluginHub presents components that extend AgenTerm APIs, providers,
  workflows, renderers, runtimes, tools, and sidecars.
- [ ] AppHub presents independently launchable or composed user applications
  built from components, scripts, resources, workflows, and UI.
- [ ] the two views remain distinct product classes; they do not create
  competing manifest formats, installers, or an ambiguous all-items market.
  Remote public markets, commercial transactions, and silent updates are
  later gates.

### InfoHub

- [ ] InfoHub owns the user-facing projection of `source -> item -> provenance
  -> route`: external information can become a card, notification, Composer
  draft, or explicit workflow input.
- [ ] no incoming item automatically executes a destructive Fleet action,
  transaction, or external commitment. A decentralized source is one future
  backend and does not transfer node ownership into InfoHub.
- [ ] offline or stale catalog data includes source identity and observation
  time. Missing provenance, source failure, or network unavailability is
  visible rather than silently replaced by cached data presented as current.

## Renderer-neutral host contract

- [x] Control Center separates product models and semantic actions from the
  renderer. The first reliable shell may remain native; adopting a system
  WebView is an evidence-based renderer choice, not a product dependency.
- [~] a future WebView host uses local, versioned, integrity-identified packaged
  resources and a bounded, versioned typed message bridge. Network-loaded pages
  never receive a privileged bridge.
- [~] Windows WebView2, macOS WKWebView, and Linux WebKitGTK availability,
  packaging, startup cost, DPI, locale, accessibility, clipboard, screenshots,
  activation, crash, and reload behavior are reported through platform
  capabilities.
- [x] missing or failed WebView support yields an explicit unavailable/fallback
  state and cannot block the native terminal GUI. Renderer restart reconstructs
  state from a fresh public projection and never claims process or event
  continuity.
- [x] using a WebView does not move product authority into JavaScript, load
  Control Center code directly from the network, or justify rewriting the
  terminal renderer, Tabs, Composer, or Settings.
- [~] `agenterm-cc screenshot --output PATH [--json]` captures the actual
  native window owned by the live, PID/start-identity-matched Control Center
  registry without focusing or activating it. Windows reuses the bounded
  platform GDI/PNG encoder and returns dimensions, byte length and SHA-256;
  owner replacement during capture fails closed and removes the ambiguous
  output. macOS now serves the exact last-presented softbuffer frame through
  an owner-PID/start-identity-bound request/result channel; its structured
  renderer snapshot carries selected view, server state/reason/context,
  physical dimensions, scale factor, and title beside the PNG digest. Linux
  now selects the same renderer-owned request path instead of reporting a
  manufactured platform-level unavailability; zero/missing native handles
  remain typed failures. A real X11/Wayland journey and retained PNG are still
  required before Linux screenshot support can be marked shipped.

The v0.1.11 spike reports passive `runtime_presence` separately from
`host_state`. A detected runtime never implies a working host:
`host_state=unimplemented` and `active_renderer=native` remain truthful on
all platforms. The native client smoke on each executable host queries
`agenterm-cc capabilities --json` without opening a window and requires the
platform backend (`webview2`, `wkwebview`, or `webkitgtk`), the stable bridge
version, the native active renderer, and the unimplemented host state
independently of whether runtime presence is detected, missing, or failed.
The renderer-neutral bridge v1 binds the exact packaged
origin, main frame, per-document nonce, request ID and deadline; enforces a
64 KiB message and eight-request concurrency bound; and exposes only
`host.ready`, `host.facts`, and read-only `fleet.snapshot`. It has no generic
eval, shell, process, network, navigation, listener, or download escape
hatch. Actual host loading, packaged-resource integrity, startup/reload
measurement, accessibility, and renderer-owned Unix window evidence remain
future promotion gates.

## v0.1.12 system-WebView host spike

- [~] an isolated `research/agenterm-webview` experiment now has local packaged,
  read-only Cockpit page. It is a reusable host experiment for future
  independent applications (optional Control Center views, PluginHub,
  InfoHub, Workflow) and does not replace the existing native `agenterm-cc`.
  `agenterm.exe` and `agenterm-server` never acquire a WebView dependency. The
  direct-WRY host and a separately locked minimal Tauri v2 reference load only
  packaged read-only assets and leave the stable renderer native. A standalone
  bridge-v1 core now enforces the exact packaged origin, top frame, fresh
  document nonce, portable non-replayed request ID, deadline, 64 KiB message,
  eight-in-flight and bounded per-document memory contracts. The native WRY
  adapter remains absent, so public receipts still truthfully report
  `bridge=absent`.
- [~] compare a minimal Tauri v2 host with a direct-WRY host before selecting
  a production implementation. The comparison records exact dependency and
  licence inventory, Rust/JS toolchain impact, binary/archive size, required
  system runtime, cold/warm startup, first paint and RSS. The first Windows
  receipt proves system WebView2 availability, no-activate page-load smoke,
  no residual host process, and a substantial sealed size difference
  (direct-WRY 520,704 bytes versus Tauri 8,763,392 bytes); its 604-second
  outer measurement deadline made build timing unavailable, and license,
  first-paint/RSS and three-platform evidence remain open. The decision is
  therefore `defer`, not adoption.
- [~] the fallback launcher and direct-WRY host now share typed
  `failure { code, stage, detail }` receipts for missing/invalid host,
  runtime probe, native window and WebView creation failures; black-box tests
  prove missing and invalid siblings exit 69 while retaining the native
  renderer. A real Windows machine with WebView2 deliberately unavailable is
  still required, as are macOS WKWebView reload/crash fallback and Linux
  WebKitGTK unavailable/PNG evidence; no fixed browser runtime is bundled.
- [~] the experiment loads only integrity-identified local assets. Its tested
  bridge-v1 core admits only `host.ready`, `host.facts`, and `fleet.snapshot`
  with exact origin, top-frame, nonce, request-id, deadline, 64 KiB and
  eight-in-flight bounds; it has no generic eval, shell, process, arbitrary
  navigation, download, or network bridge. Exposure remains open until the
  WRY adapter and a real public Fleet projection provide typed responses.
- [ ] adoption requires independent package and runtime evidence on each
  native platform plus crash/reload/no-activate/fallback black boxes. Until
  then `active_renderer=native` is the truthful stable state.

## v0.1.11 delivery gate

- [~] `agenterm-cc --help`, `--version`, and `capabilities --json` are
  side-effect free and truthful on all six platform/architecture release
  cells. Native client probes validate the platform WebView backend,
  renderer/host separation, bridge version, and runtime-presence vocabulary;
  cross-built cells remain build/existence evidence rather than executable
  runtime evidence.
- [~] the main-workspace entry launches or focuses the matching Control Center;
  explicit no-activate preserves the prior foreground application, and a
  missing or incompatible binary fails without disturbing the terminal.
  Process reuse, no-activate, and missing-sibling failure are black-box proven;
  an incompatible sibling binary remains an open fault-injection case.
- [ ] Control Center selects the same logical instance as its caller, consumes
  the shared endpoint resolver, and does not start a server or choose
  arbitrarily among multiple authorities.
- [~] Cockpit presents a causally identified snapshot and terminal-independent
  component availability; its public snapshot now carries explicit
  total/running/dead tab counts while the native renderer displays the same
  logical instance, PID/version, build commit/profile/cleanliness,
  epoch/sequence, active stable tab ID/title and
  component states. Public stable-ID inspect/select entry points now preserve
  the server as sole Fleet authority and return causally verified post-state.
  Workflows, Extensions, and InfoHub retain truthful unavailable states; native
  renderer navigation remains open.
- [~] close, force-kill, renderer failure, server restart, server loss,
  incompatible protocol, repeated open, and GUI-with-server-retained journeys
  prove process reuse, bounded cleanup, recovery, and PTY/workspace isolation.
  The Windows public smoke now proves typed close, repeated-open PID reuse,
  force-kill/stale-owner replacement, missing-binary failure, live server loss,
  same-endpoint/new-epoch recovery in the existing Control Center PID, PTY
  availability after recovery, and exact orphan cleanup. It now also drives a
  malformed incompatible sibling through the public endpoint selector, proves
  typed `server_incompatible` in both offline and live-renderer projections,
  preserves the same renderer/server PID, epoch, PTY content and registry
  bytes, then restores the original endpoint without residue. The same owning
  journey now replaces only its isolated native-window handle with an invalid
  HWND, requires public screenshot capture to fail with typed
  `control_center_screenshot_capture_failed`, restores the handle, and proves
  the same Control Center PID, server PID/epoch and live PTY survive before a
  fresh renderer-owned PNG succeeds. It also runs the Human GUI and Control
  Center against one authority, detaches the GUI through the public
  close/keep-server actions, and proves the same Control Center, server epoch
  and PTY remain while the UI lease is released. These Windows renderer-failure
  and cross-process server-retention leaves are closed; equivalent native Unix
  composition remains part of the three-platform evidence gate.
- [~] structured snapshot/action evidence and PNG evidence agree on selected
  view, connection state, labels, availability, and geometry. The Windows
  Control Center smoke now pairs the connected Cockpit snapshot/window title
  with a nonempty, decoded native-window PNG, verifies its typed owner PID,
  dimensions, byte length and digest, and retains the successful image at
  `dist/evidence/control-center-live-cockpit.png`. Native macOS now retains
  equivalent renderer-owned structured evidence at the host's actual backing
  scale; a separately recorded 2x sample proves Retina rendering. Linux remains
  open.
- [ ] any distributed executable has its own size budget, hash, SBOM,
  provenance, startup measurement, capability catalog, and public black-box
  owner; it does not inflate `agenterm.exe`.

## v0.1.12 macOS convergence evidence

- [~] native macOS 26.5 arm64 public task
  `control-center-macos-smoke` passes in 5.01 seconds with isolated settings,
  workspace, instance registry, native runtime, logical `dev` authority, and
  typed cleanup. It proves the caller-selected Unix socket and exact server
  PID/epoch/context without starting a second server.
- [~] one native Control Center PID survives repeated open, explicit focus,
  no-activate, server kill, typed `server_unreachable`, malformed sibling
  `server_incompatible`, and same-scope replacement with a new PID/epoch.
  Typed close and forced renderer-process kill preserve the server and PTY;
  stale owner recovery creates one replacement projection and leaves no owned
  process, socket, registration, or request/result file.
- [~] renderer-owned structured evidence and its retained PNG agree at the
  current display's actual backing scale. A recorded 2x sample is 1520x960
  physical pixels for the 760x480 logical Cockpit; the portable CI gate also
  accepts a truthful 1x virtual display instead of treating CPU architecture
  as a Retina guarantee. Both paths bind the same owner PID, logical `dev`
  context, connected state, selected view, title/tab count, dimensions, byte
  count, and exact file SHA-256. Failure output retains these individual facts
  rather than collapsing them into an opaque composite assertion. The visible
  framebuffer displays the logical authority rather than an absolute socket
  path.
- [~] native Linux now has a public X11/Wayland-aware lifecycle journey wired
  to main CI: X11 requires caller-selected Unix IPC, compositor-observed
  no-activate/focus reuse, renderer-owned structured frame plus independently
  hashed PNG, server loss/new epoch, renderer kill/replacement, PTY isolation
  and orphan cleanup. Wayland runs the portable owner/reuse lifecycle without
  pretending compositor focus is observable; unavailable displays return typed
  `Unsupported` without fake evidence. The X11 process-window adapter now
  selects one unique viewable EWMH client by exact PID, rejects ambiguity, and
  posts checked key/pointer events only to that window. The owning journey's
  `control-center.linux-native-cockpit-input` evidence requires keyboard and
  pointer active-tab changes while the compositor foreground witness, Control
  Center PID, server PID/endpoint/epoch and PTY remain unchanged. Wayland emits
  no positive-input evidence. A new real Linux CI execution, Wayland native
  evidence, six-cell reruns and packaged evidence remain open.
- [~] the Linux journey creates its isolated runtime as a direct `0700`
  directory owned by the effective UID before binding the Unix endpoint; the
  smoke verifies both owner and mode instead of weakening the production IPC
  `UnsafeEndpoint` boundary. This closes the hosted-runner umask failure once a
  new matching-host CI receipt returns. The X11 focus witness is located by a
  per-run unique visible title and then verified by exact window name; it no
  longer assumes the Xaw `xmessage` client publishes `_NET_WM_PID`, while the
  subsequent compositor activation and foreground assertions remain intact.
  Run `30708799815` proved that discovery fix and then exposed the production
  gap behind it: winit documents initial `active=false` as unsupported on X11,
  so the newly mapped Control Center stole the independent witness focus. The
  Linux adapter now creates a no-activate window hidden, installs
  `_NET_WM_USER_TIME=0`, and only then maps it; inability to establish that
  native invariant is a typed startup failure, not a best-effort fallback. A
  matching-host receipt for this exact behavior remains required.
- [~] macOS now owns the matching Quartz exact-PID process-window adapter. It
  rejects zero/multiple layer-0 on-screen candidates, bounds Retina client
  coordinates, and uses `CGEventPostToPid` only after the non-interactive TCC
  preflight. `control-center.macos-native-input-contract` proves either real
  keyboard/pointer navigation without changing the foreground app or the exact
  `process_window_input_unsupported` / `permission_denied` safe failure with no
  state change. The latter is contract evidence only and is not a positive
  native-input claim; a TCC-authorized native receipt remains open. The
  positive branch derives its pointer target from the current bounded
  three-row Cockpit viewport and sends it immediately after the keyboard
  server-state transition, proving the Control Center's last-input-wins queue
  retains input while the previous local receipt is still completing. The
  journey now keeps both evidence PTYs alive and translates the renderer-owned
  framebuffer row through the observed Quartz frame/client inset before
  posting the event; it does not retry alternate coordinates or accept a dead
  target as positive input evidence. Run `30708799815` proved the keyboard and
  live-target branches but showed that a targeted background click may reach
  AppKit without a preceding winit `CursorMoved`, leaving the shell with no hit
  position. The macOS shell now reads the current `NSEvent.locationInWindow`
  during `MouseInput` and converts AppKit bottom-left logical coordinates to
  renderer top-left physical coordinates. `window_client_rect` is typed
  Unsupported on macOS rather than relabeling WindowServer outer bounds; the
  journey now derives one point-to-pixel scale from the observed outer-frame
  width and exact client framebuffer width instead of applying the independent
  winit scale twice. A new positive ARM64 receipt remains required.

## Explicit v0.1.11 non-goals

- [ ] no complete workflow designer, scheduler, autonomous agent fleet, or
  cross-machine run recovery
- [ ] no production package marketplace, payment, public transaction,
  unprompted install, or automatic update
- [ ] no automatic execution of InfoHub signals
- [ ] no requirement to implement Control Center in a WebView or to bundle a
  complete browser runtime
- [ ] no libp2p/IPFS node inside Control Center, `agenterm.exe`, or the stable
  server
- [ ] no second PTY, workspace, event-journal, workflow-run, install, Agent
  permission, or credential authority
