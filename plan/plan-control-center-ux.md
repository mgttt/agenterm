# Control Center UX 设计任务书（L-CC · v0.2.0）

状态：**设计定稿 rev3**（2026-08-05）。本文件是设计**任务书/起点**；
**实现级 SSOT** 见 [`plan/design-control-center-ux.md`](design-control-center-ux.md)
（功能与布局设计文档，含 Key Decisions、几何/hit 契约、PR Plan）。
实现仍归 `prd/PRD_02_21_control_center.md` 与 `src/control_center.rs`。

关联：`plan/plan-v0.1.15.md` §5.3（L-CC 内容成熟）、
`plan/archive/plan-v0.1.11.md` §3.3（首版导航树）、
`research/agenterm-webview/`（WebView 技术探针，非产品定稿）。

---

## 一、设计目标

Control Center（`agenterm-cc`）是**可替换的二级控制面**：人类用户作「决策意志」
（选 server、选 tab、批准安装/运行），工具负责**可执行、可观察、可验证**的
投影与导航。

设计师需交付：

1. **顶层 Tab / 视图切换** 的信息架构与交互模型
2. **每个视图的区域布局**（主栏、侧栏、详情、空状态、降级态）
3. **与主终端 Human workspace 的视觉关系**（二级窗口，不抢主视口）
4. **分阶段可实现的线框**（Phase A 可先 native-text，Phase C 可 WebView）

非目标：Workflow 执行引擎、softmgr 事务、网络节点、Script 权限层——这些能力
只以**诚实空状态 + 未来入口**出现。

---

## 二、硬约束（不可违反）

| 约束 | 设计含义 |
|------|----------|
| 可替换投影 | CC 崩溃/升级不影响 PTY、workspace、server |
| 单一 Fleet 权威 | 所有事实来自 `agenterm server` 快照；CC 不编造数据 |
| 诚实可用性 | 未交付能力显示 `unavailable` + 原因码，不用假数据填充 |
| 无第二权威 | 不能把 Rhai task 列表伪装成 durable workflow |
| 显式事务 | 安装/更新必须用户确认；无静默安装入口 |
| Epoch 连续 | server 重启后旧数据不得当作 live；需 Recovery 视觉语言 |
| 进程复用 | 同一用户域最多一个交互 CC；`open` 聚焦已有窗口 |

视图 ID（已实现于 `capabilities --json`，勿改名除非 P2 决策通过）：

`cockpit` · `workflows` · `extensions` · `info_hub`

---

## 三、现状（设计师起点）

### 已 shipped（v0.1.11–v0.1.14）

- 独立进程 `agenterm-cc`，工具栏 `Control Center` / 紧凑 `CC` 入口
- **仅 Cockpit** 有内容：纯文本 monospace 布局（`NativeTextWindowHost`）
- Fleet 摘要、3 行 tab 视口、键盘/指针选择 tab（`select-window` + receipt）
- 快照 JSON 中 Workflows / Extensions / InfoHub 为 `unavailable` + 原因
- **无 Tab 栏、无侧栏、无视图切换 UI**（`selected_view` 硬编码 `cockpit`）

### 代码热点

| 文件 | 职责 |
|------|------|
| `src/control_center.rs` | 投影、呈现、导航、快照 |
| `src/platform/services/control_center_shell.rs` | 原生文本窗口宿主 |
| `research/agenterm-webview/assets/` | WebView Cockpit 占位（深色实验页） |

---

## 四、推荐信息架构（待设计师细化）

### 4.1 顶层导航：左侧垂直 Tab（推荐）

**理由**：四个主视图 + 未来 diagnostics；左侧栏符合「控制塔」隐喻；
与主终端「左树 + 右视口」形成呼应但不复制；窄窗口下可折叠为图标轨。

```text
┌──────────────────────────────────────────────────────────────┐
│ AgenTerm Control Center          [server ▾] [epoch] [···]   │  ← 顶栏：权威选择 + 连接态
├────────┬─────────────────────────────────────────────────────┤
│ ● Cockpit      │                                             │
│ ○ Workflows    │              主内容区                        │
│ ○ Extensions   │         （随选中视图变化）                    │
│ ○ InfoHub      │                                             │
│                │                                             │
│ ─────────────  │                                             │
│ ○ Diagnostics  │                                             │
└────────┴─────────────────────────────────────────────────────┘
│ status: connected · renderer=native · sequence=…               │  ← 底栏：诊断摘要
└──────────────────────────────────────────────────────────────┘
```

**备选**（设计师需对比稿）：顶部分段 Tab（Segmented）——适合宽窗，但 4+ 项易挤。

### 4.2 Extensions 二级结构

