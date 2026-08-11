# 自宿主架构：三层温度分层 + fleet OS 哲学转向内侧

| 字段 | 值 |
|------|-----|
| **主题** | 主 crate `src/` 内部耦合位置的终极形态——让 agenterm 的内部架构与它的产品理念同构 |
| 日期 | 2026-08-11（rev5：新增 §8.1 深度技术与耦合改良的互补关系 + 全文最终梳理） |
| 状态 | 概念提案（idea garden，未排期，非任务单） |
| 作者 | GLM 5.2 宏观 review（2026-08-11）产出 |
| 野心声明 | **产品内部极度牛逼技术、整体架构稳健同时足够灵活、灵活甚至动态适应 UI/UX 的需求变更与进化** |
| 关联 | `plan/ARCHITECTURE.md`（结构 SSOT、L2/L3 债务）、`plan/design-script-engine-trait.md`（trait 统一已落地的先例）、`plan/design-frontend-shared-core.md`（双前端巨石测绘）、`plan/design-binary-size-and-reuse.md`（体积归因）、`plan/design-release-base-vs-apps.md`（base vs app 分轨发布）、`plan/plan-v0.1.18.md`（Portable App Substrate）、`prd/PRD_02_07_agent_control_plane.md`、`prd/PRD_02_10_rhai_scripting.md`、`prd/PRD_02_21_control_center.md` |
| 范围声明 | **只读 + 设计提案**；本文档不修改任何 `.rs` 文件，不替代 `ARCHITECTURE.md` 的结构 SSOT 地位 |

---

## 0. 一句话

agenterm 的产品理念是 **fleet OS——把多个进程组织成一棵可见、可控、可验证、detach 不死的树**。
但 agenterm 的内部代码**没有体现这个理念**：60+ 模块挤在一个 crate 里，裸函数调用，没有可见的
依赖树、没有可控的层间边界、没有可验证的模块契约。

**本文的核心主张：agenterm 应该成为自己的第一个 dogfood 用户——用 fleet OS 哲学重构自己的内部
架构。** 这不是代码整洁运动，是产品理念的 self-hosting（自宿主）。

### 全文导读

| 章节 | 一句话 | 给谁看 |
|------|--------|--------|
| §1 初心校准 | 三个诉求（牛逼 + 稳健 + 灵活）两两冲突，唯一解法是分层 | 想理解"为什么"的人 |
| §2 三层温度架构 | hot 应用层 / cold 契约层 / frozen 机制层——各层承担不同诉求 | 想理解"是什么"的人 |
| §3 self-hosting | 产品理念 ↔ 内部架构的同构对应 | 想理解哲学的人 |
| §4 诊断 | 真实依赖数据 + 方向倒转问题 + 耦合机制 + 沉淀困境 | 想理解"当前痛点"的人 |
| §5 四个手段 | Trait(P0) / Event Spine(P1) / Capability(P2) / 双模(P3) | 想理解"怎么做"的人 |
| §6 与 v0.1.18 的关系 | App Pack 就是 hot 层载体，Host ABI 就是契约层 | 关心版本规划的人 |
| §7-8 落地收益 | 架构债务对应 + 多 agent 并发 + codex 互补关系 | 关心工程回报的人 |
| §9-10 边界与行动 | 非目标 + 按优先级排序的后续动作 | 准备动手的人 |

---

## 1. 初心校准：三个诉求与一个矛盾

### 1.1 野心拆解

| 诉求 | 含义 | 结构性张力 |
|------|------|-----------|
| **极度牛逼技术** | 碰到物理层（FFI / 汇编）、形式化验证级别，不只是"能跑" | 牛逼技术通常**重**——`transpile.rs` 11700 行是牛逼，但也是单点风险 |
| **稳健** | 在规模增长 10 倍、多 agent 并发、跨平台时不腐烂 | 稳健要求**强约束**——边界硬、契约明、漂移可检测 |
| **灵活到动态适应 UI/UX** | UI 层的需求变更是高频的，架构要能接住甚至运行时适应 | 灵活要求**弱耦合**——改 UI 不该牵连底层重编译重测试 |

### 1.2 两两冲突

```
牛逼技术 ←→ 灵活：越牛的技术通常越专、越重、越难改
稳健    ←→ 灵活：约束越强越稳健，但改起来越痛
牛逼技术 ←→ 稳健：越复杂的实现越难验证、越脆弱
```

大多数项目只能选两个：
- 牛逼 + 灵活 = 研究原型（酷但不稳定）
- 稳健 + 灵活 = 典型好工程（但不惊艳）
- 牛逼 + 稳健 = 航天软件（稳健但僵硬）

**三个都要的唯一解法：不在同一层面同时满足，而是分层。** 每一层承担不同的诉求。

### 1.3 "牛逼"的重新定义

有两种牛逼：

| 类型 | 含义 | 例子 | 寿命 |
|------|------|------|------|
| **局部牛逼** | 单个模块做到物理级极致 | codex 死磕 con 的汇编 FFI、vt100 parser 硬化 | 一旦做对就冻结 |
| **架构级牛逼** | 分层干净到各层可以"无视彼此存在" | UI 团队改 UI 不知道底层 PTY 是什么；底层优化不知道 UI 在做什么实验 | 架构的寿命 > 任何单点实现 |

**局部牛逼让单个模块极致；架构级牛逼让整个系统可进化。** agenterm 需要两者都有——但只有架构级
牛逼才能支撑"灵活到动态适应 UI/UX 进化"。

---

## 2. 核心洞察：三层温度架构

