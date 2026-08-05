# Plan: `agenterm server` authority entry (no separate PE)

Status: **implemented on main** (2026-08-05).  
Product contract: [`prd/PRD_02_02_executable_family.md`](../prd/PRD_02_02_executable_family.md).

## Outcome

- Preferred authority entry: **`agenterm server`** (subcommand, separate process).
- **Deleted** the `agenterm-server` binary / dist member.
- Windows GUI autostart spawns `current_exe server …` (same PE, new process).

## Accepted trade-off

Windows locks a running PE. With GUI and authority sharing `agenterm.exe`,
replacing that file while Keep Server is active may fail until the authority
stops. Product choice: fewer executables over image-isolated upgrade.

## Explicit non-goals

- Reintroduce `agenterm-server.exe`
- Merge mux/mcp/rhai
- Change Unix embedded GUI ownership model beyond entry naming
