# 作战小队邮箱（SSOT）

有机感知：[fleet-awareness.md](fleet-awareness.md)  
协议：[inter-agent-comms.md](inter-agent-comms.md)  
登记表：[session-registry.md](session-registry.md)

最后由**主控1**刷新：2026-08-06T00:52Z（换防完成；主控2 待命）

## 共享事实

| 键 | 值 |
|----|-----|
| 产品版本 | **0.1.14**（`Cargo.toml`）；v0.1.15 计划另案 |
| 当前主线任务 | **server/CLI 首要**；CI 收口；CC 产品化不急 |
| 产品契约 | `prd/PRD_02_21_control_center.md` / `prd/PRD_02_02_executable_family.md` |
| 执行计划 | `plan/plan-control-center-ux.md` |
| `origin/main` | tip 见 git；含 CI 重跑 + research 三 Tab + Wry 体积倾向 + 换防登记 |
| CI | run `31060999962` @ `f3b95a5`（前次 `31059086660` @ `da25929` Windows 被 cancel）。观察中；Windows quality gate 待结论。docs 推送不触发 CI（paths-ignore） |
| 云环境 | Personal `mgttt/agenterm`；`environmentPublicId=7ef6e5b0-8a35-11f1-b532-320a589b8025` |
| SkinHub / 外置皮肤包 | **不做**（M14）；本任务仅内置四预设 |
| palette SSOT | `assets/skins/**/palettes/*.json`；`DARK`/`LIGHT` const 已删 |
| WebView | 仅 `research/agenterm-webview/`；三 Tab 占位；**体积优先 direct-WRY**（Win ~521KiB vs Tauri ~8.4MiB）；**勿**链入发布 `agenterm-cc`（4 MiB） |
| CC 远景 | 可吸收为 `agenterm cc` 子命令（独立进程/GUI）；超级智能体 + Hub；慢慢丰富 |
| auto-dream | **绿**：Automation `f2326638-…`；duty findings=0 |
| `duty.lock` | （无） |

## 主控指令（未消化则分身不得另起炉灶）

### → 分身3（皮肤设计）
1. IDLE 待命；fancy 终稿 icon 再开 `cursor/skins-design-icon`。
2. 仅 `assets/skins/fancy/icon.*` + `icon-direction.md`。

### → 分身4（皮肤工程）
1. Phase 2A/2B 已合 main；席位转 IDLE。
2. 无新指令勿另起炉灶；deferred 叶等新主控再派。

### → 主控1（当前）
1. 观察 CI `31060999962` 结论；Windows quality gate 若再挂/超时则查 hang。
2. 近程只推进 **server/CLI**；CC nav-chrome / WebView 产品化降级。
3. 若 WebView：优先 direct-WRY，Tauri 仅 research 对照。
4. 分身3/4 IDLE；暂不派活除非用户要求。

## 请示队列

（空）

## 席位状态

### 主控1 · 2026-08-06T00:52Z
- 状态: RUNNING — 当前主控；换防自 主控2
- bcId: `bc-a4df769a-f16d-4ee8-9bd3-6b1ce4e1097b`
- URL: https://cursor.com/agents/bc-a4df769a-f16d-4ee8-9bd3-6b1ce4e1097b
- 分支: `main`（代理元数据 `cursor/1-0e37` 勿当工作区）
- tip: 见 git
- 下一步: 等用户指令；观察 CI `31060999962`
- 阻塞: 无

### 主控2 · 2026-08-06T00:52Z
- 状态: IDLE — 已换防/待命；勿再当唯一统筹
- bcId: `bc-05b7c357-d712-440d-b140-8774bfa90e2a`
- URL: https://cursor.com/agents/bc-05b7c357-d712-440d-b140-8774bfa90e2a
- 下一步: 无新指令不开工
- 阻塞: 无

### 舰队值班会话 · 2026-08-06T01:26Z
- 状态: IDLE — 本轮 duty 结束；无新指令不开工
- bcId: `bc-98ea34bf-63c4-4682-b864-443c35d74774`
- URL: https://cursor.com/agents/bc-98ea34bf-63c4-4682-b864-443c35d74774
- 下一步: cron 下一轮再起
- 阻塞: 无

### 分身3 · 2026-08-05
- 状态: IDLE
- bcId: `bc-f2d5f6f1-41c0-44b1-bd74-1b80f036013b`
- 下一步: 终稿 icon → `cursor/skins-design-icon`

### 分身4 · 2026-08-05
- 状态: IDLE — Phase 2A/2B 已合 main
- bcId: `bc-c3e01145-b870-4e74-b18c-f3aea06ea800`
- 延后: Apply 后 Linux icon；Win fancy.ico embed；metrics 绘制

## 交接日志

- 2026-08-06T01:26Z · 舰队值班会话(`bc-98ea34bf-…`) · duty: noop findings=0 main=06c76b2；未 apply；lock 已清
- 2026-08-06T00:52Z · 主控1(`bc-a4df769a-…`) · 换防完成；主控2 待命；CI=`31060999962` 观察中
- 2026-08-06T00:51Z · 主控2(`bc-05b7c357-…`) · 已 spawn 主控1=`bc-a4df769a-…`；CI 重跑=`31060999962` @ `f3b95a5`；本席 IDLE
- 2026-08-06T00:49Z · 主控2(`bc-05b7c357-…`) · 准备换防；Wry>Tauri 体积结论已写入 plan/evidence
- 2026-08-06T00:26Z · 舰队值班会话(`bc-0c498e8e-…`) · duty: noop findings=0 main=da25929；未 apply；lock 已清
- 2026-08-05T23:26Z · 舰队值班会话(`bc-da35c7e5-…`) · duty: noop findings=0 main=30437eb；未 apply；lock 已清
- 2026-08-05T23:06Z · 舰队值班会话(`bc-0958a47a-…`) · duty: noop findings=0 main=478e131；env=`7ef6e5b0` 已对齐；未 apply；lock 已清