### 2.1 分层图

```
         UI/UX 进化在这里发生（hot）
         ┌──────────────────────────────────────────┐
         │  应用层 / Apply Layer                     │
APPLY    │  skin / layout / workflow / composer 行为 │  ← 每天每周变
(hot)    │  → rh / qjs 脚本定义行为                  │  ← .agp App Pack 热替换
         │  → 不编译进 PE                            │  ← v0.1.18 Portable App Substrate
         └──────────────┬───────────────────────────┘
                        │ 只通过 Intent/Effect 通信
                        │ 不 import 任何下层类型
         ┌──────────────┴───────────────────────────┐
CONTRACT │  契约层 / Contract Layer                  │  ← 每月每季变
(cold)   │  Trait 定义 + Protocol DTO                │  ← 这是"稳健"的锚
         │  Intent / Effect 类型化消息               │  ← 改它需要最高审查
         │  Capability 声明                          │
         │  Receipt 机制 + event_journal spine       │
         └──────────────┬───────────────────────────┘
                        │ 只有 trait 方法调用
                        │ 依赖注入
         ┌──────────────┴───────────────────────────┐
MECHANISM│  机制层 / Mechanism Layer                 │  ← 做对后不动
(frozen) │  ConPTY / PTY / 窗口 / FFI / vt100        │  ← 极度牛逼技术住这里
         │  rh AOT / qjs engine / IPC transport      │  ← codex 的汇编 FFI
         │  process supervisor / pixel-window        │
         │  → 编译进 PE，极少变更                     │
         └──────────────────────────────────────────┘
```

### 2.2 每层的职责与温度

| 层 | 温度 | 承担的诉求 | 变更频率 | 形态 |
|----|------|-----------|---------|------|
| **应用层** | hot | 灵活 / 动态适应 | 每天 / 每周 | `.agp` App Pack（rh/qjs 脚本）+ 主 crate 内的 UI 语义 |
| **契约层** | cold | 稳健的锚 | 每月 / 每季 | trait 定义 + typed message + capability 声明 |
| **机制层** | frozen | 极度牛逼技术 | 做对后不动 | 编译进 PE 的 Rust 代码（platform crate + engine crate） |

### 2.3 三个诉求不再打架

- **极度牛逼技术**沉底在 frozen 层：ConPTY FFI、vt100 parser、rh AOT transpiler——一旦做对
  就冻结，上层的变更不触碰它。
- **稳健**锚定在 cold 层：trait + protocol + capability 声明——这是整个架构的合同，改它
  需要最高审查，但一旦定稿就为上下两层提供确定性。
- **灵活**浮顶在 hot 层：UI 语义、皮肤、布局、workflow——这些高频变更被锁在应用层，不扩散
  到其他层，不触发机制层重编译。

**关键不变量：hot 层的变更永远不应该迫使 cold 层或 frozen 层重新编译或重新验证。** 这是
"灵活到动态适应 UI/UX 进化"的形式化定义。

---

## 3. self-hosting：产品理念与内部架构的同构

### 3.1 对应表

agenterm 的产品理念（fleet OS 的五个核心能力 ORG→OBS→INT→DUR→AUTO）如何映射到内部架构：

| 产品理念 | 产品里做什么 | 内部架构对应物 | 当前状态 |
|---------|-------------|--------------|---------|
| **ORG（组织）** | 进程组织成 tab 树，父关子提升 | 模块按依赖图组织成层，依赖方向单向（hot→cold→frozen） | ❌ 当前是扁平 pub mod，无层次 |
| **OBS（观测）** | 事件纪元/序列日志 | 跨层通信经 event_journal spine，自动有 trace | ⚠️ journal 存在但是事后审计，不是通信脊柱 |
| **INT（干预）** | typed control ops + receipt | 层间调用经 trait，返回 Receipt<T> 而非裸值 | ⚠️ ScriptEngineBackend trait 已证明，但未推广 |
| **DUR（持久）** | detach-first，关窗不死 | 应用层（.agp）热替换时机制层进程不重启 | 📋 v0.1.18 规划中 |
| **AUTO（自动化）** | rh 脚本编排 + MCP | CI / smoke / validation 用 rh 自动化 | ✅ 已对 |

### 3.2 一句话哲学

> **agenterm 不只是 fleet OS for processes；它本身就是用 fleet OS 哲学构建的软件。**

产品上你做的是：进程组织成树、事件可审计、资源有租约、控制有契约、detach 不死。
内部架构上也应该是：模块组织成依赖树、跨层通信可审计、层间访问有 capability、层间契约有
trait 定义、应用层热替换时机制层 detach 不重启。

### 3.3 这不是 vanity project

self-hosting 不是自恋——它有直接的工程回报：

1. **每个内部架构决策都是一次产品 dogfood**。如果 event_journal 作为内部通信脊柱不好用，
   那它作为产品功能也不好用——你会立刻发现。
2. **内部架构的约束工具（capability/trait/receipt）就是产品要卖给用户的能力**。agenterm 卖
   的是"可验证的控制"——内部架构有可验证的控制，意味着这些机制经过了自身的压力测试。
3. **多 agent 协同变成 fleet 管理的子问题**。每个 agent = 一个 worker，领地 = workspace
   path，海关 = capability boundary，handoff = event_journal typed event。agenterm 自己的
   fleet 机制就是多 agent 编排器。

---

## 4. 诊断：已有的分层零件与缺口

### 4.1 已经在正确位置的部分

