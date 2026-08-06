# Rhai ↔ Rust 封装边界契约

| 字段 | 值 |
|------|-----|
| **文档** | Native 内核与 Rhai App Pack 之间 **清晰、严格、可证明** 的边界 SSOT |
| **日期** | 2026-08-06 |
| **状态** | 设计稿 rev1 |
| **受众** | 产品、Script 运行时、GUI/CC、发布/证据 |
| **关联** | `plan/design-scripting-boundary-comparison.md`（**行业边界对照**）、`plan/agenterm-rhai-app.md`、`plan/ARCHITECTURE.md`、`docs/agenterm-rhai-runtime.md`、`prd/PRD_02_10_rhai_scripting.md`、`AGENTS.md` |

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

## 2.1 L1「内核」具体包括什么（清单 SSOT）

本文 **「内核」** = Rhai **不得实现或 per-byte/per-cell 驱动** 的 Rust 子系统。  
不等于「整个 repo」：许多 `src/frontend/*`、`src/control_center.rs` 产品语义 **将来可迁 pack**（L3），仍 **不是** 内核。

### 2.1.1 判定规则（三条，满足任一即内核）

1. **Fleet 权威**：持有或变更 workspace / tab 树 / PTY / journal / epoch 的 **唯一真相**。  
2. **实时机制**：字节级或 cell 级热路径（parser、grid、blit、PTY 泵、输入泵）。  
3. **平台机制**：OS 窗口/ConPTY/POSIX/IPC 传输的 **唯一 native 实现**（`agenterm-platform`）。

### 2.1.2 内核域 A — Fleet 权威（`agenterm server`）

| 子系统 | 职责 | 主要代码锚点 |
|--------|------|----------------|
| **Server 进程入口** | 无 HWND 权威进程；`server` 子命令 | `src/server_app.rs`, `src/bin/agenterm.rs` |
| **Tab / 树** | 稳定 tab ID、父子、晋升、环检测 | `src/tab_tree.rs` |
| **Workspace 持久化** | 保存/加载 workspace；路径布局 | `src/workspace.rs`, `src/platform/policy/workspace.rs`, `paths.rs` |
| **Working context** | 代理/归档控制上下文 | `src/working_context.rs` |
| **Control dispatch** | 公共 typed op 分发到 server 状态机 | `src/control_dispatch.rs`, `src/operations.rs`, `src/commands.rs` |
| **Control authority / contract** | 权威边界、协议字段 | `src/control_authority.rs`, `src/control_contract.rs` |
| **UI lease** | 单 live GUI 租约；relay 队列 | `src/ui_lease.rs`, `src/ui_command.rs` |
| **Settings 持久化（server 侧事实）** | 与 Fleet 交叉的设置项 | `src/settings.rs`（**存储/合并**；modal 文案可迁 pack） |
| **Instances 注册** | 逻辑实例发现 | `src/instances.rs` |

**PRD 归属：** Human workspace 中的 **server 侧**、Agent control plane、Executable family §server。

### 2.1.3 内核域 B — 协议、IPC、Journal

| 子系统 | 职责 | 主要代码锚点 |
|--------|------|----------------|
| **IPC 端点与传输** | loopback 帧、超时、多路 | `src/ipc_endpoint.rs`, `src/ipc_transport.rs`, `src/platform/ipc_transport_impl.rs`, `crates/agenterm-platform/.../ipc*` |
| **Protocol / hello / bootstrap** | 版本协商、epoch、sequence | `src/protocol.rs`, `src/client/mod.rs`（**客户端**）；server 侧在 server_app 树 |
| **Event journal** | 有序事件、gap/restart 检测 | `src/event_journal.rs` |
| **Operations / receipts** | 突变 receipt、wait、replay | `src/operations.rs`（与 dispatch 交界） |
| **Wake / upgrade identity** | 唤醒合并、升级身份 | `src/wake_signal.rs`, `src/upgrade_identity.rs`, `src/build_identity.rs` |

**Facade 出口（L2，非内核）：** `fleet.*` 经 `src/script_fleet.rs` + broker **调用** 上表，不 **实现** 上表。

### 2.1.4 内核域 C — PTY 与子进程

