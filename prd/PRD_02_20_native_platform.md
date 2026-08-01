# Native platform abstraction

Parent: [AgenTerm product tree](../PRD.md#product-tree)

This module owns the native operating-system adaptation boundary used by the
AgenTerm desktop GUI. It does not own terminal, Fleet, Settings, or Hub product
behavior. Those product semantics remain in their existing PRD modules and
consume this layer through typed platform capabilities.

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Product outcome

- [~] the Platform Facade is being promoted from an internal module into the
  independently consumable `crates/agenterm-platform` workspace package. The
  first real dependency slices move process and PTY contracts, services,
  private target selection and all three native adapters without retaining
  duplicate source. Platform shell defaults also move under the process feature;
  shared DPI/geometry conversion is exposed by the window feature. The remaining
  capability families and product-code extraction are open. Generic host
  filesystem conventions and cross-process path/slot locks are public while
  AgenTerm directory names, audit filenames and Script concurrency policy remain
  root-package concerns. Typed IPC endpoints, optional serde support, Windows
  named pipes and Unix sockets are public together with typed transport failures,
  bounded I/O and endpoint ownership checks. Only AgenTerm endpoint/workspace
  naming remains in the root package, and IPC capability status is available.
- [~] Clipboard, bounded XRGB screenshot encoding and font-file candidates are
  reusable crate capabilities. Terminal paste limits, HWND/GDI capture and font
  handles remain explicit root product extensions rather than leaking native
  types through the public crate API.
- [~] Normalized modifier/key classification, committed Unicode precedence,
  UTF-16 decoding, and per-platform primary-shortcut policy are reusable through
  the `input` feature. Native frontend event translation remains a root product
  extension. The `ime` feature now owns public preedit/commit actions and
  display-aware status: Linux/macOS are available with a display, while Windows
  remains explicitly Unsupported until native preedit adaptation ships.
- [~] Activation policy and typed native-window requests are public crate API.
  Windows show/no-activate/restore operations and Linux/macOS winit activation
  intent live in target-isolated crate adapters; AgenTerm owns only its window
  lifetime and maps typed failures into the product protocol.
- [x] Script worker process-tree ownership consumes the crate process facade:
  Windows Job Objects and Unix process groups have no duplicate root native
  implementation. AgenTerm-specific audit-path naming remains product policy.
- [x] Product path composition consumes `filesystem::host_directories` and
  `executable_name`; the three root OS path adapters are deleted. AgenTerm
  directory/file naming remains product policy without compile-time OS selection.
- [x] Script Runtime clipboard uses the public crate facade with its independent
  two-second robustness deadline; duplicate root native clipboard adapters are
  deleted and no authorization policy is introduced.
- [x] Script Runtime atomic filesystem mechanics and child-pipe observation use
  public filesystem/process facades. Product budgets and receipts remain in the
  unrestricted runtime; duplicate root native adapters are deleted.
- [x] Script Runtime child-window observation/input/control uses the public
  process-window contract and selected adapters. Windows is native; Linux/macOS
  return typed Unsupported without narrowing caller policy.
- [~] Passive system-WebView discovery is public and selected inside the crate,
  with Missing and Failed kept distinct. Native font discovery/metrics and an
  opaque RAII font resource are public; the Windows renderer consumes its RAII
  resource and all three duplicate root native font implementations are deleted.
- [ ] Windows, macOS, and Linux native frontends consume one declared platform
  contract for window lifecycle, normalized input, IME, DPI, clipboard, font
  discovery, screenshots, activation, and applicable system integration.
- [ ] Product-visible UI state, actions, labels, layout geometry, and semantic
  snapshots remain platform-neutral and are not reimplemented inside an OS
  adapter.
- [x] Toolbar hit-to-action mapping is one platform-neutral product table; the
  former three OS-named copies are deleted rather than exported as platform API.
- [x] Display backend facts and headless-aware window status are public crate
  API selected by Windows/Linux/macOS adapters.
- [ ] Necessary native differences remain explicit capabilities or typed
  unsupported results; parity never means hiding a missing behavior or forcing
  all systems through a lowest-common-denominator widget implementation.

## Target tree

```text
crates/agenterm-platform/
├─ Cargo.toml             default-light capability features
└─ src/
   ├─ lib.rs              stable public capability/status surface
   ├─ contract/           OS-neutral types, typed errors, and behavior contracts
   ├─ process.rs, ...     public capability facades
   ├─ selected.rs         private compile-time adapter selection only
   └─ adapters/
   ├─ windows/
│  ├─ window
│  ├─ input
│  ├─ ime
│  ├─ clipboard
│  ├─ font
│  └─ integration
   ├─ macos/
│  ├─ window
│  ├─ input
│  ├─ ime
│  ├─ clipboard
│  ├─ font
│  └─ integration
   └─ linux/
   ├─ window
   ├─ input
   ├─ ime
   ├─ clipboard
   ├─ font
   └─ integration
src/                      AgenTerm product extensions only
├─ Windows/Unix frontend, rendering and input orchestration
├─ Control Center projection shell
└─ endpoint discovery, workspace, Fleet, Script and UI policy
```

The tree describes ownership, not a requirement to create one source file per
leaf. macOS and Linux may reuse private Unix implementations without merging
their public capability identities. The first implementation should stay
small and split files only when a stable responsibility needs independent
tests or ownership.

The reusable crate must never depend on the root `agenterm` package. Public
signatures expose no `windows-sys`, `libc`, `rmux-pty`, winit, theme, Fleet,
Control Center or UI protocol types. Product-specific executable names,
environment variables, paths and legacy discovery stay in the root package or
enter the crate only as caller-supplied platform-neutral values.

### Platform Facade closure rule (revision 4, 2026-07-31)

- [x] revision-4 Platform Facade is the sole production native boundary

`platform` is not merely a collection of GUI helpers. It is the required
middle layer between product code and an operating system. Product/domain
modules call typed `platform::{ipc, process, paths, ui_host, webview, ...}`
facade services; they do **not** branch on Windows, macOS, Linux, Unix, native
handles, or OS path/identity conventions. `selected.rs` and
`adapters/{windows,macos,linux}` are the only production implementation sites
that may select an OS with `cfg`.

This preserves necessary native implementations without allowing OS conditionals
to spread through Fleet, IPC contracts, Script Runtime, Settings, Control
Center, or terminal product state. The facade owns common typed inputs,
outputs, capability facts, and errors; adapters own only OS mechanisms. A
future Web/TUI/agentic projection may consume the same facade contracts without
being forced through an OS widget API.

The migration is complete only when all of the following are true:

- [x] production `#[cfg(windows|unix|target_os|target_family)]` selection is
  confined to `src/platform/**` and unavoidable binary-entry bootstrap code;
  test-only fixtures are excluded from this count
- [x] `ipc_endpoint`, `ipc_transport`, process identity/tree control, standard
  paths, native activation/clipboard/screenshot, Control Center shell hosting,
  and WebView runtime probing call facade services rather than OS APIs directly
- [x] typed IPC endpoint selection and native listener/stream mechanics are
  owned by `contract::ipc`, `services::ipc`, and selected adapters; the legacy
  duplicate Unix-socket and Windows named-pipe implementations are deleted,
  while platform-neutral framing/server compatibility remains shared
- [~] top-level `win_app`, `unix_app`, and native wake modules are now
  adapter-owned implementation details while shared UI state remains platform
  neutral; physical ownership is complete, while further state normalization
  remains open
- [x] manual and repository-enforced source scanning finds no production
  OS-selection cfg or native API import outside `src/platform/**`; only
  necessary binary subsystem attrs and structurally excluded test fixtures
  remain. The gate scans every Rust source, allows only three exact subsystem
  attributes, and its fixture proves product markers are rejected while
  comments and test items are ignored.
- [x] a repository boundary test fails when a new non-platform production
  source file imports OS-native crates or contains an OS-selection `cfg`
- [x] `selected.rs` is the sole production adapter assembly point; the former
  top-level Windows/Linux/macOS module trees are adapter-private `native/`
  mechanisms, `platform/mod.rs` contains no production OS selection, and an
  internal gate rejects selection cfg and native API markers elsewhere in
  contracts or services
- [x] all three platform adapters satisfy the same facade contract tests; a
  missing capability remains a typed unsupported result, never an implicit
  fallback. One host test loads all three OS-neutral adapter manifests and
  validates revision 3, all eight capabilities, and typed Unsupported/Failed
  probes without compiling another platform's native APIs.

## Shared contract

- [ ] normalized input distinguishes physical key identity, logical key chord,
  committed text, IME preedit/commit, pointer movement/buttons, wheel motion,
  resize, focus, and scale-factor changes
- [x] text input is not reconstructed from physical key codes when the native
  window system provides committed Unicode text
- [x] shortcuts and text commits remain distinct so Shift characters, keyboard
  layouts, dead keys, CJK input, terminal control keys, and platform primary
  modifiers preserve their native meaning
- [x] platform services are capability-oriented rather than one global
  `OsLayer` object; an adapter exposes only the window, clipboard, font,
  screenshot, activation, or integration capability it implements
- [x] product actions use stable semantic identities such as `toggle-tabs`,
  `toggle-locale`, and `font-increase`; Win32 control IDs, winit events, HTML
  elements, and future TUI keys remain adapter details
- [x] rendering and `ui-snapshot` consume the same resolved labels, states, and
  geometry so semantic evidence cannot claim content different from the
  visible GUI

`agenterm-cli protocol-info` exposes the current adapter kind, contract
revision, and all eight typed capability statuses. Client-side and
`--running` server responses use the same schema, so automation can distinguish
Available, Unsupported, and Failed without reading source or assuming parity.

## Platform branches

- Windows
  - [x] contract-revision-1 Windows adapter owns toolbar action mapping,
    UTF-16 committed-text decoding, and Control/AltGr shortcut separation; the
    Win32 `WM_COMMAND`, `WM_CHAR`, and terminal-key hot paths consume it
  - [x] slice-2 owns bounded Unicode clipboard access and typed activation /
    show-without-activation behavior; terminal selection, paste, startup, and
    relayed focus hot paths consume the adapter
  - [x] slice-3 owns bounded GDI full-window and terminal-region PNG capture,
    with RAII resource cleanup, shared strict client-frame clip validation, and
    typed allocation/capture/encoding/`screenshot_invalid_clip` failures
  - [x] contract-revision-2 freezes shared scale-factor validation, logical /
    physical extent conversion, window metrics, geometry-event classification,
    and stable scale error codes; Windows adopts the revision without routing
    its native DPI messages through a Unix abstraction
  - [x] GDI terminal-font creation and metric measurement now execute through
    `platform::windows::font`; device-context, font-creation, and metric
    failures have stable typed diagnostics, while the application retains
    ownership of the live font handle and rendering lifecycle
  - [x] Windows capability reporting no longer claims full IME support:
    committed UTF-16 text is shipped, while unadapted IME preedit reports
    explicit `ime-preedit-not-yet-adapted`
  - [x] contract-revision-3 makes native minimize/maximize/restore snapshot
    state and bounded client resize consume the shared window lifecycle
    contract; Win32 retains HWND sizing and non-client-frame arithmetic
  - [x] the Windows-subsystem launcher and replaceable Win32 GUI projection now
    physically reside in the Windows adapter; `lib.rs` reaches them only through
    `services::frontend → selected`, and no longer selects or declares Windows
    application modules itself
  - [~] existing HWND controls, GDI rendering, system menu, and remaining input
    behavior are shipped inside the adapter but still need narrower normalized
    contracts around product-neutral state
  - [~] adapt the remaining behavior behind the shared contracts without
    changing the replaceable-GUI/server split or taking ownership of server
    state
  - [x] the first slice preserves native Edit behavior, ConPTY input,
    no-activate, parent-console launcher guidance, and running-image-safe
    development
- macOS
  - [x] contract-revision-1 toolbar hits now resolve through
    `platform::macos::toolbar` stable action IDs before shared product handlers;
    the visible order remains Toggle Tabs, New, then right-aligned Settings,
    locale, font decrease, and font increase
  - [x] the Unix frontend adapter's keyboard and Cocoa IME preedit/commit paths consume
    `platform::macos::input` and `platform::macos::ime`; Command remains the
    product modifier, terminal Control chords remain PTY input, and native
    committed text wins for Shift/Option/dead-key, Space, and CJK input
  - [x] physical resize and scale-factor events, logical client sizing, PTY
    resize, layout, and rendering now consume `platform::macos::scale`; the
    no-activate launch path also configures the macOS event loop before window
    creation
  - [x] contract-revision-2 removes the duplicated macOS conversion and
    geometry classifier; the macOS adapter aliases the shared types while
    retaining Cocoa/winit event extraction
  - [x] ordered macOS system-font candidates and availability probing now live
    in `platform::macos::font`; the shared Unix renderer consumes that adapter
    metadata and no longer owns Apple font paths
  - [x] contract-revision-3 shares window semantic state and strict client-size
    validation with Windows while the winit adapter retains Cocoa calls
  - [x] clipboard reads/writes now consume `platform::macos::clipboard` with a
    256 KiB byte budget, live bounded reads, supervised stdin writes, and typed
    failures; native macOS verifies the blocked-writer deadline and product
    Command-C/Command-V round-trip
  - [x] whole-window and pane PNG capture now consume
    `platform::macos::screenshot` with checked dimensions, clips, framebuffer
    length, a 64 MiB RGBA budget, and typed validation/I/O/encoding failures
  - [~] winit/softbuffer windowing, native IME events, system-font rasterization,
    Unicode/color/attribute rendering, cursor, and POSIX PTY interaction are in
    active delivery
  - [~] expose the remaining macOS behavior through the shared platform
    contracts while retaining Apple-native keyboard semantics and scale-factor
    accuracy
  - [ ] signed stable distribution installs a complete signed and notarized
    application bundle; an installer must not replace it with an unsigned
    locally assembled wrapper
- Linux
  - [x] contract-revision-1 toolbar and keyboard hot paths, plus IME and
    clipboard slice-2 bridges, consume `platform::linux` with explicit
    headless/backend failures
  - [x] Linux clipboard helpers: read/write probed separately, display-matched
    X11/Wayland selection, wall-clock timeouts, live stdout byte budget, typed
    `clipboard_timeout` / `clipboard_too_large` / `clipboard_unavailable`
  - [x] Linux screenshot encode and activation/no-activate (`with_active`)
    consume `platform::linux`; dimensions/pixels/path, strict clip bounds, and
    headless failures are typed; X11/Wayland softbuffer paths Available,
    headless Unsupported; native X11 `DISPLAY=:1` evidence recorded below
  - [x] contract-revision-2 removes the duplicated Linux conversion and
    geometry classifier; the Linux adapter retains X11/Wayland/headless
    capability discovery and consumes the shared typed contract
  - [x] contract-revision-3 shares window semantic state and strict client-size
    validation with Windows while the winit adapter retains X11/Wayland calls
  - [~] winit/softbuffer windowing, X11/Wayland input, system-font fallback,
    clipboard, cursor, scaling, and POSIX PTY interaction are in active delivery
  - [~] expose the remaining Linux behavior through the shared platform
    contracts with explicit X11/Wayland capability facts and headless failure
    diagnostics
  - [ ] packaged launch retains its declared dynamic-library adapter and does
    not require system-wide installation for bundled runtime dependencies

## Boundary and non-goals

- [x] the initial GUI-only boundary did not relocate process, filesystem,
  Script Runtime, Fleet authority, or IPC modules. That limit is superseded by
  revision 4: their OS-specific mechanisms now migrate behind the Platform
  Facade while their product semantics and public contracts stay unchanged.
- [x] the platform layer does not contain product labels, Settings policy,
  tab-tree behavior, Hub navigation, or terminal lifecycle decisions
- [x] `arch/` remains absent until CPU-specific code exists, such as explicit
  x86-64/AArch64 SIMD, assembly, ABI, or atomic implementations; operating
  systems never appear under `arch/`
- [x] no large flag-day rewrite: old paths remain until their replacement
  passes the same public behavior and rendered-evidence gates

## Parallel implementation rules

- [x] the primary agent owns `src/platform/mod.rs`, shared event/capability
  contracts, Windows adaptation, PRD alignment, and final integration
- [x] the macOS agent owns the macOS adapter and macOS-native evidence
- [x] the Linux agent owns the Linux adapter and Linux-native evidence
- [x] platform agents consume the shared contract and must request a contract
  change instead of independently editing its semantics
- [x] commits remain small and merge to `main` early; every adapter commit
  states the contract revision it implements and keeps unrelated product work
  out of the platform migration

## Migration and acceptance

1. [~] freeze normalized event and capability types with table-driven unit
   tests; contract revision 2 owns scale-factor validation, logical/physical
   conversion, window metrics, and geometry classification; revision 3 owns
   window semantic state and bounded client resize; OS adapters remain
   incremental
2. [x] adapt one narrow vertical slice—toolbar labels/actions plus keyboard
   text/shortcut separation—on all three systems
   (Linux hot-path wired through `platform::linux` @ `78f5333`; macOS toolbar,
   keyboard, and committed-text hot paths wired @ `4aadcb0`/`1e6b09c`;
   Windows hot paths and AltGr-safe UTF-16 input are wired through
   `platform::windows`)
3. [~] move IME, clipboard, DPI, font, screenshot, activation, and remaining
   window integration incrementally behind the same boundary
   (Linux IME+clipboard @ `66c54a5`/`b5d54ef`; clipboard helper harden @
   `bf17150`; DPI/scale @ `57958c1`; font discovery/metrics @ `25a45d2`;
   screenshot+activation @ `1b454c2`. Windows bounded clipboard,
   activation, and screenshot capture hot paths are adapted. macOS clipboard
   first cut @ `11ce9b8`, bounded screenshot @ `3811bda`, and Cocoa IME
   preedit/commit @ `91055b6` are adapted; Linux/macOS scale duplication is
   replaced by the contract-revision-2 shared implementation)
4. [~] remove superseded platform-specific paths only after native black-box
   and screenshot evidence passes on that platform

Completion requires:

- [x] formatting and warnings-denied Clippy on every target
- [ ] the same toolbar action identities, ordering, locale, and Settings
  default/terminal-override semantics on Windows, macOS, and Linux
- [ ] uppercase/Shift punctuation, Space, control keys, CJK IME, pointer,
  wheel, clipboard, DPI resize, focus, and no-activate behavior exercised
  through native public-interface tests
- [ ] structured snapshots and PNG evidence agree on visible labels, geometry,
  theme, active terminal, modal, and focus state
- [ ] platform failures are typed and diagnosable, with no hidden fallback that
  reports a capability as available when it did not execute

Revision-4 migration evidence (2026-08-01, implementation complete; final
integrated validation recorded below):

- [x] Final serial Windows-host validation passes repository lint, rustfmt,
  PRD alignment, warnings-denied all-target Clippy, 389 library tests, both
  static boundary scans, the three-adapter contract test, the seven-binary dev
  build, CLI help/protocol-info, native IPC smoke, and the full Control Center
  public smoke. The Control Center receipt includes a 760×480 PNG plus
  no-activate, reuse, recovery, exact typed close, and orphan-free cleanup.
- [x] The local Linux-target probe fails before AgenTerm source checking because
  this Windows host has no `x86_64-linux-gnu-gcc`; Linux/macOS native/cross
  compilation remains the existing six-target CI responsibility and is not
  misreported here as local evidence.

- [x] The dependency graph and explicitly bounded delivery leaves for this
  continuing migration live in `plan/plan-platform-facade-v4.md`; they keep
  Script Runtime process/window/clipboard/stream/file work separate from the
  Control Center and PTY/frontend hot paths. This plan grants no runtime
  authority and does not imply Candidate, tag, or public Release.
- [x] Script Runtime `std.process.list` and `std.process.kill`, together with
  owned child-tree cleanup, now call `platform::process` as their only native
  implementation owner. The product module preserves its existing typed Rhai
  receipts; its former Win32 snapshot/terminate, Linux `/proc`/`kill`, macOS
  `libproc`, Job Object, and Unix process-group copies have been deleted.
  `platform::process` is now a compatibility-only facade over
  `services::process → selected → adapters/{windows,linux,macos}`; process
  observation, inventory, termination, child-tree, and CLI autostart native
  mechanics have no remaining root-service implementation copy.
- [x] Script Runtime clipboard bindings now call
  `services::script_clipboard → selected → adapters`. Windows retains its
  two-second Unicode clipboard, UTF-16, and transferable-allocation contract;
  Linux/macOS retain the existing typed Unsupported result rather than silently
  inheriting GUI clipboard timeout or paste-policy semantics.
- [x] Script Runtime child-window facts, keyboard/pointer delivery, native
  messages, bounds, resize, and dialog-control operations now call
  `services::script_window → selected → adapters`. Win32 handles and messages
  no longer appear in `script_process`; Linux/macOS retain the public typed
  Unsupported receipts rather than a silent compatibility fallback.
- [x] Script Runtime atomic promotion, append parent-directory durability, and
  reparse-point metadata now call `services::script_files → selected →
  adapters`. Windows retains verbatim-path `MoveFileExW` promotion; Unix
  retains parent-directory `fsync`; product code keeps its unrestricted-path
  API and existing typed error receipts.
- [x] Script Runtime child stdout/stderr pump now obtains native pipe probe
  tokens and `PeekNamedPipe` availability only through
  `services::script_stream → selected → adapters`. The Windows exited-child
  drain behavior remains bounded; Linux/macOS deliberately supply no native
  probe token, so the portable blocking-reader path remains explicit.
- [x] Script worker supervision and audit serialization now call
  `services::supervisor_audit → selected → adapters`; their former product
  `platform/{windows,unix}` subtrees have been deleted. Adapter-owned Job
  Objects/process groups, global slot locks, and named audit locks retain the
  existing bounded cleanup and cross-process serialization semantics.
- [x] Host path and executable-name conventions now call
  `services::paths → selected → adapters/{windows,linux,macos}`. The root
  `platform::paths` module is compatibility-only; settings, workspace,
  instance-registry, sidecar naming, and terminal font defaults retain their
  existing Windows, Linux, and macOS conventions without target selection in
  product callers. This is convention discovery, not a path permission policy.
- [x] Terminal default-shell discovery now calls
  `services::runtime → selected → adapters/{windows,linux,macos}`. The product
  runtime no longer selects COMSPEC versus SHELL; existing cmd.exe and
  `/bin/sh` fallback behavior is unchanged.
- [x] The Unix new-terminal dialog now obtains its primary shell id, label,
  and program from the same typed runtime facade. Its `zsh` versus `sh`
  selection no longer appears in frontend product code; Windows declares its
  equivalent `cmd` descriptor through the shared contract.
- [x] Unix frontend clipboard calls now traverse
  `services::ui_clipboard → selected → adapters/{linux,macos}` and map native
  availability, size, timeout, and backend diagnostics to one typed contract.
  The adapter-local clipboard helper remains a temporary string-result
  compatibility projection; native frontend ownership is complete.
- [x] Unix softbuffer XRGB screenshot encoding now traverses
  `services::ui_screenshot → selected → adapters/{linux,macos}`. Adapter
  validation and encoder errors retain explicit typed Unsupported/Failed
  results before the adapter-local frontend string projection; the renderer is
  now physically adapter-owned.
- [x] Unix frontend font-file candidate selection now traverses
  `services::ui_font → selected → adapters/{linux,macos}`; the shared glyph
  cache and rasterizer remain shared adapter rendering details, without target
  selection or duplicate candidate tables.
- [x] Linux display capability classification now calls the selected Linux
  scale adapter. The shared geometry contract no longer branches on a display
  target; headless remains typed Unsupported and a usable display Available.
- [x] The obsolete Linux/macOS root native compatibility trees are deleted.
  Unix frontend and Control Center product orchestration now call the public
  crate activation, input, IME, window geometry, and display APIs directly;
  platform facts no longer route back through a root selected native module.
  The Windows-hosted crate tests and Agenterm Clippy lane pass. A local Linux
  target probe is unavailable past the dependency build-script boundary because
  this host lacks `x86_64-linux-gnu-gcc`, so native Unix compilation remains an
  explicit outstanding evidence item rather than an inferred success.
- [x] Native Windows screenshot capture now belongs to the crate Windows
  adapter. The stable public surface accepts an opaque caller-owned window
  handle and neutral window/client capture area, enforces bounded allocation
  and strict clipping, returns typed failures, and keeps every GDI type private.
  Linux and macOS report native-window capture as Unsupported while retaining
  renderer-owned XRGB PNG encoding. AgenTerm remote GUI and Control Center use
  this facade; the duplicate root GDI implementation and clip contract are
  deleted. Control Center file replacement and activation also reuse their
  existing crate capabilities.
- [x] The final root Windows `native` compatibility module is deleted. The
  replaceable GUI consumes public crate activation and input APIs directly,
  preserving typed activation diagnostics, Control/AltGr arbitration, and the
  stateful UTF-16 decoder. Win32 GUI host/render ownership remains open and is
  not claimed complete by removing this projection layer.
- [x] Windows launcher wake delivery and parent-console diagnostics now use
  public crate activation/process facades. `PostMessageW`, standard-handle
  probing, attach-only parent-console behavior, and console cleanup remain in
  the Windows adapter; AgenTerm keeps only wake coalescing, GUI argument policy,
  and IPC handoff. The root frontend entry no longer imports Win32 APIs.
- [x] macOS primary-shortcut arbitration is Command/meta-only. Control remains
  available to terminal control-key encoding (including Ctrl-C) instead of
  being misclassified as a product shortcut; a target-neutral contract test
  protects this invariant even on non-macOS development hosts.
- [x] Windows Control Center native shell mechanics now live behind the public
  generic `NativeTextWindowHost` extension boundary. Window creation, message
  loop, timer, GDI text paint, focus, close, title and invalidation are crate
  adapter responsibilities; the main crate retains Control Center state and
  maps it through the neutral host trait. Linux/macOS currently return typed
  Unsupported from this runner until their pixel-surface host is connected.
- [x] Linux/macOS native text-window hosting is now connected through the same
  public trait. The crate shared Unix adapter owns winit event loops/windows,
  softbuffer surfaces, raw identity extraction, resize/present, focus, polling,
  and renderer-frame receipts. Root Control Center services use one product
  bridge on all three targets and no longer select OS shell implementations.
  A Windows-hosted Linux-target all-feature Clippy run passes with warnings
  denied; it also exposed and closed stale PTY, clipboard-timeout and IPC-test
  cross-target compile defects.
- [x] Native dependencies are isolated by Cargo capability feature. Win32
  module features are forwarded from process, filesystem, locking, IPC,
  window, clipboard, screenshot, and font rather than declared as one global
  union. Default and each individual capability compile independently; on
  Windows the minimal process/filesystem dependency trees contain only
  `windows-sys -> windows-link` and do not activate UI/GDI/clipboard modules.
- [x] Window minimized/maximized/restored semantic state and native-flag
  precedence are public platform-neutral window contracts. AgenTerm's 320x240
  CLI resize minimum and error wording remain product policy in the main crate.
- [x] The public input contract now includes a native-library-free normalized
  key event with logical/named keys, bounded physical identity, press/release,
  repeat, committed text and modifier snapshot. Shift+Tab is explicitly
  preserved as named Tab plus Shift. Product composer/tmux/PTY-byte policy
  remains in the main crate; adapters own winit/Win32 event conversion.
- [x] Linux/macOS selected input adapters implement winit-to-contract key
  normalization inside the crate. Logical/named/physical keys, element state,
  repeat and committed text cross the public extension boundary only as crate
  types. Linux-target warnings-denied Clippy compiles focused mapper test targets
  for Shift+Tab and stable letter/digit identity; native execution remains CI evidence.
- [x] Control Center state-directory protection, exclusive state-file creation,
  atomic replacement, existing-window focus, and direct native capture now
  call `services::control_center → selected → adapters/{windows,linux,macos}`.
  The root facade retains only the typed strategy projection; shell rendering,
  no-activate, focus, capture, and event-loop mechanics are adapter-owned.
- [x] Passive system-WebView runtime discovery now calls
  `services::webview → selected → adapters/{windows,linux,macos}`. The root
  facade and shared facts contain no target selection; WebView2, WebKitGTK,
  and WKWebView filesystem probes remain passive and preserve their existing
  Detected/Missing/Failed facts. AgenTerm continues to use its native renderer.

- [x] `settings`, instance PID/start-identity checks, terminal default-shell
  selection, Control Center atomic-file/focus/capture routing, and passive
  WebView runtime probing now call typed `platform::{paths,process,runtime,
  control_center,webview}` services. Script Runtime's owned child-tree call
  path now uses `platform::process`; it retains its existing typed receipts.
  Control Center sidecar executable naming and registry PID/start-identity
  matching also now consume `platform::{paths,process}`. Control Center's
  projection/registry/IPC receipts remain in the product layer behind a typed
  host, while selected Windows/Linux/macOS shell adapters own native event
  loops, rendering, no-activate behavior, focus, window identity, and frame
  capture. Windows public no-activate evidence opens the adapter-owned shell,
  captures a 760×480 native PNG, closes it, and observes `not_running` with no
  residual owner.
- [x] The approved PTY boundary exposes normalized spawn, size, exit, and
  session/reader/wait operations only. POSIX `openpty`/fork/session/exec/poll
  and Windows ConPTY/job mechanics reside below selected adapters while
  retaining concurrent reader/wait and terminate-to-EOF ordering.
- [x] POSIX PTY allocation, fork/session/exec, resize, polling, and child
  lifecycle code now physically resides in `adapters/linux/pty.rs`; macOS has
  an explicit adapter entry over that private POSIX mechanism. The Windows
  adapter wraps `rmux-pty` and converts the neutral terminal size and process
  identity at its boundary. `src/pty` is now an OS-neutral compatibility
  projection over `services::pty → selected → adapter`; reader/wait concurrency,
  exit-triggered pseudoconsole close, and force-terminate ordering remain in
  the existing runtime path. Spawn, resize, reader/wait handle cloning, wait,
  and force-terminate now return the shared typed `Unsupported`/`Failed`
  lifecycle contract with stable failure codes; byte reads/writes retain their
  standard I/O semantics.
- [x] native IPC identity, default endpoint/workspace derivation, listener /
  stream framing, named-pipe and Unix-socket mechanics, permissions, peer
  identity, and stale recovery now reside beneath `src/platform/`; the
  product-facing `ipc_transport` is an OS-neutral shim. Legacy TCP and the
  v1/v2 instance schema remain unchanged. The final adapter split and both
  repository-wide static closure gates pass.
- [x] `LogicalInstance`, `ServerScopeId`, typed `IpcEndpoint`, endpoint
  selector, and legacy migration contract now live in
  `platform::contract::ipc`; `src/ipc_endpoint.rs` is compatibility-only.
  The contract's 11 focused tests retain main/dev separation, opaque identity,
  selector priority, legacy TCP parsing, and serialization evidence.
- [x] Typed IPC transport errors (`UnsupportedEndpoint`, endpoint validation /
  collision, bounded connect / accept timeout, and I/O) now live in
  `platform::contract::ipc_transport`. TCP compatibility framing and all three
  native adapter implementations consume that one error contract; the root
  `ipc_transport` module is an OS-neutral compatibility projection.
- [x] IPC adapter implementations are now physically selected from
  `platform::adapters/{windows,linux,macos}/ipc.rs`; macOS reuses the private
  Unix mechanism while keeping its own adapter identity. `selected.rs` is the
  only IPC target selector. Native endpoint bind/connect/accept and stream
  I/O now traverse `services::ipc → selected → adapter`; the former native
  transport implementation copies are deleted.
- [x] Script Runtime HTTP TLS provider/root-store selection and
  platform-specific TLS-error classification now traverse
  `services::script_http → selected → adapters`; the Rhai HTTP surface keeps
  its existing bounded timeout, proxy, response, and typed-error behavior.
- [x] CLI server autostart now calls `platform::process`; Windows Job
  breakaway, executable discovery, and null-stdio child creation are adapter
  mechanics, while unsupported hosts preserve the former no-autostart retry
  behavior.
- [x] Script worker sidecar executable-name conventions now resolve through
  `platform::paths`; client discovery no longer branches on `.exe` naming.
- [x] Workspace persistence now obtains its default Windows/Unix and
  instance-scoped path from `platform::paths`; the workspace domain no longer
  branches on LOCALAPPDATA, XDG, or server-scope conventions.
- [x] Windows-hosted `cargo fmt`, warnings-denied Clippy, focused facade/unit
  tests, `agenterm-cli --help`, both static boundary gates, and the same-host
  three-adapter contract test pass for the integrated revision-4 tree. Final
  Quick/build/public-smoke receipts are recorded only after the closing serial
  validation run.

Windows slice-1 evidence (2026-07-30):

- [x] warnings-denied all-target/all-feature Clippy
- [x] 286 library tests, including native toolbar-ID mapping, toolbar ordering, Ctrl shortcut, AltGr
  committed text, BMP Unicode, surrogate pairs, and orphan-surrogate handling
- [x] incremental Windows artifact build
- [x] `remote-ui-smoke` native public-interface journey: 15 evidence IDs,
  including toolbar/tabs, keyboard navigation, locale, Settings, clipboard,
  scrollback, close/detach, server restart, and replaceable-GUI recovery

Windows slice-2 evidence (2026-07-30):

- [x] warnings-denied all-target/all-feature Clippy
- [x] 289 library tests, including typed clipboard/activation diagnostics and
  UTF-16 allocation bounds
- [x] incremental Windows artifact build
- [x] `remote-ui-smoke` proves terminal selection, bounded clipboard copy/paste,
  system-menu behavior, and replaceable-GUI recovery through native interfaces
- [x] `startup-smoke` proves `--no-activate` and a 535 ms first native window

Windows slice-3 evidence (2026-07-30):

- [x] warnings-denied all-target/all-feature Clippy
- [x] 296 library tests, including the shared toolbar-order contract, strict
  overflow-without-shrink clip validation, 8K pixel-budget boundaries,
  in-place BGRA conversion, and typed screenshot failures
- [x] incremental Windows artifact build
- [x] `remote-ui-smoke` captures and validates both full-window and
  terminal-region PNG files through public CLI commands, then completes all 15
  replaceable-GUI evidence journeys
- [x] exact-head native rerun after shared-contract adoption: warnings-denied
  all-target/all-feature Clippy, 296 library tests, a 537 ms no-activate first
  window, and the complete replaceable-GUI remote smoke all pass

macOS hot-path evidence (2026-07-30):

- [x] `cargo fmt --check` and all-target warnings-denied Clippy pass
- [x] 445 library tests pass; focused macOS adapter tests cover stable toolbar
  order/IDs, Command versus terminal Control, Shift punctuation,
  Option/dead-key composition, Space, CJK IME preedit/commit, clipboard byte
  and timeout failures, screenshot bounds/failures, invalid scale metrics, and
  Retina scale-factor changes
- [x] native Cocoa GUI smoke with `AGENTERM_NO_ACTIVATE=1` produces a structured
  960x600 logical snapshot and a 1920x1200 Retina PNG; toolbar labels/order,
  locale, focus, window state, and layout geometry agree; the Unix/macOS
  snapshot publishes bounds for all six rendered toolbar controls, matching
  the Windows snapshot shape
- [x] native Accessibility resize produces an 800x468 logical snapshot and a
  1600x936 Retina PNG with zero GUI stderr; the same no-activate probe preserves
  the previously frontmost application
- [x] native Command-C/Command-V round-trip copies and pastes the exact composer
  marker through the macOS adapter; public CLI capture produces a 1920x1200
  whole-window PNG and a 1560x928 pane PNG, both under the adapter budget
- [x] the macOS clipboard writer is supervised in source; the blocked-pipe
  regression test and native Command-C/Command-V smoke pass on macOS

Cross-platform integration review (2026-07-30):

- [x] Linux screenshot requests that extend outside the framebuffer return typed
  `screenshot_invalid_clip` and do not write a silently shrunk PNG. Native
  Linux verification (2026-07-30): `cargo fmt --check`; warnings-denied
  all-target/all-feature Clippy; **8** `platform::linux::screenshot` unit tests
  (including overflow-clip reject-without-write); X11 Available / Wayland
  Available / headless Unsupported capability statuses plus activation
  `--no-activate` / `AGENTERM_NO_ACTIVATE=1` on native `DISPLAY=:1`; public CLI
  whole-window PNG **950×594** and pane PNG **702×459**
- [x] macOS clipboard write timeout covers blocked stdin delivery as well as
  child exit; native macOS executes the blocked-writer deadline test in 0.06
  seconds for the focused four-test suite

Contract-revision-2 local evidence (2026-07-31):

- [x] Windows-hosted shared platform tests pass **24/24**, including shared
  unit/fractional scale conversion, zero-size resize handling, typed invalid
  metrics, and all three adapters declaring revision 2
- [x] Windows warnings-denied library/test Clippy and `git diff --check` pass
- [x] Windows GDI font adapter tests pass **15/15** for the Windows platform
  slice; an incremental six-executable build and public no-activate startup
  smoke pass with a **570 ms** first native window
- [x] public `agenterm-cli protocol-info` reports the current contract revision and all
  eight Windows capability statuses; IME preedit and undeclared shell
  integration are explicit Unsupported results rather than false availability

Contract-revision-3 local evidence (2026-07-31):

- [x] shared window tests prove native-state precedence and strict
  320×240-to-`i32::MAX` client-size bounds; Windows and Unix `window-resize`
  hot paths consume the same typed validation
- [x] Windows Quick Gate passes repository lint, formatting, PRD alignment,
  warnings-denied all-target Clippy, and **303** library tests
- [x] CI run
  [`30565063423`](https://github.com/mgttt/agenterm/actions/runs/30565063423)
  passes Windows, Linux, and macOS on x86-64 and ARM64. The Linux x86-64 job
  also passes the portable Quick Gate, cross-platform automation audit,
  manifest client build/probe, and public Script CLI test; Windows passes its
  public startup/CLI slice
- [x] contract revision 3 is release-qualified for v0.1.10. Broader normalized
  pointer/wheel/IME ownership and the remaining adapter migrations above stay
  explicitly partial instead of being relabeled as shipped