| 已有实现 | 当前位置 | 应属层级 | 状态 |
|---------|---------|---------|------|
| `agenterm-platform` crate（FFI/窗口/PTY/输入/IME/剪贴板） | 机制层 | frozen | ✅ 已对 |
| `boundary_tests.rs`（platform↔product 红线） | 机制↔契约边界 | frozen↔cold | ✅ 已对 |
| `ScriptEngineBackend` trait（四引擎统一） | 契约层 | cold | ✅ 已对，已证明 |
| `fleet_call(operation_id, params_json)` | 契约层 | cold | ✅ 已对 |
| `protocol.rs` / `operations.rs`（typed ops） | 散在主 crate | cold | ⚠️ 内容对，但未独立成层 |
| `event_journal.rs`（epoch/sequence/回放） | 事后审计 | 应是 cold 的 spine | ⚠️ 零件对，角色错位 |
| `ui_lease.rs`（资源租约） | 散在主 crate | cold | ✅ 零件对 |
| rh/qjs 脚本引擎（App Pack 载体） | 机制层 | frozen（引擎本身） | ✅ 已对 |
| v0.1.18 `.agp` Portable App Substrate | 规划中 | hot（应用层载体） | 📋 已规划 |

### 4.2 缺口

| 缺什么 | 影响 | 对应手段 |
|--------|------|---------|
| **契约层未显性独立**——protocol/operations/trait 散在主 crate | 层间边界靠纪律不靠结构；改 frontend 仍可能触发底层重编译 | 手段 C（Trait as ABI） |
| **跨模块通信走函数调用不走消息**——event_journal 是事后审计 | 模块间隐式耦合，集成测试靠实时编排 | 手段 A（Event Spine） |
| **pub mod 全局可见**——没有"模块只能依赖谁"的声明 | 60 个模块互相可见，依赖关系不可见不可约束 | 手段 B（Capability-Scope） |
| **应用层未从主 crate 分离**——UI 语义和底层混在一个编译单元 | 改 UI 触发全 crate 重编译；无法独立交付 UI 变更 | 手段 D（双模编译）+ v0.1.18 App Pack |

### 4.3 真实依赖拓扑（基于 `src/` 实测数据，2026-08-11）

对 `src/` 全部 `.rs` 文件扫描 `use crate::` 语句（共 ~126 条），统计入度（被多少模块依赖）
和依赖方向。

#### 入度最高的模块（耦合中心）

| 被依赖的模块 | 入度 | 依赖者 | 角色 |
|---|---|---|---|
| `script_protocol` | **8** | client, script_engine, script_backend, script_worker, script_rh_run, script_lua_run, worker_supervisor×2 | 脚本域的事实标准 |
| `platform::*` | **8+** | commands, frontend, script_stdlib, script_process, script_audit, worker_supervisor… | 机制层（正常——本该被所有人依赖） |
| `frontend::*` | **5** | ⚠️ 被 `platform/adapters/{unix,windows}/frontend/*` 依赖 | **方向反了** |
| `ui_bridge` | **3** | ui_lease, ui_interaction, frontend/tab_editor | UI 常量/约束共享点 |
| `operations` | **3** | client/mod, agent_tools, script_fleet | 控制平面的事实标准 |
| `ipc_endpoint` | **5** | control_center, mcp_catalog, frontend, instance_picker, instance_identity | IPC 寻址枢纽 |
| `instances` | **3** | frontend_server, instance_picker, instance_identity | 实例发现枢纽 |

#### 最严重的问题：依赖方向倒转

依赖本该单向流 `hot → cold → frozen`。但实测发现 frozen 层的 adapter 反向依赖 hot 层：

```
platform/adapters/                         ← 本该在 frozen 机制层
  ├── unix/frontend/mod.rs     ──→ crate::frontend::*           ← ~74 条 use 指向 hot 层
  ├── unix/frontend/render.rs  ──→ crate::theme, locale,
  │                               terminal_cursor, ui_geometry   ← 指向 hot 层
  ├── unix/frontend/wake.rs    ──→ crate::frontend, wake_signal ← 指向 hot 层
  ├── unix/frontend/window_state ──→ crate::commands             ← 指向 cold 层
  ├── windows/remote_frontend.rs ──→ crate::frontend::interaction,
  │                                  ui_snapshot                 ← 指向 hot 层
  └── windows/frontend.rs      ──→ crate::frontend::*, wake_signal ← 指向 hot 层
```

**这就是 L2 债务的根源**。adapter（机制层）反过来依赖了 frontend（应用层）。这不是纪律问题——
是 `frontend` 和 `platform/adapters` 的关系定义反了。frontend 在扮演 controller 角色，
adapter 在扮演 view 角色，但它们被放在了"机制层依赖应用层"的物理结构里。

#### 四个隐性域

从依赖数据看，`src/` 已经隐性地分成四个域：

