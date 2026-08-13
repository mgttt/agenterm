# `agenterm-cu` targets and transports

Parent: [Computer-use foundation (`agenterm-cu`)](PRD_02_28_agenterm_cu.md)

This module owns the target family, transport selection, and the per-platform
backends that realize the abstract command set from
[29](PRD_02_29_cu_command_surface.md).

Legend: `[x]` shipped, `[~]` partial, `[ ]` planned.

## Target family

- [ ] `current`, `ssh`, `rdp` and `vnc` are tiers of one family sharing one
  command set. `current` is the **local degenerate tier** — transport is
  in-process — not a temporary prototype to be replaced later.
- [ ] `current` ships first. Doing so is the cheapest way to pin the interface,
  because adding a remote transport afterwards changes transport only, not the
  commands above it.
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
`agenterm-platform` (`a11y-tree` and related contracts); `cu` selects the stack
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
| Linux | **AT-SPI2** (`at-spi2-core` / `org.a11y.atspi.*`) | AT-SPI accessible hierarchy | AT-SPI `Action` / `Component` |

### Requirements by stack

**Shared (all hosts)**

- [ ] `agenterm-platform` exposes one typed `accessibility_tree` contract:
  flattened nodes with path id, role, name, states, exact bounds, and action
  names. `cu` maps this contract to its public JSON without host-specific
  fields leaking upward.
- [ ] when the a11y bus / API is missing (headless without a11y, no registry,
  denied permission), `tree` and node actuation return typed `Unsupported` or
  `Failed` — never coordinate guessing while reporting structured success.
- [ ] screenshot and coordinate pointer paths are explicit degraded modes with
  observable markers in the reply; they do not satisfy a caller that requested
  structured node identity.

**Linux — AT-SPI2 (`current` first evidence)**

- [~] `current` on Linux/X11 enumerates a control tree through AT-SPI2:
  `agenterm-platform` (`a11y-tree`) implements the host stack; `cu` consumes
  libagenterm milestone 6 (`agt_a11y_tree_snapshot` / `agt_a11y_node_perform`)
  rather than calling platform accessibility APIs directly. Nodes carry role,
  name, states, screen bounds, and action names; node ids are child-index paths
  from each application root (for example `/3/0/0/1/0`).
- [~] `click --node <path>` and `focus --node <path>` invoke AT-SPI `Action`
  / `Component::grab_focus` for the resolved node via `agt_a11y_node_perform`.
  A node-addressed click uses a named `click`/`press` when present, otherwise
  the AT-SPI default action (index 0) when the node exposes any actions —
  including Chrome controls whose `GetActions` names are empty. It does not
  require `--coords` / `--degraded`. Invalid paths return typed
  `a11y_node_not_found`.
- [~] `windows` / `screenshot` / coordinate-degraded input on `current` still
  use `agenterm-platform` until `agt_window_enumerate` / unified screenshot /
  `agt_input_inject` milestones ship; capability JSON documents the gap.
- [ ] AT-SPI unavailable at runtime (no session bus, registry absent) → typed
  `Unsupported` / `Failed`; no silent fallback to XTest coordinates.
- [ ] black-box evidence: `scripts/cu-linux-smoke.sh` against the real `cu`
  binary on a host with `DISPLAY` and `at-spi2-registryd`.

**Windows — native API + UIA**

- [ ] UIA-backed `tree` and structured `click` / `focus` on `current` through
  `agenterm-platform`. Not claimed in the Linux-first slice.
- [ ] Win32 window enumeration and input injection remain separate platform
  contracts consumed by `cu`; UIA is the structured control-tree authority.

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
- [~] Linux `current` / AT-SPI2: `scripts/cu-linux-smoke.sh` (real `cu`, X11
  `DISPLAY`, running `at-spi2-registryd`) proves `tree`, refused unauthorized
  actuation, audited degraded coordinate click, invalid node path failure, and
  structured AT-SPI click when a clickable node exists.
- [ ] a cross-tier conformance test proves the same abstract command produces
  equivalent observable results on every tier that declares support for it.
- [ ] capability declaration is tested against reality: a target that declares
  support and then fails the command is a defect, not a runtime condition.
