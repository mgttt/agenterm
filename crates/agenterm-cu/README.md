# agenterm-cu

`agenterm-cu` is AgenTerm's computer-use foundation: a target-agnostic command
surface for orchestrator agents to observe and actuate a desktop through
structured data instead of screenshot/OCR coordinate guessing.

## Intended agent loop

Orchestrator agents (not humans staring at pixels) should run:

```text
loop until goal:
  observe structured state (windows, control tree, typed capabilities)
  act by structured identity (window + node path id) when a tree exists
  wait on observable conditions with bounded timeouts — never sleep
```

`cu` is capability, not judgment: no planner, model, or agent loop ships here.

Named window placement (`window-place`, Spectacle **full** action catalog) is
accepted under [`prd/PRD_02_32_cu_window_placement.md`](../../prd/PRD_02_32_cu_window_placement.md).
v0.1.19 implements that tree concurrently (geometry + apply + grants +
history), not a half-screen demo. The verb is not in the command enum yet.

## Native accessibility mapping (按图索骥)

| Concern | Windows | Linux (`current` slice) | macOS (planned) |
|---------|---------|------------------------|-----------------|
| Window list | Win32 `EnumWindows` | X11 `_NET_CLIENT_LIST` | `AXUIElement` application windows |
| Control tree | **UIA** (`IUIAutomation`) | **AT-SPI2** (`org.a11y.atspi.*` on D-Bus) | **AX** (`NSAccessibility`) |
| Node identity | automation id + runtime id + bounds | path id (`/0/2/5`) + role + name + bounds | AX path + role + title + bounds |
| Node click/focus | `InvokePattern` / `LegacyIAccessible` | AT-SPI `Action::do_action("click")` / `Component::grab_focus` | `AXPress` / `AXRaise` |
| Text entry | `ValuePattern` / `SendInput` | AT-SPI `EditableText` (future) / `input-inject` | AX value + events |
| Screenshot | GDI native capture | typed `unsupported` (no OCR substitute) | typed `unsupported` (planned) |

Linux `tree` and structured `click` / `focus` use **AT-SPI2 only**. If the
accessibility bus is unavailable (no session bus, headless without a11y), commands
return typed `unsupported` / `failed` — never a silent coordinate fallback.

Coordinate clicks remain available only with explicit `--degraded` and are
audited separately from AT-SPI actuation.

## Linux `current` slice

| Command | Backend |
|---------|---------|
| `windows` | X11 window enumeration (`agenterm-platform`) |
| `tree` | AT-SPI2 flattened control tree with role, name, states, bounds, actions |
| `click --node <path>` | AT-SPI2 `Action` (`click` / `press`) |
| `focus --node <path>` | AT-SPI2 `focus` action or `Component::grab_focus` |
| `click --coords X,Y --degraded` | XTest (explicit degraded mode only) |
| `send-text` / `send-keys` | XTest keyboard injection |
| `screenshot` | typed `unsupported` on Linux native capture |
| `wait` | polls window state |

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

Every command requires an explicit `--target`. Observation commands need the
`observe` grant; actuation commands need `actuate`. Grants come from `--grant` or
`AGENTERM_CU_GRANT` (comma-separated). Local `current` is not exempt.

Unauthorized actuation returns `refused`, distinct from `unsupported` and
mechanism failures. Authorized actuation is appended to a JSONL audit log
(`AGENTERM_CU_AUDIT_PATH`, default `~/.local/share/agenterm/cu-audit.jsonl`).
If the audit path cannot be written, actuation does not execute.

## Examples

```bash
# Declare capabilities (observe grant)
cu --target current --grant observe capabilities

# List top-level windows
cu --target current --grant observe windows

# AT-SPI control tree (all application roots)
cu --target current --grant observe tree

# Scoped tree for one X11 window handle
cu --target current --grant observe tree --window 0x3c00007

# Structured click by node path (AT-SPI)
cu --target current --grant actuate click --node /3/0/0/1/0

# Structured focus
cu --target current --grant actuate focus --node /3/0/0/1/0

# Wait for at least one window, 3s max
cu --target current --grant observe wait --timeout-ms 3000 --window-count-gte 1

# Refused without actuate grant
cu --target current --grant observe send-text hello

# Audited coordinate click (explicit degraded mode only)
cu --target current --grant actuate click --coords 100,200 --degraded

# JSON command envelope
cu exec --grant observe,actuate --json '{"verb":"windows","target":"current"}'
```

## Black-box evidence

From the repository root on a host with `DISPLAY` set (X11 or Xvfb) and a
running AT-SPI registry (`at-spi2-registryd`):

```bash
./scripts/cu-linux-smoke.sh
```

## Layering

```text
native primitive     agenterm-platform (AT-SPI2 / X11 / XTest — owned there)
    ↑
abstract command     agenterm-cu library (`Command`, typed `CuReply`)
    ↑
current transport    in-process `Executor` for target `current`
    ↑
shell command        `cu` binary
```

`cu` never opens raw OS APIs. Missing mechanisms are added to
`agenterm-platform` with typed `Available` / `Unsupported` / `Failed`.