```
┌─ 脚本引擎域（耦合最健康）─────────────────────────────┐
│  script_protocol (8 入度，域内核 / 事实标准)            │
│  script_engine → script_backend → script_rh_run        │
│  script_worker, script_lua_run, script_qjs_host        │
│  script_fleet, script_http, script_net, script_process │
│  script_clipboard, script_image, script_stdlib         │
│  script_catalog, script_api_view, script_api_validate  │
│  script_task, script_stream, script_project, script_error │
│  agent_tools                                           │
│  ★ 内部以 script_protocol 为契约，依赖方向正确          │
└────────────────────────────────────────────────────────┘

┌─ 控制平面域 ──────────────────────────────────────────┐
│  operations (3 入度，域内核 / 事实标准)                 │
│  protocol → control_contract                           │
│  commands, client/mod, server_app                      │
│  control_authority, control_dispatch, control_center   │
│  ipc_endpoint, ipc_transport, instances                │
│  mcp_catalog, mcp_fleet, mcp_stdio                     │
│  event_journal, worker_supervisor                      │
│  ★ operations 是事实标准，但没独立成 trait              │
└────────────────────────────────────────────────────────┘

┌─ 前端/UI 域 ──────────────────────────────────────────┐
│  ui_bridge (3 入度，常量/约束共享点)                    │
│  ui_snapshot, ui_geometry, ui_interaction              │
│  ui_lease, ui_client, ui_command, ui_clipboard         │
│  frontend/ (21 个子模块)                               │
│  theme, locale, settings, wake_signal                  │
│  terminal_runtime, terminal_cursor, terminal_lifecycle │
│  ⚠️ 被 platform/adapters 反向依赖                      │
└────────────────────────────────────────────────────────┘

┌─ 平台机制域 ──────────────────────────────────────────┐
│  agenterm_platform (crate，独立编译单元)               │
│  platform/ (产品 glue：policy + contract + services)   │
│  platform/adapters/{windows, unix, linux, macos}       │
│  ★ platform crate 本身健康（boundary_tests 保护）       │
│  ⚠️ platform/adapters 反向依赖前端/UI 域              │
└────────────────────────────────────────────────────────┘
```

#### 关键发现

1. **`script_protocol` 就是 trait-as-ABI 的已验证形态**——8 个入度，全仓库入度最高的非
   platform 模块。八个模块通过它通信而不是直接互调。这已经是一个事实上的契约层，只是用
   DTO/enum 形态而非 trait 形态。
2. **agenterm 团队已经知道"共享协议通信"优于"直接函数调用"**——脚本域这样做了。只是没有
   推广到其他域。
3. **核心问题不是"依赖太多"而是"依赖没有方向"**——平均入度 2-3 并不严重；74 条反向依赖
   才是漂移腐烂的根源。

### 4.4 依赖改良的核心原则：方向性优先于减少

**混淆的概念**：人们常说"降低耦合"，但混了两个不同的问题。

| | 依赖太多（广度问题） | 依赖方向错（方向问题） |
|---|---|---|
| 现象 | 模块 A 依赖 20 个模块 | frozen 层依赖 hot 层 |
| 感受 | "耦合太重" | "改 UI 牵连底层" |
| agenterm 情况 | 还好（平均入度 2-3） | **这才是真问题**（74 条反向） |

同样数量的 `use` 语句，如果全部单向流动（hot→cold→frozen），系统是健康的；如果有一部分
倒流（frozen→hot），系统就会漂移腐烂。

**agenterm 的核心问题不是"依赖太多"，是"依赖没有方向"。**

因此改良的优先序是：

```
第一步：消除反向依赖（搬代码，不加 trait）     ← 方向性
  adapter 只保留 present / wake / IME / pixel-render
  产品语义搬回 frontend/
  → 74 条反向 use 归零

第二步：把事实标准提升为 trait（加 trait）       ← 固化正确方向
  script_protocol 已是事实标准（8 入度）→ 保持
  operations 是事实标准（3 入度）→ 提取为 trait ControlHost
  ui_bridge 是事实标准（3 入度）→ 提取为 trait UiHost
  → 调用方只依赖 trait，不 import 具体类型

第三步：trait 放独立 crate（物理隔离）          ← 编译期隔离
  crates/agenterm-protocol/ 定义所有 trait + DTO
  → Cargo fingerprint 天然隔离，多 agent 并发编译成立
```

**trait 是第二步的工具，不是第一步的解药。** `ScriptEngineBackend` trait 能成功，是因为脚本域
的依赖方向本来就是对的（调用方依赖被调方，没有反向）。trait 只是把正确的方向固化。如果先加
trait 而不纠正方向，trait 会把错误的方向也固化——更难改。

### 4.5 三阶段升级路线

```
现在              阶段 1              阶段 2              阶段 3
                 消除反向依赖          trait 契约          应用层脱离编译

frontend ←── adapter    frontend         trait ControlHost    .agp App Pack
    ↑        ↑              ↑                  ↑                  ↑
    │        │              │                  │                  │
  ui_bridge  │          ui_bridge         ui_bridge           ui_bridge
    │        │              │                  │                  │
 operations  │          operations         operations          operations
    ↑        │              ↑                  ↑                  ↑
    │     (反向消除)         │              trait 定义             │
    │                      │                  │                  │
  platform ──┘          platform          platform            platform
                       (只被依赖)         (只被依赖)           (只被依赖)
```

| 阶段 | 目标 | 做法 | 解决的问题 |
|------|------|------|-----------|
| **阶段 1**（近期） | 消除反向依赖，确立方向 | 产品语义从 adapter 搬回 frontend；adapter 只留 present/wake/IME/render | 74 条反向 use 归零；L2 漂移根源消除 |
| **阶段 2**（中期） | 事实标准提升为 trait 契约 | operations → trait ControlHost；ui_bridge → trait UiHost；script_protocol 保持 | 调用方只依赖 trait，实现可替换 |
| **阶段 3**（远期） | trait 独立 crate + 应用层脱离编译 | trait 放 `crates/agenterm-protocol/`；应用层 UI 语义变 `.agp` 包 | Cargo fingerprint 隔离；UI 变更不触发重编译 |

### 4.6 耦合机制：现在用什么，未来用什么（零新发明）

#### 现状：100% Rust 原生 `use crate::`

