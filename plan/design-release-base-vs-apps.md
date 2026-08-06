# 发布架构：基座（Platform Base）与上层应用（Apps）分轨

| 字段 | 值 |
|------|-----|
| **文档** | 可执行基座 vs 上层应用的分包、分版、分发布产品设计 |
| **日期** | 2026-08-06 |
| **状态** | 设计稿 rev1 |
| **受众** | 产品（主导）、发布/CI、工程实现、softmgr 远期 |
| **SSOT 关联** | `prd/PRD_02_02_executable_family.md`、`prd/PRD_02_17_delivery_quality.md`、`prd/PRD_02_21_control_center.md`、`plan/design-llm-gateway-rhai-logic-pack.md`、`skills/agenterm-release/SKILL.md` |

---

## 1. 执行摘要

**用户/商业问题：** 终端 Fleet 内核（server、PTY、CLI、Script）变更慢、证据门严；Control Center、LLM 网关、Hub 壳、Logic Pack 变更快。若全部绑在一个 zip + 一个 semver + 一次 Candidate，则：

- 改 CC 导航也要重跑全平台 stress qualification；
- 用户为不需要的 GUI 壳被迫下载整包；
- 4 MiB 预算与 WebView/Playwright 需求互相挤压。

**产品结论：** 发布分为 **Platform Base（基座）** 与 **Apps（上层应用）** 两条产品线，共享同一 Fleet 权威与公共契约，但 **独立打包、独立版本、独立更新节奏**。

```text
         ┌─────────────────────────────────────────┐
         │  P4 Apps（体验层，快迭代）                 │
         │  CC · LLM Gateway · WebView 壳 · Hub UI  │
         │  + Rhai Logic Packs · 皮肤/主题包          │
         └──────────────────┬──────────────────────┘
                            │ 依赖 ≥ base.min_version
         ┌──────────────────▼──────────────────────┐
         │  P0–P2 Platform Base（基座，慢迭代）       │
         │  agenterm · server · cli · mux · rhai · mcp│
         │  platform crate · 公共 IPC/协议            │
         └─────────────────────────────────────────┘
```

**近程不推翻** 现有 exact-SHA Candidate/Promotion；**演进**为：Candidate 仍封印 **Base 全集**；Apps 可先 **同 tag 附属资产**，再演进到 **独立 App Release** + softmgr。

---

## 2. 分层定义（产品语义）

### 2.1 Platform Base（基座）

**是什么：** 用户装完即可 **开终端、起 server、用 CLI/Script 自动化** 的最小可信平台。

| 成员 | 角色 | 是否在 Base zip |
|------|------|-----------------|
| `agenterm` / `agenterm.exe` | GUI + `server` 子命令 | ✅ |
| `agenterm-cli` | 公共控制面 | ✅ |
| `agenterm-mux` | RMUX 兼容 | ✅ |
| `agenterm-rhai` | Script 运行时 | ✅ |
| `agenterm-mcp` | MCP sidecar | ✅ |
| `crates/agenterm-platform` | 共享原生契约 | （库，非独立 PE） |
| 内置 Rhai **开发/构建** task 树 | 仓库自动化 | ✅（脚本，非 App） |

**不在基座：**

- `agenterm-cc`（Control Center）
- `agenterm-llm-gateway` / browser worker
- `agenterm-cc-web` / WebView 实验 host
- `agenterm-net`（研究）
- LLM Logic Pack、外置皮肤包

**不变量：** Base 升级 **不得** 静默改变 workspace 语义；server epoch/协议 breaking 必须 major 或 explicit migration。

### 2.2 Apps（上层应用）

**是什么：** 可选、可替换、可独立升级的体验与能力侧车；**不**成为第二 Fleet 权威。

| App ID | 产物 | 典型体积 | 更新频率 |
|--------|------|----------|----------|
| `app.control-center` | `agenterm-cc` (+ 远期 `agenterm cc` 子命令入口) | ≤4 MiB PE | 中–高（UX/IA） |
| `app.control-center-web` | WebView 壳 + `assets/`（research → 可选 App） | host ~521KiB 级 + assets | 高（UI 壳） |
| `app.llm-gateway` | `agenterm-llm-gateway` + browser worker | 独立预算 | 中（Native Shell） |
| `app.llm-gateway-pack` | Rhai Logic Pack | 小（KiB–MiB） | **很高**（站点适配） |
| `app.skins-builtin` | 四预设皮肤 | 小 | 低 |
| （远期）`app.softmgr-ui` | 包管理 UI | TBD | 低 |

**CC 产品定位（上层应用旗舰）：**

- 超控智能体 / Cockpit / Hub 路由 — 见 `plan/design-cc-hyper-control-agent.md`
- Native renderer 为稳定发布路径；Web 壳为 **可选 App**，不绑 Base
- 入口：`open-control-center` → 解析已安装 App；缺失则诚实 `control_center_app_not_installed`