PRD 要求 **PluginHub** 与 **AppHub** 分视图，不能合并成「万能市场」。

```text
Extensions（顶层 Tab）
├─ 子 Tab / 侧栏分段：PluginHub | AppHub | Installed | Sources
└─ 主区：目录列表 + 详情抽屉（安装前只读；事务按钮显式）
```

### 4.3 Workflows 二级结构

```text
Workflows
├─ Definitions（定义列表）
├─ Runs（运行时间线）
├─ Designer（图编辑器 — Phase B 晚于列表）
└─ Evidence（run 关联的 receipt / 截图 / 日志入口）
```

本阶段空状态文案示例：`Workflow runtime unavailable` — 附「需要什么组件」链接。

### 4.4 InfoHub 二级结构

```text
InfoHub
├─ Sources / Subscriptions（左栏）
├─ Items 流（中栏卡片）
└─ Provenance + Routes（右栏详情）
     └─ 显式动作：→ Notification · → Composer draft · → Workflow input
```

**禁止**：卡片上的「一键执行 destructive Fleet action」。

---

## 五、分视图布局规格（线框 v0）

### 5.1 Cockpit（Phase A 优先）

**用户问题**：我现在管的是哪支舰队？谁活着？谁死了？我要切到哪个 tab？

```text
┌─ Server strip ─────────────────────────────────────────────┐
│ Instance: user_main   PID: 12345   v0.1.14 @ 8ff2b5a      │
│ Epoch: 3   Sequence: 1842   Components: server ✓ wf ✗ …   │
└────────────────────────────────────────────────────────────┘
┌─ Fleet summary ────────────┬─ Active tab ──────────────────┐
│ Total 12 · Running 8 · Dead 4 │ @3  build  (running)        │
└────────────────────────────┴──────────────────────────────┘
┌─ Tab roster (scroll) ──────────────────────────────────────┐
│ >* @1  reviewer   (running)                                │
│    @2  logs       (dead)                                   │
│    @3  build      (running)   ← 3-row viewport, 可扩展      │
└────────────────────────────────────────────────────────────┘
┌─ Actions ──────────────────────────────────────────────────┐
│ [Inspect]  [Select in terminal]   keyboard: ↑↓ Enter        │
└────────────────────────────────────────────────────────────┘
```

**交互保留**：现有 `inspect` / `select` 语义；扩展为按钮 + 快捷键，不单靠文本提示。

**降级态**：

| 状态 | 顶栏 | 主区 |
|------|------|------|
| `server_unreachable` | 红/琥珀连接徽章 | 上次快照灰显 +「Recovering…」 |
| `server_incompatible` | 版本不匹配说明 | 阻断操作，仅 diagnostics |
| `projection_worker_unavailable` | 本地 worker 失败 | 不影响 server 的说明 |

### 5.2 Workflows（Phase B · 先空状态壳）

```text
┌─ Empty / unavailable shell ────────────────────────────────┐
│  ◇  Workflows                                              │
│     Workflow runtime is not available on this server.      │
│     Reason: workflow_runtime_unavailable                   │
│     [Learn what ships in v0.2.0 →]                         │
└────────────────────────────────────────────────────────────┘
```

有数据后的目标布局（设计师出高保真）：

- 左：定义列表 + 状态过滤
- 中：Run 时间线 / 步骤
- 右：Evidence 抽屉（链接到 `agenterm-cli` receipt、PNG）

### 5.3 Extensions（Phase B · PluginHub / AppHub 分栏）

```text
┌─ PluginHub | AppHub | Installed | Sources ─────────────────┐
├─ Catalog list ───────────────┬─ Detail panel ──────────────┤
│ □ runtime-bridge   compatible │ Name · version · signature  │
│ □ theme-pack       available  │ [Install…] 显式、需确认      │
│ □ …                           │ Compatibility matrix        │
└──────────────────────────────┴─────────────────────────────┘
```

**PluginHub vs AppHub 视觉区分**（沿用 plan-v0.1.11 §3.4）：

| | PluginHub | AppHub |
|---|-----------|--------|
| 图标隐喻 | 齿轮 / 模块 | 窗口 / 应用 |
| 卡片强调 | capability、runtime | 体验、组合、UI |
| 典型 CTA | Add capability | Open / Install app pack |

### 5.4 InfoHub（Phase C）

卡片流 + 来源色条 + provenance 展开；离线源显示 `stale` 而非隐藏。

### 5.5 Diagnostics（贯穿，可独立 Tab 或 `···` 菜单）

- Component availability 表（与快照 `components` 对齐）
- Connection / renderer / projection worker 状态
- 「Capture PNG evidence」—— 已有 `screenshot` 能力

---

