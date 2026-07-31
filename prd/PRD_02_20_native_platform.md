# Native platform abstraction

Parent: [AgenTerm product tree](../PRD.md#product-tree)

This module owns the native operating-system adaptation boundary used by the
AgenTerm desktop GUI. It does not own terminal, Fleet, Settings, or Hub product
behavior. Those product semantics remain in their existing PRD modules and
consume this layer through typed platform capabilities.

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Product outcome

- [ ] Windows, macOS, and Linux native frontends consume one declared platform
  contract for window lifecycle, normalized input, IME, DPI, clipboard, font
  discovery, screenshots, activation, and applicable system integration.
- [ ] Product-visible UI state, actions, labels, layout geometry, and semantic
  snapshots remain platform-neutral and are not reimplemented inside an OS
  adapter.
- [ ] Necessary native differences remain explicit capabilities or typed
  unsupported results; parity never means hiding a missing behavior or forcing
  all systems through a lowest-common-denominator widget implementation.

## Target tree

```text
src/platform/
├─ mod.rs                 the only product-facing Platform Facade
├─ contract/              OS-neutral types, typed errors, and capability ports
├─ services/              facade operations: ipc, process, paths, ui-host, webview
├─ selected.rs            private compile-time adapter selection only
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
```

The tree describes ownership, not a requirement to create one source file per
leaf. macOS and Linux may reuse private Unix implementations without merging
their public capability identities. The first implementation should stay
small and split files only when a stable responsibility needs independent
tests or ownership.

### Platform Facade closure rule (revision 4, 2026-07-31)

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

- [ ] production `#[cfg(windows|unix|target_os|target_family)]` selection is
  confined to `src/platform/**` and unavoidable binary-entry bootstrap code;
  test-only fixtures are excluded from this count
- [ ] `ipc_endpoint`, `ipc_transport`, process identity/tree control, standard
  paths, native activation/clipboard/screenshot, Control Center shell hosting,
  and WebView runtime probing call facade services rather than OS APIs directly
- [ ] top-level `win_app`, `unix_app`, and other native frontend modules become
  adapter-owned implementation details while shared UI state remains platform
  neutral
- [ ] a repository boundary test fails when a new non-platform production
  source file imports OS-native crates or contains an OS-selection `cfg`
- [ ] all three platform adapters satisfy the same facade contract tests; a
  missing capability remains a typed unsupported result, never an implicit
  fallback

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
  - [~] existing Win32 window, HWND controls, GDI rendering, native clipboard,
    system menu, and remaining input behavior are shipped but remain
    distributed across Windows application modules
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
  - [x] `unix_app` keyboard and Cocoa IME preedit/commit paths now consume
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

- [~] the initial GUI-only boundary did not relocate process, filesystem,
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

Revision-4 migration evidence (2026-07-31, partial):

- [~] The dependency graph and explicitly bounded delivery leaves for this
  continuing migration live in `plan/plan-platform-facade-v4.md`; they keep
  Script Runtime process/window/clipboard/stream/file work separate from the
  Control Center and PTY/frontend hot paths. This plan grants no runtime
  authority and does not imply Candidate, tag, or public Release.
- [x] Script Runtime `std.process.list` and `std.process.kill`, together with
  owned child-tree cleanup, now call `platform::process` as their only native
  implementation owner. The product module preserves its existing typed Rhai
  receipts; its former Win32 snapshot/terminate, Linux `/proc`/`kill`, macOS
  `libproc`, Job Object, and Unix process-group copies have been deleted.
- [x] Script Runtime clipboard bindings now call
  `services::script_clipboard → selected → adapters`. Windows retains its
  two-second Unicode clipboard, UTF-16, and transferable-allocation contract;
  Linux/macOS retain the existing typed Unsupported result rather than silently
  inheriting GUI clipboard timeout or paste-policy semantics.
- [x] Script Runtime atomic promotion, append parent-directory durability, and
  reparse-point metadata now call `services::script_files → selected →
  adapters`. Windows retains verbatim-path `MoveFileExW` promotion; Unix
  retains parent-directory `fsync`; product code keeps its unrestricted-path
  API and existing typed error receipts.

- [~] `settings`, instance PID/start-identity checks, terminal default-shell
  selection, Control Center atomic-file/focus/capture routing, and passive
  WebView runtime probing now call typed `platform::{paths,process,runtime,
  control_center,webview}` services. Script Runtime's owned child-tree call
  path now uses `platform::process`; it retains its existing typed receipts.
- [~] native IPC identity, default endpoint/workspace derivation, listener /
  stream framing, named-pipe and Unix-socket mechanics, permissions, peer
  identity, and stale recovery now reside beneath `src/platform/`; the
  product-facing `ipc_transport` is an OS-neutral shim. Legacy TCP and the
  v1/v2 instance schema remain unchanged. The final adapter split and the
  repository-wide static closure gate remain deliberately unmarked.
- [~] `LogicalInstance`, `ServerScopeId`, typed `IpcEndpoint`, endpoint
  selector, and legacy migration contract now live in
  `platform::contract::ipc`; `src/ipc_endpoint.rs` is compatibility-only.
  The contract's 11 focused tests retain main/dev separation, opaque identity,
  selector priority, legacy TCP parsing, and serialization evidence.
- [~] IPC adapter implementations are now physically selected from
  `platform::adapters/{windows,linux,macos}/ipc.rs`; macOS reuses the private
  Unix mechanism while keeping its own adapter identity. `selected.rs` is the
  only IPC target selector. Native endpoint bind/connect/accept and stream
  I/O now traverse `services::ipc → selected → adapter`; the former native
  transport implementation remains platform-private deletion debt, not a
  product execution path.
- [~] Script Runtime HTTP TLS provider/root-store selection and
  platform-specific TLS-error classification now traverse
  `services::script_http → selected → adapters`; the Rhai HTTP surface keeps
  its existing bounded timeout, proxy, response, and typed-error behavior.
- [~] CLI server autostart now calls `platform::process`; Windows Job
  breakaway, executable discovery, and null-stdio child creation are adapter
  mechanics, while unsupported hosts preserve the former no-autostart retry
  behavior.
- [~] Script worker sidecar executable-name conventions now resolve through
  `platform::paths`; client discovery no longer branches on `.exe` naming.
- [~] Workspace persistence now obtains its default Windows/Unix and
  instance-scoped path from `platform::paths`; the workspace domain no longer
  branches on LOCALAPPDATA, XDG, or server-scope conventions.
- [x] Windows-hosted `cargo fmt`, `cargo clippy --lib -- -D warnings`, focused
  process-facade and Script Runtime tests, plus `agenterm-cli --help` pass for
  this partial slice. Full boundary scanning and cross-platform adapter
  contract evidence remain required before completion.

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
