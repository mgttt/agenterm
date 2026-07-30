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
├─ mod.rs                 shared capability contracts and normalized events
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

## Shared contract

- [ ] normalized input distinguishes physical key identity, logical key chord,
  committed text, IME preedit/commit, pointer movement/buttons, wheel motion,
  resize, focus, and scale-factor changes
- [ ] text input is not reconstructed from physical key codes when the native
  window system provides committed Unicode text
- [ ] shortcuts and text commits remain distinct so Shift characters, keyboard
  layouts, dead keys, CJK input, terminal control keys, and platform primary
  modifiers preserve their native meaning
- [ ] platform services are capability-oriented rather than one global
  `OsLayer` object; an adapter exposes only the window, clipboard, font,
  screenshot, activation, or integration capability it implements
- [ ] product actions use stable semantic identities such as `toggle-tabs`,
  `toggle-locale`, and `font-increase`; Win32 control IDs, winit events, HTML
  elements, and future TUI keys remain adapter details
- [ ] rendering and `ui-snapshot` consume the same resolved labels, states, and
  geometry so semantic evidence cannot claim content different from the
  visible GUI

## Platform branches

- Windows
  - [x] contract-revision-1 Windows adapter owns toolbar action mapping,
    UTF-16 committed-text decoding, and Control/AltGr shortcut separation; the
    Win32 `WM_COMMAND`, `WM_CHAR`, and terminal-key hot paths consume it
  - [~] existing Win32 window, HWND controls, GDI rendering, native clipboard,
    system menu, activation, screenshots, and input behavior are shipped but
    remain distributed across Windows application modules
  - [~] adapt the remaining behavior behind the shared contracts without
    changing the replaceable-GUI/server split or taking ownership of server
    state
  - [x] the first slice preserves native Edit behavior, ConPTY input,
    no-activate, parent-console launcher guidance, and running-image-safe
    development
- macOS
  - [~] winit/softbuffer windowing, native IME events, system-font rasterization,
    Unicode/color/attribute rendering, clipboard, cursor, DPI, and POSIX PTY
    interaction are in active delivery
  - [ ] expose macOS behavior through the shared platform contracts while
    retaining Apple-native keyboard semantics and scale-factor accuracy
  - [ ] signed stable distribution installs a complete signed and notarized
    application bundle; an installer must not replace it with an unsigned
    locally assembled wrapper
- Linux
  - [x] contract-revision-1 toolbar and keyboard hot paths, plus IME and
    clipboard slice-2 bridges, consume `platform::linux` with explicit
    headless/backend failures
  - [~] winit/softbuffer windowing, X11/Wayland input, system-font fallback,
    clipboard, cursor, scaling, and POSIX PTY interaction are in active delivery
  - [~] expose the remaining Linux behavior through the shared platform
    contracts with explicit X11/Wayland capability facts and headless failure
    diagnostics
  - [ ] packaged launch retains its declared dynamic-library adapter and does
    not require system-wide installation for bundled runtime dependencies

## Boundary and non-goals

- [ ] the initial migration does not relocate or redesign PTY, process,
  filesystem, network, Script Runtime, Fleet authority, or IPC modules
- [ ] the platform layer does not contain product labels, Settings policy,
  tab-tree behavior, Hub navigation, or terminal lifecycle decisions
- [ ] `arch/` remains absent until CPU-specific code exists, such as explicit
  x86-64/AArch64 SIMD, assembly, ABI, or atomic implementations; operating
  systems never appear under `arch/`
- [ ] no large flag-day rewrite: old paths remain until their replacement
  passes the same public behavior and rendered-evidence gates

## Parallel implementation rules

- [ ] the primary agent owns `src/platform/mod.rs`, shared event/capability
  contracts, Windows adaptation, PRD alignment, and final integration
- [ ] the macOS agent owns the macOS adapter and macOS-native evidence
- [ ] the Linux agent owns the Linux adapter and Linux-native evidence
- [ ] platform agents consume the shared contract and must request a contract
  change instead of independently editing its semantics
- [ ] commits remain small and merge to `main` early; every adapter commit
  states the contract revision it implements and keeps unrelated product work
  out of the platform migration

## Migration and acceptance

1. [~] freeze normalized event and capability types with table-driven unit
   tests; no GUI behavior moves in this step
   (`src/platform/mod.rs` contract revision 1; OS adapters still incremental)
2. [~] adapt one narrow vertical slice—toolbar labels/actions plus keyboard
   text/shortcut separation—on all three systems
   (Linux hot-path wired through `platform::linux` @ `78f5333`; macOS adapter
   exists but its Unix GUI hot paths still need explicit adapter routing;
   Windows hot paths and AltGr-safe UTF-16 input are wired through
   `platform::windows`)
3. [~] move IME, clipboard, DPI, font, screenshot, activation, and remaining
   window integration incrementally behind the same boundary
   (Linux IME+clipboard first cut @ `66c54a5`/`b5d54ef`; DPI/scale next;
   font/screenshot/activation still deferred)
4. [ ] remove superseded platform-specific paths only after native black-box
   and screenshot evidence passes on that platform

Completion requires:

- [ ] formatting and warnings-denied Clippy on every target
- [ ] the same toolbar action identities, ordering, locale, and Settings
  default/terminal-override semantics on Windows, macOS, and Linux
- [ ] uppercase/Shift punctuation, Space, control keys, CJK IME, pointer,
  wheel, clipboard, DPI resize, focus, and no-activate behavior exercised
  through native public-interface tests
- [ ] structured snapshots and PNG evidence agree on visible labels, geometry,
  theme, active terminal, modal, and focus state
- [ ] platform failures are typed and diagnosable, with no hidden fallback that
  reports a capability as available when it did not execute

Windows slice-1 evidence (2026-07-30):

- [x] warnings-denied all-target/all-feature Clippy
- [x] 286 library tests, including native toolbar-ID mapping, toolbar ordering, Ctrl shortcut, AltGr
  committed text, BMP Unicode, surrogate pairs, and orphan-surrogate handling
- [x] incremental Windows artifact build
- [x] `remote-ui-smoke` native public-interface journey: 15 evidence IDs,
  including toolbar/tabs, keyboard navigation, locale, Settings, clipboard,
  scrollback, close/detach, server restart, and replaceable-GUI recovery
