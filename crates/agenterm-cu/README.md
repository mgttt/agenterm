# agenterm-cu

`agenterm-cu` is AgenTerm's computer-use foundation: a target-agnostic command
surface for orchestrator agents to observe and actuate a desktop through
structured data instead of screenshot/OCR coordinate guessing.

## Intended agent loop

Orchestrator agents (not humans staring at pixels) should run:

```text
loop until goal:
  observe structured state (windows, control tree, typed capabilities)
  act by structured identity (window + node id) when a tree exists
  wait on observable conditions with bounded timeouts — never sleep
```

`cu` is capability, not judgment: no planner, model, or agent loop ships here.

## Linux `current` slice

The `current` target is the in-process local tier of the future
`ssh`/`rdp`/`vnc` family. On Linux/X11 today:

| Command | Status |
|---------|--------|
| `windows` | Wired through `agenterm-platform` X11 enumeration |
| `tree` | Typed **degraded** — AT-SPI2 is not wired in platform yet |
| `screenshot` | Command exists; native window capture returns typed `unsupported` on Linux |
| `click` / `send-text` / `send-keys` | Wired through platform XTest input injection |
| `wait` | Polls window state until a condition is met or timeout |

Structured node clicks require a control tree. When the tree is unavailable,
`click --window … --node …` returns typed `unsupported`. Coordinate clicks are
only accepted with an explicit `--degraded` marker so success never hides pixel
guessing.

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

# Degraded control tree response
cu --target current --grant observe tree

# Wait for at least one window, 3s max
cu --target current --grant observe wait --timeout-ms 3000 --window-count-gte 1

# Refused without actuate grant
cu --target current --grant observe send-text hello

# Audited coordinate click (explicit degraded mode)
cu --target current --grant actuate click --coords 100,200 --degraded

# JSON command envelope
cu exec --grant observe,actuate --json '{"verb":"windows","target":"current"}'
```

## Black-box evidence

From the repository root on a host with `DISPLAY` set (X11 or Xvfb):

```bash
./scripts/cu-linux-smoke.sh
```

## Layering

```text
native primitive     agenterm-platform (owned there, consumed here)
    ↑
abstract command     agenterm-cu library (`Command`, typed `CuReply`)
    ↑
current transport    in-process `Executor` for target `current`
    ↑
shell command        `cu` binary
```

`cu` never opens raw OS APIs. Missing mechanisms are added to
`agenterm-platform` with typed `Available` / `Unsupported` / `Failed`.