| 子系统 | 职责 | 主要代码锚点 |
|--------|------|----------------|
| **PTY 抽象** | ConPTY / POSIX master-slave | `src/pty.rs`, `crates/agenterm-platform/.../pty*` |
| **进程拉起 / 监控** | shell 子进程、remain-on-exit | `src/terminal_lifecycle.rs`, `platform` process spawn |
| **Worker supervisor** | Script worker 监管（非 Fleet PTY） | `src/worker_supervisor.rs` |

**禁止 pack：** `std::process` 间接 **替代** server 开 tab/PTY（自动化应走 `fleet` typed op，不得自造第二 PTY 权威）。

### 2.1.5 内核域 D — 终端运行时（parser / grid / scrollback）

| 子系统 | 职责 | 主要代码锚点 |
|--------|------|----------------|
| **Terminal runtime** | cell grid、scrollback、viewport | `src/terminal_runtime.rs` |
| **VT 解析与状态** | ANSI/escape、alternate screen 等 | `src/terminal_runtime.rs` 及关联模块（与 `vt100`/parser 逻辑同树） |
| **Terminal observation** | capture、screen DTO 生成 | `src/terminal_observation.rs` |
| **Terminal cursor / lifecycle** | 光标、dead 态、生命周期 | `src/terminal_cursor.rs`, `src/terminal_lifecycle.rs` |
| **Named buffer / locale** | 终端相关缓冲与 locale | `src/named_buffer.rs`, `src/locale.rs` |
| **Scrollback 常量与数学** | 行数上限、thumb 几何（**server 侧事实**） | `src/lib.rs` `SCROLLBACK_LINES`；几何 **投影** 在 `src/ui_geometry.rs`（产品层，但 **scrollback_offset 真相在 server**） |

**禁止 Facade 暴露（T0）：** `feed_byte`、`get_cell(x,y)`、逐行 parser 钩子。

**允许 Facade（T1，低频）：** `terminal(id).capture(max_bytes)` → 读 **已提交** 输出，不驱动 parser。

### 2.1.6 内核域 E — 平台机制（无产品名的 OS 层）

| 子系统 | 职责 | 主要代码锚点 |
|--------|------|----------------|
| **agenterm-platform** | 窗、输入、IME、剪贴板、截图、字体、DPI、shm | `crates/agenterm-platform/src/**` |
| **Platform adapters** | Win32 / winit-X11 / Wayland 呈现与事件泵 | `src/platform/adapters/windows/**`, `unix/frontend/**`, `linux/**`, `macos/**` |
| **Render / blit 路径** | 像素级绘制、文本栅格化到屏幕 | adapter 内 `render.rs` 等；**非** Rhai |
| **Raw 输入泵** | 键鼠/wheel/IME 进入状态机 | adapter + `src/platform/adapters/.../input*` |
| **WebView host 探测** | 系统 WebView 能力（非 CC 产品逻辑） | `src/webview_host.rs` |

**ARCHITECTURE 对齐：** §1.0 **机制层** + **Host present** 的绘制/事件 **实现** = 内核；**产品语义**（`src/frontend/interaction.rs` 等）= **默认 Rust，可 Strangler 到 pack**。

### 2.1.7 内核域 F — Script / MCP **引擎**（实现体，非用户逻辑）

| 子系统 | 职责 | 边界 |
|--------|------|------|
| **Rhai Engine 宿主** | 编译、执行、预算、隔离 | `src/script_runtime.rs`, `src/script_repl.rs`, `src/bin/agenterm-rhai.rs` |
| **注册与 catalog** | `register_*`、API 树 | `src/script_*.rs`, `src/script_catalog.rs` |
| **MCP stdio 服务** | 只读 MCP 侧车 | `src/mcp_stdio.rs`, `src/mcp_fleet.rs`, `src/mcp_catalog.rs` |

这是 **L2 的宿主**，不是 pack 内容；用户/pack **脚本** 跑在其上。

### 2.1.8 明确 **不属于** 内核（可迁 Rhai App Pack，L3）

