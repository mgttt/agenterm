# ⚠️ 已归档：agenterm 产品设计 Review

> **归档于 2026-08-10。** Task/Pipeline/rules 结论已由
> `plan/design-cc-hyper-control-agent.md` 与 `prd/PRD_02_21_control_center.md` 吸收。
> 本文只保留决策背景，不是活跃产品计划。

# agenterm 产品设计 Review（历史分析）

> 2026-08-09。基于 `src/` + `crates/` + `plan/ARCHITECTURE.md` 的完整阅读。
> 角度：产品设计，非代码质量。问的是「这个产品现在是什么，缺什么，往哪走」。

---

## 1. 现在是什么：一个设计良好的终端 + 控制面

### 1.1 分层干净

三层边界（ARCHITECTURE.md §1.0）在代码里真实成立：

```
crates/agenterm-platform    纯 OS 机制，零产品耦合。boundary_tests 可以证明。
src/frontend/*              产品语义（action、dialog、gesture、snapshot 字段）
src/platform/adapters/*     怎么画、收事件（Win/Unix 两套 adapter）
```

这不是「设计文档写了但代码没跟」。`boundary_tests.rs` 真的在跑、真的在拦截越界。**这是 agenterm 最值钱的架构资产。**

### 1.2 入口清晰

| 入口 | 角色 | 设计评价 |
|------|------|---------|
| `agenterm server` | Fleet 权威（PTY、tab、event journal） | 职责单一，好 |
| `agenterm-cli` | 控制面（mux、mcp、cc 生命周期） | CLI 统一入口，正确 |
| `agenterm-cc` | 独立投影进程 | 隔离干净，文档明确「不拥有 PTY」 |
| `agenterm-rh` | 脚本运行时 | 多引擎（rh/lua/qjs），framed worker 模式好 |

每个进程都知道自己不该做什么。CC 的 `HELP` 字符串甚至写死了「never owns terminal, PTY, workspace, server」。

### 1.3 脚本/Worker 系统是强力地基

`WorkerSupervisor` + `ScriptTask` + `ScriptInvocation` 已经提供了一个**受监督的、有预算的、可取消的、可 broker 回调的** task 执行模型。这正是超控智能体的 Pipeline 执行器所需要的——不需要重写，只需要扩展。

### 1.4 Observable Fleet 已就位

`EventJournal` 29 种事件，epoch/sequence 跟踪，MCP `agenterm_wait` 已实现事件等待。这是规则引擎**事件监听**的基础设施——已经在生产代码里，不是计划。

---

## 2. 缺什么：从「终端控制面」到「工作组织者」的三块拼图

### 2.1 持久化 Task/Pipeline 层（缺）

**现状：** `ScriptTask` 是内存中的、一次性的（worker 进程结束就没了）。没有「昨天创建的 Task，今天恢复」的概念。

**需要：**
- `~/.local/share/agenterm/tasks/` 目录下的 JSON 持久化
- Task 有 stable ID、创建时间、状态、关联 tab、步骤进度
- Pipeline 定义存储（`~/.local/share/agenterm/pipelines/`）
- 与 Fleet 事件关联（tab 创建/关闭时更新 Task）

**落点：** `agenterm-rh` 新增 `pipeline` provider，或独立 `agenterm-task` 模块。

### 2.2 事件→规则→动作 引擎（缺）

**现状：** `EventJournal` 有事件流，但消费端需要**自己实现**过滤、匹配、触发。`agenterm_wait` 是一次性的条件等待，不是持续监听。

**需要：**
- 持久的事件订阅（注册一次，多次触发）
- 模式匹配（tab 输出匹配 regex、tab 状态变化、空闲超时）
- 规则定义 DSL（或 JSON schema）
- 动作执行（通知、步骤推进、task 状态更新）

**落点：** 两种可能——
- A) `agenterm server` 新增 `watch` API（事件在 server 进程内匹配，零轮询）
- B) `agenterm-rh` 新增 `rules` provider（脚本注册规则，framed worker 持久运行）

B 更轻量（不增加 server 复杂度），但需要轮询。A 更高效，但需要 server 变更。**建议先 B 后 A**。

### 2.3 CC 的「智能」层（缺）

**现状：** `control_center.rs` 是一个纯投影主机——读取 server 快照、渲染、处理点击。没有任何「理解」或「推理」能力。

