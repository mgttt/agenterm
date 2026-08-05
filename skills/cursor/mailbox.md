# 作战小队邮箱（SSOT）

有机感知：[fleet-awareness.md](fleet-awareness.md)  
协议：[inter-agent-comms.md](inter-agent-comms.md)  
登记表：[session-registry.md](session-registry.md)

最后由**舰队值班会话**刷新：2026-08-05T23:26Z（duty 一轮）

## 共享事实

| 键 | 值 |
|----|-----|
| 产品版本 | **0.1.14**（`Cargo.toml`）；v0.1.15 计划另案 |
| 当前主线任务 | **`agenterm server` 子命令入口** — 计划 `plan/plan-agenterm-server-mode.md`；`--server` 仅过渡别名 |
| 产品契约 | `prd/PRD_02_06_human_workspace.md` § Built-in skins (v1) |
| 执行计划 | `plan/plan-skins-v1.md`（含 Phase 2B deferred + post-merge cleanup） |
| `origin/main` | `30437eb`；skins Phase 2B / palette SSOT / preset grid 已合 |
| 云环境 | Personal `mgttt/agenterm`；`environmentPublicId=7ef6e5b0-8a35-11f1-b532-320a589b8025` |
| SkinHub / 外置皮肤包 | **不做**（M14）；本任务仅内置四预设 |
| palette SSOT | `assets/skins/**/palettes/*.json`；`DARK`/`LIGHT` const 已删 |
| auto-dream | **绿**：Automation `f2326638-…` 已绑主控2 同环境；本轮 `bc-da35c7e5-…`（显示名「舰队值班会话」）完整跑通 |
| `duty.lock` | （无） |

## 主控指令（未消化则分身不得另起炉灶）

### → 分身3（皮肤设计）
1. IDLE 待命；fancy 终稿 icon 再开 `cursor/skins-design-icon`。
2. 仅 `assets/skins/fancy/icon.*` + `icon-direction.md`。

### → 分身4（皮肤工程）
1. Phase 2A/2B 已合 main；席位转 IDLE。
2. 无新指令勿另起炉灶；deferred 叶等主控2 再派。

## 请示队列

（空）

## 席位状态

### 主控2 · 2026-08-05
- 状态: RUNNING — 与用户对话中（造梦误标 IDLE 已更正）
- 分支: `main` / 代理分支 `cursor/2-c843`
- 下一步: 等用户；deferred skins（Linux Apply icon / Win fancy.ico / metrics paint）按需再派
- 阻塞: 无
- 已合: skins Phase 1–2B；`bb5ef28` cursor_agent.py；palette SSOT；`appearance_preset_grid`
- 舰队: duty findings=0；auto-dream 已通（Automation `f2326638-…`）

### 舰队值班会话 · 2026-08-05T23:26Z
- 状态: IDLE — 本轮 duty 结束；无新指令不开工
- bcId: `bc-da35c7e5-9c42-4939-b03a-a7e136927eb5`
- URL: https://cursor.com/agents/bc-da35c7e5-9c42-4939-b03a-a7e136927eb5
- 分支: `main`（duty 工作区）
- 下一步: cron 下一轮再起
- 阻塞: 无

### 分身3 · 2026-08-05
- 状态: IDLE — fleet-pulse 已感知；无新指令不开工
- bcId: `bc-f2d5f6f1-41c0-44b1-bd74-1b80f036013b`
- URL: https://cursor.com/agents/bc-f2d5f6f1-41c0-44b1-bd74-1b80f036013b
- main基准: `0f6948e`
- 下一步: 终稿 icon → `cursor/skins-design-icon`（仅 `assets/skins/fancy/icon.*` + `icon-direction.md`）
- 阻塞: 无

### 分身4 · 2026-08-05
- 状态: IDLE — Phase 2A/2B 已合 main
- bcId: `bc-c3e01145-b870-4e74-b18c-f3aea06ea800`
- URL: https://cursor.com/agents/bc-c3e01145-b870-4e74-b18c-f3aea06ea800
- 分支: `cursor/skins-eng-phase2a`（合完即删）
- 证据: tip `669b8de`；`cargo test --lib` 620 passed（自称）
- 延后: Apply 后 Linux icon；Win fancy.ico embed；metrics 绘制
- 下一步: 待命
- 阻塞: 无

## 交接日志

- 2026-08-05T23:26Z · 舰队值班会话(`bc-da35c7e5-…`) · duty: noop findings=0 main=30437eb；未 apply；lock 已清
- 2026-08-05T23:06Z · 舰队值班会话(`bc-0958a47a-…`) · duty: noop findings=0 main=478e131；env=`7ef6e5b0` 已对齐；未 apply；lock 已清