当前所有模块间的耦合全部是 Rust 语言原生的 `mod` + `use crate::`：

```rust
// src/lib.rs 声明
pub mod operations;
pub mod frontend;
pub mod script_protocol;

// src/client/mod.rs 使用
use crate::operations::operation_by_id;  // ← 这就是耦合，Rust 原生 import
```

没有任何自定义机制——没有依赖注入框架、没有消息总线、没有 RPC。就是 Rust 的模块系统：
`pub mod` 让模块全局可见，`use crate::` 让另一个模块引用它。

这就是为什么 60+ 模块互相全可见——Rust 的 `pub mod` 天然就是"对 crate 内所有人公开"。
Rust 原生不提供"frontend 不能 import platform 的具体类型"这种粒度的可见性控制（只有
`pub` / `pub(crate)` / `pub(super)` 三档，太粗）。

#### 未来：仍然是 100% Rust 原生，不发明任何新机制

三层温度架构不需要任何非 Rust 机制。它改变的是"可见性策略"和"调用方式"，但用的全部是
Rust 语言已有的功能：

| 层间关系 | 耦合机制 | 是新发明的吗 |
|---------|---------|------------|
| 应用层 → 契约层 | `use crate::protocol::TerminalHost`（trait） | ❌ Rust 原生 trait |
| 契约层 → 机制层 | trait 的 impl（`impl TerminalHost for Workspace`） | ❌ Rust 原生 impl |
| 应用层 → 机制层 | **不允许直接 use**，只能经契约层 trait | ❌ 就是"不写 use"，Rust 原生 |
| trait 放哪 | 独立 crate `crates/agenterm-protocol/` | ❌ Rust 原生 workspace crate |
| Event Spine | in-process channel + 普通 enum | ❌ Rust 标准库 `mpsc` / `broadcast` |
| Capability-Scope | build.rs 扫描 `use` 语句 + 编译期 warning | ❌ Cargo build script |

#### 现在的调用方式 vs 未来的调用方式

```rust
// —— 现在：直接 import 机制层的具体类型 ——
use crate::operations::{OPERATION_CATALOG, operation_by_id, OperationSpec};

fn on_new_tab(&mut self) {
    operation_by_id("create_tab").execute(params);  // 直接函数调用
}

// —— 未来：只 import 契约层的 trait ——
use crate::protocol::TerminalHost;  // ← 仍然是 Rust 原生 use

fn on_new_tab(&mut self, host: &mut dyn TerminalHost) {
    host.create_tab(spec);  // trait 方法调用，仍然是 Rust 原生
}
```

**机制没变——还是 `use` + 方法调用。变的是"use 了什么"：从具体类型变成 trait。**

#### Capability-Scope 和 Event Spine 听起来像新机制？

| 手段 | 听起来像 | 实际是 |
|------|---------|--------|
| Capability-Scope | 权限系统 / 沙箱 | 一个 **build.rs 脚本**：扫描 `use crate::` 语句，和声明文件对比，不匹配发 warning。零运行时成本。 |
| Event Spine | 消息中间件 | 一个 **in-process channel**：模块 emit 普通 enum，订阅者收到后执行。和标准库 channel 一样，只是约定"跨层通信走 channel 而非直接调用"。 |

两个都是**约定 + 轻量工具**，不是新的基础设施。

**一句话：现在用 Rust 原生 `use crate::`，未来仍然用 Rust 原生 `use crate::`。变的是 use 的
目标从"具体类型"变成"trait"，外加一份 build.rs 脚本防止 use 错方向。不发明任何新耦合机制。**
Rust 的 trait + workspace crate + build.rs 就是全部需要的工具——它们已经是 Rust 语言的一部分。

### 4.7 沉淀困境：为什么模块总是无法好好沉淀

#### 症状

> "模块们总是因为进化而没法好好沉淀，开发速度也极为缓慢"

这不是 agenterm 独有的困境——这是**所有宏内核式单 crate** 的通病。根因在于：**没有不可变层，
所以没有东西可以沉淀。**

#### 根因分析

在一个 60+ 模块全 `pub mod` 互相可见的单 crate 里，每一次进化都会**动摇所有模块**：

```
你想改一个 UI 交互
  → 碰 frontend/mod.rs
    → 但 frontend 被 adapter 反向依赖（74 条 use）
      → 所以 adapter 也要改
        → 但 adapter 是机制层，改它要重编译整个 platform
          → 重编译慢
            → 一次改动牵连整个 crate
              → 没有任何模块是"安全的、不用动的"
```

这就像在流沙上盖楼——**每一层都不是硬地基，所以每一层都在动，每一层都无法沉淀**。

| 症状 | 宏内核根因 | 三层温度架构如何解 |
|------|-----------|-------------------|
| 改一个 UI 交互牵连底层重编译 | 所有模块在一个编译单元 | hot 层改动不触发 cold/frozen 重编译 |
| adapter 改了要回头改 frontend | 反向依赖（74 条 use） | 方向纠正后，adapter 变更不扩散到 hot 层 |
| 不确定哪些模块"已经稳定" | 没有温度标记——所有模块都在同一温度 | frozen 层标记为"做对后不动"；cold 层标记为"极少变"；hot 层才允许频繁改 |
| 想加新功能但怕碰坏旧的 | 没有层间契约——碰任何东西都可能影响任何东西 | trait 契约是防火墙——只要不改 trait 签名，底层变动不影响上层 |
| 开发速度缓慢 | 每次改动编译范围太大 + 回归测试范围太大 | 按层编译 + 按层测试——hot 层改动只测 hot 层 |

