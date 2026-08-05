# 作战小队邮箱（SSOT）

协议：[inter-agent-comms.md](inter-agent-comms.md)  
登记表：[session-registry.md](session-registry.md)

最后由**主控2**刷新：2026-08-05

## 共享事实

| 键 | 值 |
|----|-----|
| 产品版本 | **0.1.14**（`Cargo.toml`）；v0.1.15 计划另案 |
| 当前主线任务 | **Built-in skins v1** — Phase 2A/2B **已合 main** |
| 产品契约 | `prd/PRD_02_06_human_workspace.md` § Built-in skins (v1) |
| 执行计划 | `plan/plan-skins-v1.md`（含 Phase 2B deferred） |
| `origin/main` | 含 `cursor/skins-eng-phase2a` @ `669b8de` 合流 |
| 云环境 | Personal `mgttt/agenterm`；x86_64 Linux + DISPLAY=:1 |
| SkinHub / 外置皮肤包 | **不做**（M14）；本任务仅内置四预设 |

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
- 状态: RUNNING — 编排皮肤 v1
- 分支: `main`
- 下一步: Phase 3 集成验证（Quick / theme-smoke）；PRD 勾选；deferred 排期
- 阻塞: 无
- 已合: 分身3 Phase 1；分身4 Phase 2A/2B（`669b8de`）

### 分身3 · 2026-08-05
- 状态: IDLE
- bcId: `bc-f2d5f6f1-41c0-44b1-bd74-1b80f036013b`
- URL: https://cursor.com/agents/bc-f2d5f6f1-41c0-44b1-bd74-1b80f036013b
- 下一步: 待命；终稿 icon 叶
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
