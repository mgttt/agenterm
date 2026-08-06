# Rhai ↔ Rust 封装边界契约

| 字段 | 值 |
|------|-----|
| **文档** | Native 内核与 Rhai App Pack 之间 **清晰、严格、可证明** 的边界 SSOT |
| **日期** | 2026-08-06 |
| **状态** | 设计稿 rev1 |
| **受众** | 产品、Script 运行时、GUI/CC、发布/证据 |
| **关联** | `plan/agenterm-rhai-app.md`、`plan/ARCHITECTURE.md`、`docs/agenterm-rhai-runtime.md`、`prd/PRD_02_10_rhai_scripting.md`、`AGENTS.md` |

---

## 1. 一句话

**终端与 Fleet 内核永远在 Rust；Rhai 只通过 catalog 登记的、粗粒度、有界、可 receipt 的 Facade 调用能力。**  
边界必须 **可静态描述、可 catalog 审计、可黑盒证明**——不是约定俗成。

---

## 2. 三层模型

```text
┌─────────────────────────────────────────────────────────────┐
│  L3  Rhai App Pack / 用户脚本 / Logic Pack                  │
│      产品语义、路由、文案、Hub 策略、编排                     │
│      禁止：实现内核；禁止：per-cell/per-byte 热循环          │
└───────────────────────────┬─────────────────────────────────┘
                            │ 仅经 Catalog Facade（可证明）
┌───────────────────────────▼─────────────────────────────────┐
│  L2  Rust Facade（Script API / product.* / fleet.* / llm.*）│
│      粗粒度操作、预算、typed error、receipt、availability    │
└───────────────────────────┬─────────────────────────────────┘
                            │ 内部调用；不对脚本暴露
┌───────────────────────────▼─────────────────────────────────┐
│  L1  Kernel & Mechanism（永不导出给 Rhai 实现）               │
│      server · PTY/ConPTY · parser · grid · blit · platform   │
└─────────────────────────────────────────────────────────────┘
```

| 层 | 变更节奏 | 热更 |
|----|----------|------|
| L1 | Base semver，Candidate 硬门 | ❌ |
| L2 | Base semver；catalog 增 API 需兼容范围 | ❌（仅增 surface） |
| L3 | App pack / 用户脚本 | ✅（pack 通道） |

---

## 3. 边界八条（规范）

### B1 — 内核不可实现

L1 模块 **不得** 在 Rhai 中重写或「策略插件化」到 per-byte/per-cell 控制流：

- `agenterm server` 权威、tab 树、journal、epoch
- PTY/ConPTY/POSIX 读写泵
- ANSI/parser、scrollback 存储、viewport 数学
- 帧 blit、字体 shaping、平台输入泵

**可证明：** pack 与 `agenterm.tasks.json` **不得** import 未 catalog 的 native 符号；L1 代码 **不在** Rhai 注册路径。

### B2 — 仅 Facade 出口

Rhai 触达 Fleet/终端/产品面的 **唯一** 合法路径是 Script API catalog 已登记条目（`docs/agenterm-rhai-runtime.md` + `script api --json`）。

**可证明：** `script_catalog` 与 `register_*` 漂移检测（见 `plan/precision-audit.md`）；未登记 = 不存在。

### B3 — 粗粒度

一次 Facade 调用 = **一个产品/自动化语义动作**，不是热循环一步：

| ✅ 允许 | ❌ 禁止 |
|---------|---------|
| `capture(8192)` | `feed_byte(b)` × N |
| `present_lines(vec![])` 整帧 | `get_cell(x,y)` × rows×cols |
| `tabs.set_note(id, note)` | 内层 while 扫全 scrollback |

**可证明：** catalog 条目文档含 **budget**（max bytes/ops/time）；Clippy/审查禁止暴露 L1 类型。

### B4 — 有界（Budget）

每个 Facade 必须声明并可测：

- `max_output_bytes` / `max_operations` / `timeout_ms`
- 集合深度、字符串长度、并发 Task 数（沿用 Script invocation 预算）

**可证明：** 黑盒超限 → 稳定 `limit` error；qualification 用例故意触发。

### B5 — 单一 Fleet 权威

Facade 读写作 **server 投影**；Rhai/pack **不得** 持久化 Fleet truth 副本作 live 源。

- 允许：pack 内 **会话草稿**（未提交 Intent）
- 禁止：pack 缓存 tab 树并 UI 不再 `inspect`/snapshot 校验

**可证明：** server epoch 变更后，pack 投影须失效或显式 `stale`；smoke 断言。

### B6 — Typed 失败与 Receipt

突变类 Facade **必须** 走 receipt/wait 契约（与 `fleet.*` 一致）；不得返回裸 `bool`。

**可证明：** 公共 smoke 覆盖 receipt + post-state；失败 reason 码稳定。

### B7 — 授权不在 Rhai profile

配额、批准、出站 allowlist、凭据 **在 L2 native 或 Agent harness**，不在 pack 逻辑里「假装 sandbox」。