**需要：**
- `hyper_control` view（对话式 UI）
- 关键词路由（Phase A，不依赖 LLM）
- LLM 节制调用（Phase B，通过 LLM gateway）
- 卡片渲染（工作流进度、Fleet 摘要、审批请求）

**落点：** 纯 CC 本地逻辑。不需要新进程。依赖第 2.1 和第 2.2 的数据。

---

## 3. 三层之间的空白（架构机会）

当前架构有三层，但层与层之间的**抽象阶梯**不连贯：

```
现有（已建好）：
  agenterm-rh          ← 脚本执行引擎（worker、task、budget）
  agenterm server      ← Fleet 权威（PTY、tab、event journal）
  agenterm-cc          ← 投影（只读快照 + 点击路由）

缺少（中间层）：
  ???                  ← 持久化 task/pipeline 管理
  ???                  ← 事件→规则→动作 匹配引擎
  ???                  ← LLM 消费的 token 预算层

超控智能体 v7 的设计：
  CC 对话 UI
    → 规则引擎（事件匹配 + 触发器）
    → Pipeline 执行器（步骤序列 + 状态持久化）
    → agenterm-rh worker（每个步骤的实际执行）
    → agenterm server（tab 生命周期）
```

**这个空白不是 bug——是产品演进的正常阶段。** agenterm 先做好了底层（server、端、脚本），现在需要的是**中间组织层**。

---

## 4. 现有资产的再利用价值

| 现有资产 | 超控智能体怎么复用 |
|----------|-------------------|
| `WorkerSupervisor` | Pipeline 每一步作为一个 framed worker invocation |
| `ScriptTask`（Pending/Completed/Cancelled） | Task 状态模型直接映射到 Pipeline 步骤状态 |
| `EventJournal` + `agenterm_wait` | 规则引擎的事件源 |
| `control_center.rs` shell host | `hyper_control` view 的渲染基座（已支持 nav、pointer hit-test、snapshot） |
| `operations.rs` catalog | 语义操作 ID 体系（`control-center.open` 等），新 view 复用同一模式 |
| MCP `resources/list` + `tools/list` | 规则引擎可以向 MCP 客户端暴露 task/pipeline 状态 |

**结论：不需要推倒重来。** 超控智能体是建在现有地基上的**中间层**，不是替代任何现有模块。

---

## 5. 设计建议

### 5.1 保持分层纪律

超控智能体的新代码**不应**出现在 `crates/agenterm-platform`（那是纯机制的）。应该放在：
- `src/` 中新增 `task_manager.rs` / `pipeline.rs`（产品语义层）
- `agenterm-rh` 中新增 pipeline/rules provider（脚本引擎层）
- CC 中新增 `hyper_control` view（UI 层）

### 5.2 先建中间层，再建 UI

建议实现顺序：
1. **Task/Pipeline 持久化**（agenterm-rh 新增 provider）——不依赖 UI
2. **规则引擎基础**（agenterm-rh 新增 rules provider）——不依赖 UI
3. **CC hyper_control view**（Phase 0 空壳 + Phase A 关键词路由）——依赖 1、2
4. **LLM 节制调用**——依赖 LLM gateway 就绪

### 5.3 不需要新进程

现有进程已足够：
- `agenterm-rh` = Pipeline 执行器 + 规则引擎宿主
- `agenterm server` = Fleet 事件源 + tab 管理
- `agenterm-cc` = 对话 UI
- `agenterm-llm-gateway`（planned）= LLM 消费

超控智能体本身不需要独立进程——它是 CC 的一个 view，调用 agenterm-rh 的 provider。

### 5.4 不要过度设计 Pipeline

Pipeline 应该是**步骤清单**，不是 BPMN。原因：
- 现有基础设施（WorkerSupervisor）适合单步执行，不适合复杂分支/并行
- 用户日常工作不需要 BPMN 级别的编排
- 保持简单意味着更快交付、更少 bug

如果未来需要 BPMN，可以在 Pipeline 步骤中嵌入 `type: "sub-pipeline"` 引用另一个 Pipeline，实现组合——不增加核心复杂度。

---

## 6. 一句话总结

agenterm 现在是一个**设计良好的终端 + 控制面**。底层扎实、分层干净、边界有闸。

缺的是**中间组织层**——把散落的事件、tab、脚本组织成可管理的工作单元（Task/Pipeline），用一个轻量规则引擎驱动它们，用一个对话式 UI 暴露它们。

超控智能体 v7 的设计恰好填补了这个空白——而且大部分可以在现有地基上建造，不需要推倒重来。