## 六、视觉与组件方向

### 6.1 与主终端关系

| 维度 | Human workspace (`agenterm`) | Control Center (`agenterm-cc`) |
|------|-------------------------------|--------------------------------|
| 角色 | 日常作业视口 + 舰队树 | 观测/编排/扩展的二级控制塔 |
| 密度 | 终端网格优先 | 信息卡片 + 列表优先 |
| 激活 | 主窗口 | `open` 可激活；`--no-activate` 不抢焦点 |
| 主题 | 跟随 terminal theme | **建议继承同一 theme token**，避免两套审美 |

### 6.2 渲染路径（设计需双轨标注）

| 阶段 | 渲染器 | 设计师交付物 |
|------|--------|--------------|
| Phase A | `native` 文本/轻量矢量 | ASCII 线框 + 间距 token（字符格） |
| Phase C | 可选 `system-webview` | HTML/CSS 组件库 + bridge v1 数据绑定表 |

WebView 探针 [`research/agenterm-webview/assets/index.html`](../research/agenterm-webview/assets/index.html)
仅作**暗色密度参考**，不是最终组件库。

### 6.3 状态色（建议，待设计 token 化）

- **Connected / running**：accent green（与官网 `--green` 同源）
- **Dead / exited**：muted，不删除行（对齐「退出≠消失」）
- **Unavailable**：outline + 原因码 monospace
- **Recovering**：cyan 脉冲/徽章（非 modal 阻塞）

---

## 七、交付分期（对齐 v0.2.0）

```text
Phase A — Cockpit 可操作纵切片（设计师优先）
├─ 左侧 Tab 壳 + Cockpit 三区布局定稿
├─ 连接/断开/epoch 降级视觉规范
└─ native 实现线框 → 工程可测的 snapshot 几何契约

Phase B — Workflows + Extensions 空状态壳 → 列表布局
├─ PluginHub / AppHub 分栏定稿
└─ 安装 CTA 交互（仅请求 softmgr，不实现事务）

Phase C — InfoHub + 可选 WebView 富布局
└─ 卡片流、provenance、route 动作
```

---

## 八、设计师待决问题（需出对比稿）

| ID | 问题 | 影响 |
|----|------|------|
| D-CC-1 | 左侧 Tab vs 顶部分段 vs 可折叠图标轨 | 窄窗（760×480）可用性 |
| D-CC-2 | Extensions：子 Tab vs 左二级侧栏 | PluginHub/AppHub 认知负担 |
| D-CC-3 | Cockpit tab 列表：保留 3 行视口 vs 全列表滚动 | 与现 smoke 几何断言 |
| D-CC-4 | Server 切换：顶栏下拉 vs 独立 Cockpit 区 | 多 instance 场景 |
| D-CC-5 | P2 改名是否影响窗口标题与 Tab 文案 | `Control Center` → ? |
| D-CC-6 | Phase A 是否接受「带边框的文本 UI」过渡 | 工程成本 vs 视觉完整度 |

---

## 九、设计师交付清单

- [ ] **IA 图**（四顶层视图 + Extensions/Workflows 二级）
- [ ] **Cockpit Phase A** 高保真线框（含 4 种连接态）
- [ ] **空状态组件库**（workflows / extensions / info_hub 原因码映射表）
- [ ] **导航交互说明**（键盘、焦点、与主终端 tab 选中同步）
- [ ] **Snapshot 几何契约草案**（供 `ui-snapshot` / PNG smoke 断言）
- [ ] **Theme token 表**（与 `PRD_02_06` terminal theme 对齐）
- [ ] （可选）WebView Phase C HTML 静态原型，置于 `research/agenterm-webview/assets/`

---

## 十、证据与验收挂钩

设计定稿后，工程验收对齐：

- `scripts/rhai/control-center-*-smoke.rhai` — 生命周期 + Cockpit 选择
- `agenterm-cc snapshot --json` — `selected_view` 随导航变化
- `agenterm-cc screenshot` — 布局回归 PNG
- `prd/alignment-contract.json` — 证据 ID 增量登记

---

## 十一、决策记录

| 日期 | 记录 |
|------|------|
| 2026-08-05 | 创建本设计任务书；推荐左侧垂直 Tab + 顶栏 server 条；Cockpit Phase A 为设计优先项 |
| 2026-08-06 | 产品意向：远期可吸收为 `agenterm cc` 子命令（独立进程/窗口）；内容面向超级智能体与各类 Hub。近程仍以 server/CLI 为首要。Tauri 三 Tab 占位（超级智能体/超级Hub/超级控制）仅在 `research/agenterm-webview` 做体积观察，不并入发布版 `agenterm-cc`（4 MiB 预算）。 |
