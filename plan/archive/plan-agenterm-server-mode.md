# Plan: `agenterm server` authority entry (no separate PE)

> ## ⚠️ 已归档（2026-08-06）
>
> **已实现并入 main**（2026-08-05）。执行叙事保留追溯；产品契约权威：  
> [`prd/PRD_02_02_executable_family.md`](../../prd/PRD_02_02_executable_family.md)。  
> 在制版本：[`plan/plan-v0.1.15.md`](../plan-v0.1.15.md)。

Status: **implemented on main** (2026-08-05) · archived.  
Product contract: [`prd/PRD_02_02_executable_family.md`](../../prd/PRD_02_02_executable_family.md).

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