### 2.3 Logic Pack（第三类，非 PE）

| 类型 | 例子 | 发布 |
|------|------|------|
| Gateway pack | `gateway.manifest.json` + providers | App channel 或 user drop-in |
| Rhai task pack | 用户自动化 | 用户/市场（远期） |

Logic Pack **不**进 Candidate 六平台 PE 矩阵；走 **pack 签名 + hash** 通道（见 `plan/design-llm-gateway-rhai-logic-pack.md`）。

---

## 3. 打包单元（Package SKU）

### 3.1 用户可见安装包

| SKU | 内容 | 目标用户 |
|-----|------|----------|
| **Base** | 五/六个核心 PE + 文档 | 只要终端 + 自动化 |
| **Base + CC** | Base + `app.control-center` | 默认推荐（仍可选） |
| **Full Desktop** | Base + CC + LLM Gateway + builtin packs + skins | 超控/Agent 工作流 |
| **App-only update** | 单个 `.agp`（仅 CC 或仅 pack） | 已装 Base 的用户 |

命名示例（远期）：

```text
agenterm-base-0.1.15-win-x64.zip
agenterm-app-cc-0.2.0-win-x64.agp
agenterm-app-llm-pack-2026.08.06.1.agp
```

### 3.2 包内 manifest（每个 App）

```json
{
  "app_id": "app.control-center",
  "app_version": "0.2.0",
  "requires_base": { "minimum": "0.1.15", "maximum": "0.99.99" },
  "requires_protocol": { "control_center_snapshot": 2 },
  "files": [ { "path": "bin/agenterm-cc.exe", "sha256": "…" } ],
  "entrypoints": { "cli": "agenterm-cli control-center", "gui": "agenterm-cc" }
}
```

安装时 softmgr（或近程安装器）校验：**Base 已装且版本兼容**。

---

## 4. 版本与发布火车

### 4.1 双轨 semver

| 轨 | 对象 | 规则 |
|----|------|------|
| **Base** | `agenterm` 产品版本（`Cargo.toml`） | 现有 v0.1.x；协议 breaking → minor/major 按 PRD |
| **App** | 各 `app.*` 独立 semver | CC UX 大改 → app 升 minor；纯 pack → pack 日期/build |

**禁止：** App 版本冒充 Base 版本（如 `agenterm-cc 0.1.16` 暗示整个产品 0.1.16）。

### 4.2 与 Candidate / Promotion 的关系

| 阶段 | 今天 | 目标态（分轨后） |
|------|------|------------------|
| **Candidate** | 六平台 **全 PE** + stress receipt | **Base 矩阵** 必跑；Apps **可选** parallel job 或 nightly |
| **Promotion tag** | `v0.1.15` 单 tag | `v0.1.15` = Base；Apps 可 `app-cc-v0.2.0` tag 或 GitHub Release 多资产 |
| **Receipt** | 单 qualification receipt | Base receipt **硬门**；App receipt **附加**（缺失 App 不阻塞 Base 发布） |

**产品原则：** Base 发布 **永不** 因 CC 截图回归失败而 withheld（除非声明 bundled SKU）。

### 4.3 更新体验

| 更新类型 | 用户感知 | 机制 |
|----------|----------|------|
| Base patch | 「AgenTerm 0.1.15 → 0.1.16」 | 全量 zip；server 协议兼容检查 |
| App update | 「Control Center 更新」 | `.agp` 或差分；**不**替换 `agenterm.exe` |
| Logic pack | 「模型提供方适配更新」 | reload gateway；无 PE |
| 捆绑促销 | 「Desktop 套件 2026.08」 | Release 页多资产一键下 |

---

## 5. 依赖与兼容矩阵

```text
app.control-center 0.2.x
  requires_base >= 0.1.14
  requires: agenterm-cli control-center open
  optional: app.control-center-web (WebView 壳)

app.llm-gateway 0.1.x
  requires_base >= 0.1.15
  optional: app.llm-gateway-pack

app.control-center-web 0.1.x
  requires: app.control-center >= 0.2.0 (native 回退)
  forbids: 链入 app.control-center PE（体积）
```

**CC 双轨 renderer：**

- **Shipping：** native `agenterm-cc` App
- **Optional App：** `agenterm-cc-web` + assets（direct-WRY research 线）
- 主 GUI `open-control-center`：**先**查 optional web app 是否安装且 settings 启用；否则 native CC App；再否则 unavailable

---

## 6. CI / 证据分责

| 层 | 必跑门 | 可降级/夜间 |
|----|--------|-------------|
| Base | fmt/clippy/test、server smokes、IPC、六平台 PE 预算 | stress（仍 Base 门） |
| `app.control-center` | `control-center-*-smoke.rhai`、`cc-snapshot` 几何 | WebView app 可选 |
| `app.llm-gateway` | loopback `/v1/models`、pack reload | browser worker 平台限定 |
| Logic pack | manifest schema、provider 语法、`llm-gateway reload` | 各 site 端到端 |