- catalog `capability` = **发现/兼容**，不是 grant
- pack 可调 `llm.http_forward(handle)`；**不能** 自造 handle 读 key

**可证明：** AGENTS.md 审计项；渗透测试：pack 无法 exfil 凭据明文。

### B8 — Fallback 可证

嵌入 pack 的产品路径：**pack 失败 → Rust 等价路径**，用户可见行为不劣化。

**可证明：** feature flag off / pack corrupt → snapshot/PNG 与纯 Rust 基线一致（Strangler 期）。

---

## 4. Facade 分级（Tier）

新增 API 必须标 Tier；**Tier 0–1 进 pack 热路径需主控批准**。

| Tier | 名称 | 示例 | pack 热路径 |
|------|------|------|-------------|
| **T0** | Kernel-adjacent | （不暴露） | ❌ 禁止登记 |
| **T1** | Fleet observe | `fleet.ui.snapshot`, `terminal().capture` | ⚠️ 仅低频 |
| **T2** | Fleet mutate | `tabs.set_note`, `ui.tabs.show` | ✅ 按需 |
| **T3** | Product present | `product.cc.footer_line`, `present_lines` | ✅ CC 主用途 |
| **T4** | Pack meta | `pack.version`, `pack.reload` | ✅ loader |
| **T5** | Sidecar | `llm.*`（gateway 进程） | ✅ gateway pack |

**规则：** CC 帧循环 **仅 T3+T4**；**禁止 T1 每帧调用**（应用 B3）。

---

## 5. 可证明性清单（Evidence）

| 证据类型 | 证明什么 | 所有者 |
|----------|----------|--------|
| **Catalog ↔ 注册一致** | 无幽灵 API、无漏登记 | `script_catalog` 测试 / metadata gate |
| **`script api --json`** | 文档 = 运行时 = PRD | Rhai 模块 |
| **Boundary lint** | `src/platform/boundary_tests` 扩展：Facade 不 import 错层 | platform |
| **Budget 黑盒** | 超限 typed error | script-smoke |
| **Epoch stale** | server 重启后 pack 不冒充 live | control-center / fleet smoke |
| **Fallback parity** | pack off ≡ Rust 基线 | PNG + snapshot diff |
| **Release receipt** | L1 随 Base seal；L3 pack 独立 hash | Candidate / pack manifest |

新增 Facade **必须** 在合并前指明上表至少 **两行** 证据归属。

---

## 6. 与现有资产对齐

| 已有 | 边界角色 |
|------|----------|
| `fleet.*` | T1/T2 Facade；automation SSOT |
| `std.*` / `rhai::http` | 本地/网络 Facade；非 Fleet 权威 |
| `agenterm.tasks.json` | **开发 task** manifest；≠ product pack |
| `gateway.manifest.json` | L3 pack；仅 T5 |
| `src/platform/boundary_tests.rs` | L1/L2 静态闸 |
| `design-control-center-ux.md` | T3 `present_lines` 几何契约 |

---

## 7. Strangler 迁移时的边界纪律

从 Rust 迁到 pack 的 **每一 PR** 必须：

1. 标 Tier（通常 T3）
2. 保留 Rust fallback（B8）
3. 不引入 T0/T1 热路径调用
4. 不复制 Fleet 状态（B5）
5. 更新 catalog + 一条 smoke

**禁止：** 「先迁再补边界」；「pack 里临时 `std::process` 起 PTY」。

---

## 8. 开放问题（BD-*）

| ID | 问题 | 建议 |
|----|------|------|
| BD-1 | `product.*` 与 `fleet.*` 是否同一 Engine | 同一 catalog；`product` 前缀表 CC 嵌入 |
| BD-2 | in-process CC vs broker `fleet` | CC 热路径 **in-process Facade**；broker 保留给 CLI/外部脚本 |
| BD-3 | Tier 违规 CI | catalog schema 增 `tier` 字段 + lint |
| BD-4 | pack 静态分析 | `script check` 拒绝 catalog 外调用（尽力；动态 eval 除外） |

---

## 9. 交叉引用

- App Pack 总方案：`plan/agenterm-rhai-app.md`
- 架构三层：`plan/ARCHITECTURE.md` §1.0
- Script 契约：`prd/PRD_02_10_rhai_scripting.md`
- Runtime 树：`docs/agenterm-rhai-runtime.md`
- Agent 纪律：`AGENTS.md`（unrestricted runtime ≠ 无边界 Facade）

---

## 10. 摘要（给评审用）

| 问题 | 答案 |
|------|------|
| 内核能封给 Rhai 调吗？ | **能**，经 Facade |
| 内核能放 Rhai 里吗？ | **不能**（B1） |
| 关键是什么？ | **边界清晰、严格、可证明**（本文 B1–B8 + Tier + Evidence） |
| JIT 会改变吗？ | 只放大 T2–T3 可迁范围，不改 B1/B5/B7 |
