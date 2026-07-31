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
   └─ bounded evidence and failures
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
- [ ] loss or restart of the selected server produces explicit Offline,
  Recovering, Restarted, or Incompatible state. A new epoch requires a fresh
  baseline; stale projection data is never displayed as live continuity.

## Main-workspace entry

- [ ] the Human workspace exposes one stable `open-control-center` action in
  the terminal-owned top toolbar. Visible placement, responsive geometry,
  compact `CC` fallback, keyboard access, and snapshot geometry are owned by
  the [Human workspace](PRD_02_06_human_workspace.md).
- [ ] the toolbar, CLI, Script automation, and future platform surfaces invoke
  the same action identity and authority resolver rather than constructing
  process arguments or local endpoints independently.
- [ ] a missing, incompatible, or failed `agenterm-cc` reports a non-blocking
  typed result. It does not show a modal dialog, start another Fleet authority,
  or make the terminal workspace unavailable.

## Information architecture

### Cockpit

- [ ] Cockpit is the first useful read-only slice: it shows the selected server
  build and identity, logical instance, process health, event epoch/sequence,
  tab counts and states, and component availability from current typed facts.
- [ ] inspect and select shortcuts target stable IDs and return verifiable
  post-state. A missing server says that no Fleet authority is connected
  rather than presenting a fabricated zero-sized fleet.

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

The v0.1.11 spike reports passive `runtime_presence` separately from
`host_state`. A detected runtime never implies a working host:
`host_state=unimplemented` and `active_renderer=native` remain truthful on
all platforms. The renderer-neutral bridge v1 binds the exact packaged
origin, main frame, per-document nonce, request ID and deadline; enforces a
64 KiB message and eight-request concurrency bound; and exposes only
`host.ready`, `host.facts`, and read-only `fleet.snapshot`. It has no generic
eval, shell, process, network, navigation, listener, or download escape
hatch. Actual host loading, packaged-resource integrity, startup/reload
measurement, accessibility, and six-target runtime proof remain future
promotion gates.

## v0.1.11 delivery gate

- [ ] `agenterm-cc --help`, `--version`, and `capabilities --json` are
  side-effect free and truthful on all six platform/architecture release
  cells.
- [ ] the main-workspace entry launches or focuses the matching Control Center;
  explicit no-activate preserves the prior foreground application, and a
  missing or incompatible binary fails without disturbing the terminal.
- [ ] Control Center selects the same logical instance as its caller, consumes
  the shared endpoint resolver, and does not start a server or choose
  arbitrarily among multiple authorities.
- [ ] Cockpit presents a causally identified snapshot and terminal-independent
  component availability; Workflows, Extensions, and InfoHub have stable
  navigation and truthful empty/unavailable states.
- [ ] close, force-kill, renderer failure, server restart, server loss,
  incompatible protocol, repeated open, and GUI-with-server-retained journeys
  prove process reuse, bounded cleanup, recovery, and PTY/workspace isolation.
- [ ] structured snapshot/action evidence and PNG evidence agree on selected
  view, connection state, labels, availability, and geometry.
- [ ] any distributed executable has its own size budget, hash, SBOM,
  provenance, startup measurement, capability catalog, and public black-box
  owner; it does not inflate `agenterm.exe`.

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
