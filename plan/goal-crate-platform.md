# /goal — agenterm-platform crate 跨平台封装收口

> 用法：把下面 `--- GOAL ---` 之间的内容整段发给 agent（或 `/goal` 加载本文件）即可。  
> 可执行到底；产品口径以本节「边界」为准，**不要**重新辩论「要不要做成库」。  
> 相关 SSOT：`crates/agenterm-platform/README.md`、`plan/ARCHITECTURE.md`、`AGENTS.md`、`src/frontend/ui_action_catalog.rs`。

--- GOAL ---

在仓库根（Windows 上常为 `D:\dev\agenterm`；Unix 以本机 clone 根为准）执行 **agenterm-platform 跨平台封装收口**：把「crate = 跨平台机制库」写死、可验收，并推进至少一处真实机制漏点或书面证伪 + 产品 shared-first 闸可执行。**自主执行到底**；不要中途问「需求是不是做成库」——需求未变。

## 产品意图（不变，禁止重开辩论）

- `crates/agenterm-platform` 是 **跨平台封装库**：agenterm 与 wbox（及其他 embedding app）共用。
- 应用通过 **契约 facade + feature** 碰 OS，禁止在 agenterm 产品代码里再散装第三套 Win/Lnx/OSX 实现同一机制能力。
- 库内 **禁止** AgenTerm 产品名、Fleet、工作台剧本（tab/server-strip/ui-action 表）、可执行文件命名策略——保证 wbox 可 `git` + **full SHA pin** 安全依赖。
- agenterm **跨端观感对齐**不靠「Win 做完再派 OSX/Lnx agent 抄」，而靠：
  1. **OS 能力**只进 `agenterm-platform`；
  2. **产品语义**只进 `src/frontend/*`、`src/ui_geometry.rs`、共享 snapshot/bridge；
  3. **host adapter**（`src/platform/adapters/windows/remote_frontend*`、`…/unix/frontend/*`）只 present / wake / IME / 原生控件映射。

### 边界一句话（agent 开工必须复述）

| 层 | 路径 | 装什么 |
|----|------|--------|
| 机制 | `crates/agenterm-platform` | 窗/键鼠/剪贴板/进程/IPC/PTY/字体/shm… typed Available/Unsupported/Failed |
| 产品语义 | `src/frontend/*`、`ui_*` | 手势含义、dialog 状态、geometry、action id |
| Host present | `src/platform/adapters/{windows,unix}/**` | 怎么画、怎么收事件、怎么接线 IPC |

「跨平台封装」= **机制进 platform crate**。  
「三端工作台手感齐」= **产品语义单点 + 两端 adapter 接线**，**不**把 AgenTerm 工作台塞进 platform。

## 背景（已成立，不要重新论证）

- Crate 已是 workspace member；agenterm `path` 依赖；外部消费者按 README `git`+rev。
- platform 机制面厚（process/fs/ipc/window…），审计与 feature 矩阵已存在。
- 产品层仍有 L2 债：Win remote vs Unix embedded **双拓扑** + 巨石 match；`ui_action_catalog.rs` 已是 **interim set-diff 闸**（SHARED / WINDOWS_ONLY / UNIX_ONLY），不是最终 ActionId 枚举。
- 体感「封装一直不稳」多半是 **机制层目标达成** 与 **产品双写未收** 混谈；本 goal 两者都碰，但 **不合并边界**。

## 任务（按序；P0→P3）

### P0 — 边界与 SSOT 写死（文档，同批）

1. `crates/agenterm-platform/README.md` 顶部（中或英，保持与全文一致即可）用极短段落写清：
   - **In scope**：typed OS contracts（window/input/ipc/pty/process/fs/clipboard/…）
   - **Out of scope**：product UI state、ui-action 剧本、instance/server strip、Fleet
   - **Consumers**：agenterm path；外部仓 git + full SHA pin + `default-features = false`
2. `plan/ARCHITECTURE.md` §1：三层表（platform / frontend / adapters）与上表一致；链到 `src/frontend/ui_action_catalog.rs` 为产品 `ui-action` 集合闸（interim）。
3. `AGENTS.md`：一小节 **Platform crate vs product UI**，禁止「platform 负责全部 UX」或「UX 可只在 Win adapter 落地」的歧义。

验收：三处口径互不矛盾；新人读完不会以为 wbox 要依赖 AgenTerm 工作台。

### P1 — 机制漏点清单 + 至少一处收口

1. 扫 `src/platform/adapters/**` 与相关产品热路径，找仍 **直接** 调 Win32/winit/x11/Cocoa、且语义属于 platform 契约能力的站点（剪贴板、截图、激活、字体、窗几何、进程 spawn、native IPC 等）。
2. 新建 `plan/plan-platform-encapsulation-gap.md`（短表）：
   - 列：`site` | `should live in` (platform feature 名) | `priority` | `notes` / `parity-gap?`
   - **只列机制债**；server strip / settings 全量复刻等产品叶不进此表（可指到 `plan-unix-gui-win-parity.md`）。
3. 本轮 **至少**：
   - **修 1 个**真实高价值漏点：散装 OS 调用迁回 `agenterm-platform` facade，agenterm 只调库；**或**
   - **书面证伪**「已无此类漏点」并在 gap 表写证据路径。  
   迁不动：写清阻塞（契约缺失 / 双拓扑 / 巨石耦合），**禁止**空转大重构。

