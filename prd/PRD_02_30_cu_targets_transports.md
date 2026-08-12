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
  input, process reference, clipboard, filesystem — and do not open raw OS APIs.
  A missing mechanism is added to the platform crate with typed
  `Available`/`Unsupported`/`Failed`, per
  [20 Native platform](PRD_02_20_native_platform.md).
- [ ] Windows, Linux and macOS each reach the same abstract command set through
  their own accessibility/input stacks. Product behavior does not move into the
  adapters: what a click *means* is one shared rule; how a click is delivered is
  per host.
- [ ] a platform whose control-tree access is unavailable or not yet wired
  reports the degraded mode from [29](PRD_02_29_cu_command_surface.md) rather
  than being excluded silently from the product claim.
- [ ] first-platform delivery is explicit and does not imply the others. A tier
  or platform is claimed only with its own evidence.

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
- [ ] a cross-tier conformance test proves the same abstract command produces
  equivalent observable results on every tier that declares support for it.
- [ ] capability declaration is tested against reality: a target that declares
  support and then fails the command is a defect, not a runtime condition.