**artifacts.json 演进：** `executables[]` 拆为 `base_executables[]` + `apps[]`（每 app 自有 budget 与 smoke 列表）。

---

## 7. softmgr 与分发（远期产品面）

| 能力 | 基座 | App |
|------|------|-----|
| 安装 / 卸载 | Base 安装器 | softmgr 按 `app_id` |
| 更新 | 整包 zip | `.agp` + rollback |
| 签名 | 必签 | 必签 |
| 离线 | zip 侧载 | `.agp` 侧载 |
| PluginHub 展示 | — | CC、LLM、Skins 分类 |

近程：**GitHub Release 多资产**（Base zip + CC zip）即可，不必等 softmgr PE。

---

## 8. 产品设计角色（跟进职责）

建议设立 **Release & Apps 产品设计**（可与主控/设计师同人，职责分离）：

### 8.1 持续交付物

| 节奏 | 产出 |
|------|------|
| 每个 Base 版本前 | 《Base SKU 变更说明》— 协议/迁移/非目标 |
| 每个 App 版本前 | 《App 发行说明》— UX delta、requires_base、回退 |
| 架构变更 | 更新本文 + 相关 `design-*` 交叉引用 |
| 开放问题 | 维护 **RQ-*** 表（见 §10） |

### 8.2 决策权（RACI 简表）

| 决策 | R | A | C | I |
|------|---|---|---|---|
| 某 PE 归 Base 还是 App | 产品 | 主控 | 工程/CI | 用户 |
| App 是否阻塞 Base 发布 | 产品 | 主控 | Release skill | 社区 |
| CC 默认 SKU（是否捆绑） | 产品 | 用户 | 工程 | 主控 |
| Logic pack 自动更新策略 | 产品 | 主控 | 安全/法务 | 用户 |
| Candidate 矩阵裁剪 | 产品 + CI | 主控 | — | 全员 |

### 8.3 与其他设计文档关系

| 文档 | 产品角色跟进点 |
|------|----------------|
| `design-cc-hyper-control-agent.md` | CC App 功能分期 vs Base 解耦 |
| `design-llm-bridge-web-to-api.md` | Gateway App + pack 分发布 |
| `design-llm-gateway-rhai-logic-pack.md` | pack 通道与版本 |
| `plan-cc-automation-cli.md` | `agenterm-cli cc *` 对 App 生命周期的 SSOT |

---

## 9. 分阶段路线图

| 阶段 | 产品态 | 工程态 |
|------|--------|--------|
| **P0 文档**（当前） | 本文 + SKU 定义 | 仍 monolithic zip |
| **P1 发布页分资产** | GitHub Release：`base` zip + `cc` zip 分下载 | `package-qualified` 产出多 archive |
| **P2 manifest** | 每 App `app.manifest.json` + requires_base | 安装脚本校验 |
| **P3 CC 可选** | 默认 Base-only 可装；CC 可选勾选 | GUI 无 CC 时 honest unavailable |
| **P4 Candidate 分 job** | Base Candidate 硬门；App Candidate 并行 | receipt 拆分 |
| **P5 softmgr** | PluginHub 安装/回滚 | 签名 `.agp` |

**与当前优先级对齐：** 近程 **server/CLI** = Base；CC / LLM = App 线 **P1–P2**，不拖 Base 0.1.15 收口。

---

## 10. 开放问题（RQ-*）

| ID | 问题 | 建议 |
|----|------|------|
| RQ-1 | 默认下载 SKU 是否含 CC | **Base+CC 推荐**，提供 Base-only |
| RQ-2 | `agenterm cc` 子命令 vs 独立 `agenterm-cc` PE | **过渡期并存**；App manifest 指向同一实现 |
| RQ-3 | WebView 壳是否单独 App | **是**（`app.control-center-web`） |
| RQ-4 | LLM browser worker 是否随 Gateway App 一起 | **是**；仍独立 PE |
| RQ-5 | App 是否共用 Base 的 Git tag | **否**；Base `v0.1.x`；App 独立 tag 或 Release asset 版本 |
| RQ-6 | macOS/Linux CC 与 Win 同 App 版本号 | **是**（同 app_id 跨平台矩阵） |
| RQ-7 | 产品设计角色是否 spawn 分身 | 主控可 spawn **产品设计** 会话跟 P1–P2 线框+Release note |

---

## 11. 交叉引用

- 可执行体族：`prd/PRD_02_02_executable_family.md`
- 交付质量：`prd/PRD_02_17_delivery_quality.md`
- Release 操作：`skills/agenterm-release/SKILL.md`
- CC 产品：`prd/PRD_02_21_control_center.md`
- 扩展波 W4：`prd/PRD_02_19_inspiration_and_future_vision.md` §EXT