验收：gap 文件存在且可当下一切输入；代码或证伪二选一落地。

### P2 — 产品 shared-first 闸可执行

1. 确认/完善 `src/frontend/ui_action_catalog.rs`：
   - SHARED ∪ WINDOWS_ONLY ∪ UNIX_ONLY 与集合差单测绿
   - 源码字面量存在性单测绿
   - 每个 ONLY 条目有 `parity-gap:` 原因（注释）
2. 文档或测试体现纪律：**新增产品手势** 必须先改 catalog，再改 adapter（可引用 `AGENTS.md` Cross-platform UI 节）。
3. 从 `WINDOWS_ONLY` 挑 **至多 1 个** 可诚实升 SHARED 的动作（优先：已有共享 dialog/geometry、Unix 已部分存在的周边）；能升则两端接线；升不了则只更新 gap 理由。  
   **禁止**本轮啃完整 settings / instance-picker / server-strip 全量 Unix 复刻。

验收：`cargo test --lib ui_action_catalog` 绿。

### P3 — 后续 agent 固定执行句式（写入 gap 文或 ARCHITECTURE 短节）

任何跨平台 UI/机制任务必须按序：

1. 判定：platform 机制 / frontend 产品语义 / host present？
2. 机制 → 改 `crates/agenterm-platform`，feature 与 typed Unsupported **诚实**更新。
3. 产品 → 改 `src/frontend/*` + catalog，再改 **两端** adapter。
4. 仅 host → 只动对应 adapter，并登记 catalog allowlist 或 gap 表。
5. 证据：相关 `cargo test -p agenterm-platform` + `cargo test --lib ui_action_catalog` + 直接单测；**无证据不宣称三端手感已齐**。

## 明确非目标（禁止）

- 把 AgenTerm 工作台 / Fleet / ui-action 剧本塞进 `agenterm-platform`。
- 本轮重写 remote vs embedded 为单一拓扑（可记后续叶，不实施）。
- 新开 git worktree / 任务分支；在 **main 共享 checkout** 小步改。
- 发版链、CC 大改、全量 settings Unix、全量 server-strip Unix。
- `git add -A` / 混入无关 agent 文件；擅自 `git push`（除非用户本轮明文要求）。
- 把 Script `profile` / safe 当权限边界（勿碰）。

## 工作纪律

- 遵守 `AGENTS.md`、`plan/ARCHITECTURE.md`。
- PowerShell 或本机 shell；验证优先：
  - `cargo test --lib ui_action_catalog`
  - `cargo test -p agenterm-platform --lib`（改了 platform 时）
  - 直接相关单元测试；**不要**默认 full `check.cmd`。
- 热文件：`remote_frontend.rs` / `unix/frontend/mod.rs` **最小接线**；勿顺手大重构。
- pathspec 精确暂存；`cargo fmt` 限定包。
- 并发 agent：改前 `git status`；勿覆盖他人未提交域。
- **Commit / push 仅当用户本轮明确要求**；默认改完报告 diff 与验证结果即可。

## 成功标准（检查清单）

- [x] platform README + ARCHITECTURE + AGENTS 对「crate = 跨平台机制封装」口径一致
- [x] `plan/plan-platform-encapsulation-gap.md` 存在且可执行
- [x] ≥1 机制漏点收口 **或** 书面证伪无漏点（G1 breakaway + G2 证伪 + G6/G7/G8 catalog）
- [x] `ui_action_catalog` 测试绿；shared-first 路径可执行
- [x] 报告：见 gap 文「封装完结定义」+ plan-v0.1.15 决策记录；推送 `origin/main`

## 开工第一句（强制自检，≤15 行）

回复是否同意：

> 跨平台封装 = 机制进 `agenterm-platform`；产品语义进 `src/frontend`；adapter 只 present。  
> 需求未变：crate 供 agenterm + wbox 调用。本轮不把工作台剧本塞进 crate。

若不同意 → **停写**，在报告里写出分歧；同意 → 从 P0 开始执行。

## 已知未决（做完列在报告，禁止本轮自行拍板产品方向）

1. remote vs embedded 是否中长期收敛为单一主机模型。
2. server-strip / instance-picker 是否进 Unix 产品面（排期在 `plan-v0.1.15` S′ / parity 地图，非本 goal 必做）。
3. platform 是否未来加厚「中性 pixel/control toolkit」仍零产品名（扩 scope，需另授权）。

--- END GOAL ---

## 备注（给人看，不进 agent 强制正文）

- 与已归档的 [`archive/goal-cli-input-parity.md`](archive/goal-cli-input-parity.md) 互补：历史 goal 偏 CLI `ui-input` 像素原语；**本文件偏 crate 边界 + 机制漏点 + catalog 闸**。
- 与 [`plan-unix-gui-win-parity.md`](plan-unix-gui-win-parity.md) 关系：parity 地图管**可见行为差距**；本 goal 管**封装归属与闸**。
- 产出的 `plan-platform-encapsulation-gap.md` 落地后应挂进 [`plan/README.md`](README.md) 现行表（执行本 goal 的 agent 应更新索引一行）。
