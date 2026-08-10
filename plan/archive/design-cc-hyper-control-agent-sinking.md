# ⚠️ 已归档：超控智能体 v7 下沉分析

> **归档于 2026-08-10。** 接受的 ownership 边界与分期能力已经进入
> `plan/design-cc-hyper-control-agent.md` 和 `prd/PRD_02_21_control_center.md`。
> 本文是历史 feeder analysis，不是实现计划。

# 超控智能体 v7 — 下沉分析（历史）

> 2026-08-09。哪些探索成果可以脱离「超控智能体」这一特定 UI 面，下沉为 agenterm 的通用基础设施。

---

## 1. 可下沉能力总览

```
超控智能体 v7 的组成          可下沉为                落点
─────────────────────        ──────────────         ──────────────────
规则引擎（事件→规则→动作）     通用事件匹配 + 回调        agenterm-rh provider
                                                     或 agenterm server 事件层

Pipeline 步骤序列             结构化 Task 执行器        agenterm-rh task 系统
                                                     扩展现有 task run

Task 数据模型                 JSON schema + 存储约定    agenterm-rh / data 层
                                                     统一 task/run 数据格式

Fleet 事件流消费              事件过滤 + 条件等待        agenterm server
                                                     扩展现有 Observable Fleet

LLM 节制调用                  token 预算 + 调用闸门     agenterm-llm-gateway
                                                     已有规划，对齐即可

意图路由                      关键词匹配 + 命令分发      CC 本地（超控特有）
                                                     不做通用层——太特定
```

---

## 2. 逐项分析

### 2.1 规则引擎 → agenterm-rh `rules` provider

**价值：** 规则引擎不只超控智能体需要。任何自动化场景都需要「当 X 发生时自动做 Y」。
放在 agenterm-rh 中，脚本可以直接定义和注册规则。

**已有基础：**
- agenterm-rh 已有 `task` provider（`task run` / `task list`）
- Observable Fleet 已有事件流（epoch/sequence/journal）
- agenterm-cli 已有 `wait-pane` / `wait-ui`（单次条件等待）

**缺口：**
- 没有「持续监听 + 规则匹配 + 自动触发」的机制
- `wait-*` 是一次性的，不是持久的

**建议下沉物：**

```rust
// agenterm-rh 新增 rules provider
// scripts/rules/watch.rh：

rule("build-failure") {
    when: fleet.tab_output_match(tab: "@3", pattern: "error:"),
    then: fleet.notify("Build 失败: {match}"),
    cooldown: 60s,
};

rule("task-timeout") {
    when: task.idle_timeout(task: "review PR #42", timeout: 30min),
    then: task.pause("review PR #42"),
};
```

**落点：** `agenterm-rh` 新增 `rules` provider（或扩展现有 `task` provider）。
规则定义存储在 `~/.local/share/agenterm/rules/`。
执行在 agenterm-rh framed-worker 中（持久化、可重启）。

### 2.2 Pipeline 步骤序列 → agenterm-rh task 系统

**价值：** 现有 `agenterm-rh task run` 执行单个任务。Pipeline 把多个任务串成序列（带步骤依赖、条件分支、失败处理）。这是 task 系统的自然演进。

**已有基础：**
- `agenterm-rh task run <name>` 已支持单个 task 执行
- `agenterm.tasks.json` 已支持 task 清单

**缺口：**
- 没有步骤序列概念
- 没有步骤间状态传递
- 没有半自动步骤（等待人类确认）

**建议下沉物：**

```json
// pipelines/review-pr.pipeline.json
{
  "name": "review PR",
  "steps": [
    { "name": "看 diff",      "type": "manual" },
    { "name": "跑测试",       "type": "task",   "task": "cargo-test-pr" },
    { "name": "写评论",       "type": "manual" },
    { "name": "提交 review",  "type": "manual" }
  ]
}
```

```bash
agenterm-rh pipeline run review-pr --param pr=42
  → 创建 Task 实例
  → 步骤 1: 等待人类确认
  → 步骤 2: 执行 cargo-test-pr task
  → 步骤 3: 等待人类确认
  → 步骤 4: 等待人类确认
  → Task 完成
```

**落点：** agenterm-rh 新增 `pipeline` provider。
Pipeline 定义 = `~/.local/share/agenterm/pipelines/`。
Task 实例 = `~/.local/share/agenterm/tasks/YYYY-MM-DD/`。

### 2.3 Fleet 事件匹配 → agenterm server Observable Fleet 上层

**价值：** 当前 Observable Fleet 提供了底层的 epoch/sequence/journal，但消费端需要自己实现事件过滤和条件匹配。一个更上层的「事件匹配 + 回调」API 对规则引擎和所有自动化场景都有用。

**已有基础：**
- `agenterm-cli wait-pane` / `wait-ui`（单次条件等待）
- MCP `agenterm_wait` tool（事件谓词等待）
- Observable Fleet journal

**缺口：**
- 没有持久的事件订阅（一次注册，多次触发）
- 没有输出模式匹配（regex on tab output）
- 没有复合条件（AND/OR/时序）

**建议下沉物：**

```bash
# 订阅 tab @3 的输出匹配 "FAILED"，最多触发 3 次
agenterm-cli watch \
  --tab @3 \
  --pattern "FAILED" \
  --max-fires 3 \
  --then "agenterm-rh task run notify-failure"
```

**落点：** agenterm server 新增 `watch` 能力，或 agenterm-rh 通过现有的 `wait-*` 循环实现。
server 侧实现更高效（事件在 server 进程内匹配，无需轮询）。

### 2.4 LLM token 预算 → agenterm-llm-gateway

**价值：** 所有 LLM 调用方（超控智能体、Composer、Script、MCP）都需要 token 预算控制。放 gateway 层统一管理，避免每个调用方自己实现。

**已有基础：**
- `plan/design-llm-bridge-web-to-api.md` 已规划 LLM gateway
- gateway 已规划配额/审计/熔断

**缺口：**
- 没有 per-session / per-consumer 的 token 预算
- 没有用户可见的 token 计数器

**建议下沉物：** 在 gateway 中新增 `X-Token-Budget` header 和 `/v1/usage` endpoint。
CC 和其他 consumer 通过标准 API 查询和管理预算。不放在 CC 逻辑中。

### 2.5 意图路由 → CC 本地（不下沉）

**价值：** 意图路由（"开始 review PR #42" → 创建 Task）是超控智能体的 UI 交互逻辑，不是通用能力。保持 CC 本地。

---

## 3. 下沉优先级

| 优先级 | 下沉物 | 落点 | 理由 |
|--------|--------|------|------|
| **P0** | Pipeline 步骤序列 | agenterm-rh `pipeline` provider | 直接解锁 v7 Phase A 的核心能力；task 系统自然扩展 |
| **P1** | 规则引擎基础 | agenterm-rh `rules` provider | 自驱的核心；不止超控用，所有自动化场景都需要 |
| **P2** | Fleet 事件匹配 | agenterm server watch API | 让规则引擎更高效；减少轮询 |
| **P3** | Token 预算 | agenterm-llm-gateway | 等 gateway 就绪后对齐 |

---

## 4. 不做的事（明确边界）

| 能力 | 归属 |
|------|------|
| BPMN 可视化（Sugiyama WASM） | CC 超控智能体（UI 层，不下沉） |
| Pipeline 设计器（拖放编辑） | CC Workflows tab（UI 层，不下沉） |
| 任务列表渲染 | CC `hyper_control` view（UI 层，不下沉） |
| 意图路由（自然语言 → Pipeline/Task） | CC 本地逻辑（UI 层，不下沉） |