| 区域 | 说明 | 代码锚点（初始在 Rust） |
|------|------|-------------------------|
| **CC 呈现文案 / nav / 空态** | composed lines、reason 映射 | `src/control_center.rs`, `src/platform/services/control_center*.rs` |
| **CC shell 宿主** | 窗口壳；pack 只供内容 | `src/platform/services/control_center_shell.rs` |
| **Hub / 超控 IA** | 视图状态机、占位 copy | 设计：`plan/design-cc-hyper-control-agent.md` |
| **LLM SiteAdapter 编排** | 非 PTY | `packs/llm-gateway-*`（设计） |
| **Toolbar / modal 文案与默认值** | 产品策略 | `src/frontend/toolbar.rs`, `*dialog*.rs` 等 **copy/默认** 部分 |
| **Theme token 应用逻辑** | 从 JSON 到 palette 选择 | `src/theme.rs`（**解析规则**可迁；**渲染**仍在 adapter） |
| **构建/qualification Rhai task** | CI 自动化 | `scripts/rhai/**`（永远 **不是** product pack） |

**注意：** `src/frontend/interaction.rs`（focus/wheel/selection **语义**）目前 Rust **单点**——迁 pack 是 **后期** 选项，迁的是 **规则表/策略**，不是 adapter 里的 blit。

### 2.1.9 边界图（一图）

```text
                    ┌─────────────────────────────────────┐
  L3 pack 可迁入 ──►│ CC copy/nav · Hub 空态 · LLM 路由   │
                    └──────────────────┬──────────────────┘
                                       │ product.* / fleet.* (L2)
                    ┌──────────────────▼──────────────────┐
  L2 Facade ───────►│ script_fleet · script_stdlib · llm.*  │
                    └──────────────────┬──────────────────┘
                                       │
     ══════════════════════════════════╪══ L1 内核（本文清单）══
                                       ▼
         server_app · tab_tree · workspace · event_journal · operations
         pty · terminal_runtime · terminal_observation · control_dispatch
         agenterm-platform · platform/adapters (render/input/IPC)
```

### 2.1.10 与 PRD 模块对照

| PRD 模块 | 内核？ |
|----------|--------|
| Terminal runtime (`PRD_02_01` 等) | ✅ 全内核 |
| Agent control plane / IPC | ✅ 内核 + L2 Facade |
| Human workspace **server truth** | ✅ 内核 |
| Human workspace **GUI 布局/手势** | ⚠️ 机制内核 + 语义可 pack |
| Control Center **投影** | ❌ 非内核（可 pack） |
| Rhai scripting **runtime** | ✅ 宿主内核；**用户脚本** L3 |
| LLM gateway **Native Shell** | ✅ sidecar 内核；**pack** L3 |

---

## 3. 边界八条（规范）

### B1 — 内核不可实现

L1 模块 **不得** 在 Rhai 中重写或「策略插件化」到 per-byte/per-cell 控制流。  
**具体范围** 以 **§2.1 清单** 为准（域 A–F = 内核；§2.1.8 = 非内核可 pack）。

摘要（细节见表）：

- 域 A：server 权威、tab 树、workspace、lease、control dispatch  
- 域 B：IPC、protocol、event journal、receipts  
- 域 C：PTY、shell 子进程生命周期  
- 域 D：terminal runtime、parser/grid/scrollback、capture **生成**  
- 域 E：`agenterm-platform`、adapters 渲染/输入泵  
- 域 F：Script/MCP **引擎宿主**（非用户脚本内容）

**可证明：** pack 与 `agenterm.tasks.json` **不得** import 未 catalog 的 native 符号；L1 代码 **不在** Rhai 注册路径；新增 Facade 不得暴露域 D 的 T0 细粒度 API。

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
- **行业对照（Lua/Python/Node/Bun）：** `plan/design-scripting-boundary-comparison.md`
- Agent 纪律：`AGENTS.md`（unrestricted runtime ≠ 无边界 Facade）

---

## 10. 摘要（给评审用）

| 问题 | 答案 |
|------|------|
| 「内核」具体是哪些？ | **§2.1** 六域清单 + 代码锚点 + §2.1.8 非内核 |
| 内核能封给 Rhai 调吗？ | **能**，经 Facade |
| 内核能放 Rhai 里吗？ | **不能**（B1） |
| 关键是什么？ | **边界清晰、严格、可证明**（本文 B1–B8 + Tier + Evidence） |
| JIT 会改变吗？ | 只放大 T2–T3 可迁范围，不改 B1/B5/B7 |