#### 沉淀的定义

一个模块"沉淀"了，意味着：

1. **它的接口不再变**——上游可以依赖它而不用担心。
2. **它的实现不再被频繁触碰**——因为上层有稳定接口，不需要回头改它。
3. **它的测试可以冻结**——不再因为别的模块的改动而需要更新。

在当前的宏内核结构里，**没有一个模块满足这三条**——因为 `pub mod` 全局可见 + 反向依赖 +
单编译单元，所有模块随时可能被任何人的任何改动波及。

#### 三层温度如何让沉淀发生

```
frozen 层沉淀条件：做对一次 + 契约层 trait 定稿
  → 此后上层变更永远不碰它
  → 它的测试可以冻结
  → 它是真正的硬地基

cold 层沉淀条件：trait 设计稳定（每月/每季才动一次）
  → trait 定稿后，上层和下层都可以独立进化
  → trait 本身是"沉淀下来的契约"

hot 层永远不沉淀（也不需要沉淀）：
  → UI/UX 进化发生在这里
  → 但因为不影响下面两层，hot 层的频繁变动是安全的
```

**关键洞察：frozen 和 cold 层之所以能沉淀，恰恰是因为 hot 层被允许不沉淀。** 把频繁变更
锁在 hot 层，frozen/cold 层才能安静下来。现在的结构是所有层都在动——因为所有层物理上
在同一个编译单元里，频繁变更无处可隔离。

#### 这与开发速度的关系

```
当前：  改一行 → 重编 60 个模块 → 跑全量测试 → 慢
        ↓
未来：  改 hot 层一行 → 只重编 hot 层 → 只跑 hot 层测试 → 快
        改 cold 层（trait）→ 重编 cold + hot → 跑两层测试 → 中等（但 trait 很少改）
        改 frozen 层 → 几乎不发生（做对后不动）
```

开发速度提升不来自"写代码更快"，来自**每次改动波及的范围变小**——编译范围、测试范围、
回归范围都只限于被改动的温度层。这是物理隔离的直接工程回报。

---

## 5. 四个手段（服务于一个哲学）

rev1 把这四个当成平行可选方向。rev2 重新定位：**它们是同一个哲学的四个实现手段，有明确优先级。**

### 5.1 手段全景

| 手段 | 角色 | 对野心的贡献 | 优先级 |
|------|------|-------------|--------|
| **C：Trait as ABI** | 契约层的形态 | 定义契约层的形态——没有 trait，就没有"无视彼此存在"的分层 | **P0** |
| **A：Event Spine** | 契约层的通信 | 让应用层和机制层通过消息通信，而非函数调用——这是"动态适应"的前提 | **P1** |
| **B：Capability-Scope** | 层间护栏 | 编译期约束层间不越界——这是"稳健"的护栏 | **P2** |
| **D：双模编译** | 迁移安全网 | 迁移安全网——不是野心本身，是工具 | **P3** |

### 5.2 手段 C（P0）：Trait as ABI —— 契约层的形态

**内核类比**：Windows HAL——内核不直接调硬件，通过函数指针表间接调用。换 HAL = 换硬件支持。

**agenterm 翻译**：跨层调用从直接 import 具体类型改为通过 trait 间接调用。

```rust
// 契约层（crates/agenterm-protocol/ 或 src/protocol.rs）里定义 trait = 层间合同
pub trait TerminalHost: Send {
    fn create_tab(&mut self, spec: TabSpec) -> Receipt<TabId>;
    fn observe(&mut self, tab: TabId) -> EventStream;
    fn control(&mut self, tab: TabId, cmd: ControlCmd) -> Receipt<()>;
}

// 应用层只依赖 trait，不知道机制层的具体类型
fn on_new_tab(&mut self, host: &mut dyn TerminalHost) {
    let receipt = host.create_tab(Default::default());
    // receipt 携带确定性证据（序列号、时间戳），不是 fire-and-forget
}
```

**已有先例**：`ScriptEngineBackend` trait（`src/script_engine.rs`）已证明——四引擎通过一个
trait 统一，主系统只依赖 trait。`design-script-engine-trait.md` 记录了 Trait-M1～M4 全部落地。

**收益**：
1. trait 是层间的 syscall table——换实现 = 换 HAL，上层零修改。
2. trait 放独立 crate → 天然拆分边界 → Cargo crate 级 fingerprint 隔离。
3. Receipt 机制天然嵌入——与"验证而非 sleep"产品理念一致。

**性能边界**：跨层调用不是热路径（热路径是 PTY→vt100→render 数据流，走直接调用不走 trait）。
可用 `impl Trait` 泛型编译期单态化，零运行时成本。

### 5.3 手段 A（P1）：Event Journal Spine —— 契约层的通信

**内核类比**：seL4 的 IPC 全部经 endpoint，没有捷径；Linux ftrace 插在子系统之间。

**agenterm 翻译**：把 `event_journal` 从"事后审计"升级为"事前通信脊柱"。

```
当前：  caller → fn call → 执行者 → event_journal.record(已发生的事)

升级：  caller → emit Intent ──┐
                              ├→ EventSpine ──→ event_journal（确定性记录）
                              │              ──→ 执行者 subscriber
                              │              ──→ ui subscriber（刷新视图）
                              ←── emit Effect ──
```

**具体形态**：在 `protocol.rs` 或独立 `intent.rs` 加 `Intent` / `Effect` 类型化消息。

**收益**：
1. 集成测试 = 回放 journal——喂 Intent 序列验证 Effect 序列，不需要实时编排。
2. 模块解耦是自然结果——不直接依赖，只有共享的消息词汇表。
3. 可观测性免费——所有跨层通信自动有 trace。
4. MCP / 脚本引擎已走此路——只是让内部模块也用同样方式。

**边界**：spine 只承接控制流（低频、typed、可审计）。PTY→vt100→render 是数据流
（高频、批量、零延迟容忍），保持直接调用，不走 spine。

### 5.4 手段 B（P2）：Capability-Scope —— 层间护栏

**内核类比**：seL4 的 capability——不是"能不能调用"，而是"有没有 cap token"。

**agenterm 翻译**：每个模块声明它需要什么能力，未声明的 `use crate::xxx` → 编译期 deny。

```rust
// src/frontend/mod.rs
mod frontend {
    needs {
        TerminalHost: [observe, control],     // 只依赖契约层 trait
        EventJournal: [append, subscribe],
        UiGeometry: [compute],
    }
    provides {
        UiSnapshot: [generate],
        UiAction: [dispatch],
    }
}
```

**三层渐进路径**：
- **Lv0（只读分析）**：脚本扫描 `use crate::`，产出真实依赖矩阵。零代码改动，是拆分决策的武器。
- **Lv1（build.rs 约束）**：扫描 `capabilities.toml`，未授权依赖发编译期 warning。
- **Lv2（编译期 deny）**：warning 升级为 error。

**与现有约束互补**：`boundary_tests.rs` 管 crate 间边界（platform↔product）；capability-scope
管 crate 内模块间边界。`ui_action_catalog.rs` 管 action 的 host parity；capability-scope 管
module 的依赖 parity。

### 5.5 手段 D（P3）：双模编译 —— 迁移安全网

**内核类比**：Unikernel——开发时有模块化抽象，部署时编译器内联特化成单镜像，零运行时开销。

**agenterm 翻译**：同一源码树两种编译模式——开发态 monolith（快），交付态 modular（可拆）。

```toml
[features]
default = ["monolith"]
modular = ["dep:agenterm-frontend", "dep:agenterm-control", ...]
```

**收益**：零迁移成本；CI 跑两种模式检测漂移；把"要不要拆 crate"从宗教争论变成实验。

**与 v0.1.18 的关系**： Portable App Substrate 的 base PE = monolith 编译；`.agp` App Pack =
modular 的终极形态（应用层完全脱离编译期）。

---

## 6. 与 v0.1.18 Portable App Substrate 的关系

v0.1.18 的 App Pack 方案和本文的三层温度架构**完全同构**——只是视角不同：

| v0.1.18 视角 | 本文视角 | 共同点 |
|-------------|---------|--------|
| Base PE（不可变，低频发布） | frozen + cold 层 | 编译进 PE，极少变更 |
| `.agp` App Pack（可热替换，高频发布） | hot 层 | 不编译进 PE，运行时加载 |
| Host ABI v1（Base 暴露给 App 的接口） | 契约层 trait | 唯一的层间耦合面 |
| QJS 作为 v1 App Engine | hot 层的脚本载体 | 应用层用脚本定义行为 |

**本文的主张**：v0.1.18 不应只被当作发布/分发策略，而应被提升为**整个内部架构的分层哲学**。
具体而言——

1. v0.1.18 的 Host ABI v1 **就是**契约层的第一个产品化实例。
2. v0.1.18 的 `.agp` App Pack **就是**应用层 hot 区的载体。
3. v0.1.18 的 Base PE **就是**frozen 层 + cold 层的编译态组合。

**如果 v0.1.18 成功，它证明的不只是"可以热更新应用包"，而是"三层温度架构在 agenterm 里可行"。**

---

## 7. 与现有架构债务的对应

| `ARCHITECTURE.md` 债务 | 对应手段 | 说明 |
|------------------------|---------|------|
| **L2**（双前端巨石进行中） | C + B | trait 提取 frontend 共享核（`design-frontend-shared-core.md` 五大候选的天然形态）；capability-scope 防止新双写漂移 |
| **L3**（`platform/mod.rs` 过肥半迁移） | C | `policy/*` 已拆出；trait 把剩余 `FrontendHost` facade 收敛为契约层 trait |
| **L4**（结构 SSOT 未机读双向） | B | capability-scope Lv1/Lv2 = "代码→文档围栏"的机读化 |

---

## 8. 多 agent 并发的直接收益

当前 codex（agenterm-con 汇编/FFI）和 cc（v0.1.16 CI 发布链）的领地已天然隔离（con 是独立
workspace member 只依赖 platform；CI 碰的是 scripts/rh/）。瓶颈不在当前两个 agent，而在于
**主 crate `src/` 的 60+ 模块在一个编译单元**。

三层温度架构落地后：

- **frozen 层**（platform + engine）已经是独立 crate → Cargo fingerprint 天然隔离 ✅
- **cold 层**提取为独立 `agenterm-protocol` crate → trait 定义和 DTO 独立编译
- **hot 层**的应用逻辑拆成域 crate（frontend/control/script）→ 每个 agent 持有一个域 crate
- **`.agp` App Pack** → UI/UX 实验完全脱离编译期，甚至脱离 agent checkout

多 agent 并发编译从"靠纪律避免 target 目录竞争"变成"Cargo 依赖图自动提供物理隔离"。
**协同改进和债务收敛是同一个动作。**

### 8.1 深度技术工作与耦合改良的互补关系

当前 codex 在 agenterm-con 死磕汇编 / FFI / 原生系统能力，探索技术底层、积累深厚技术底蕴。
这与耦合改良**不仅不冲突，而且是同一件事的两面**。

#### 对应关系

| 层 | 谁在干 | 在干什么 |
|---|---|---|
| **frozen**（机制层） | codex | 死磕汇编 / FFI / ConPTY / vt100 parser，把底层做到极致 |
| **cold**（契约层） | 待建设 | trait / protocol 定义层间合同 |
| **hot**（应用层） | 待解放 | UI/UX 快速迭代进化 |

codex 在 con 的深度工作**就是 frozen 层的建设**——而且他已经在一个独立 workspace member
（`crates/agenterm-con`）里做，完全不碰主 crate 的耦合问题。

#### 互相成就

```
耦合改良做的事：让 frozen 层可以真正"冻结"（上层变更不波及底层）
codex 做的事：   让 frozen 层值得冻结（做到极致，做对一次就不用再动）
```

单独做任何一件都不完整：

| 只有 codex 的深度工作，没有耦合改良 | 只有耦合改良，没有 codex 的深度工作 |
|---|---|
| 底层做到极致了，但上面 74 条反向依赖会反复打扰它——每次上层改 UI 都可能波及底层，"冻结"无从谈起 | 依赖方向对了、trait 定义好了，但 frozen 层本身不够硬——ConPTY 处理不完整、FFI 有 bug、vt100 parser 有漏洞——地基是软的 |

两件事合在一起才闭环：

1. codex 把 frozen 层做到极致 → **值得冻结**
2. 耦合改良让 hot 层变更不波及 frozen → **可以冻结**
3. frozen 真正冻结后 → cold 层有了稳定的实现基础，trait 契约才能定稿
4. trait 契约定稿后 → hot 层可以肆无忌惮地快速迭代进化
5. hot 层快速进化 → 产品价值快速释放 → 反哺更多资源投入 frozen 层深度

这是一个**正反馈循环**，codex 和耦合改良分别推动循环的两半。

#### 比喻

盖高楼：codex 在打地基（汇编 / FFI = 往下挖到岩层），耦合改良在修抗震结构（分层 + trait =
楼层之间的隔震层）。地基打得再深，没有抗震结构，上面每次装修都会震到地基——地基没法稳定。
抗震结构做得再好，地基是软的——楼还是会塌。

两件事完全互补，而且有**严格的先后关系**：地基要先打到足够硬（codex 正在做），抗震结构才
能往上修（耦合改良），最终上面每一层才能独立装修（hot 层快速进化）。

#### 当前分工的战略意义

当前的多 agent 分工不是临时凑合，而是**恰好对应了三层温度架构的建设方向**：
- codex → frozen 层建设（深度技术底蕴）
- cc → 交付链疏通（让价值流出去）
- 耦合改良 → cold 层建设（让 frozen 可以冻结、hot 可以解放）

要做的只是：等耦合改良落地后，把 codex 的工作正式标记为"frozen——做对后不再动"，让上层
变更永远不再打扰他。

---

## 9. 非目标（明确不做）

- **不做运行时 sandbox / 权限隔离**。AGENTS.md 明确 agent 权限/审批/沙箱是上层 harness 职责。
  capability-scope 是**编译期依赖声明**，不是运行时权限。
- **不替代 `ARCHITECTURE.md` 的 SSOT 地位**。本文是概念提案，不是第二份结构 SSOT。
- **不要求一次性全量重构**。手段 D 的核心价值就是"零迁移成本的渐进路径"。
- **不改 `boundary_tests.rs` 现有红线**。手段 B 补充 crate 内模块间约束，不改 crate 间约束。
- **不把 self-hosting 当 vanity**。self-hosting 是有工程回报的 dogfood，不是自恋。

---

## 10. 后续动作建议（非排期，按优先级）

| 优先级 | 动作 | 成本 | 产出 |
|--------|------|------|------|
| **进行中** | **codex 在 agenterm-con 的 frozen 层深度建设**：汇编 / FFI / 原生系统能力 | 已在进行 | frozen 层值得冻结的技术底蕴 |
| **进行中** | **cc 疏通 v0.1.16 发布链**：让已做出来的价值流出去 | 已在进行 | 解锁后续所有版本计划 |
| **P0-a** | **B-Lv0 只读依赖分析**：扫描 `src/` 每个模块的 `use crate::`，产出真实依赖矩阵 + 可视化 | 零代码改动 | 所有后续决策的输入；主 crate 拆分路线图 |
| **P0-b** | **阶段 1 消除反向依赖**：产品语义从 adapter 搬回 frontend；74 条反向 use 归零 | 搬代码，不加 trait | L2 漂移根源消除；依赖方向确立 |
| **P1** | **阶段 2 trait 契约**：选一个域（control plane 或 UI）提取事实标准为 trait | 一个域的 trait 提取 | 验证 trait-as-ABI 的迁移成本和收益 |
| **P2** | **A spine 原型**：在 event_journal 之上加 Intent/Effect 类型，先只用于集成测试 | 加类型 + 测试 fixture | 最低风险的 spine 引入方式 |
| **P3** | **B-Lv1 build.rs 约束**：基于 P0-a 的依赖矩阵生成 capability 声明，编译期 warning | build.rs 脚本 | 编译期层间越界检测 |
| **远期** | **阶段 3 + v0.1.18**：trait 独立 crate + 应用层 UI 语义变 `.agp` 包 | v0.1.18 已有规划 | self-hosting 架构的第一个产品化证明 |
