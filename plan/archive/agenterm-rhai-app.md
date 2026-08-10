# ⚠️ 已归档：Agenterm Rhai 应用包（Thin Base + Rhai App Pack）

> **归档于 2026-08-10。** Rhai 单轨 App 方案已被 QJS Portable App Substrate 取代；
> 现行架构、Phase 0 与后续去向见 [`../plan-v0.1.18.md`](../plan-v0.1.18.md)。本文只保留
> 早期讨论历史，不是现行 App Engine、Host ABI 或版本执行依据。

# Agenterm Rhai 应用包（Thin Base + Rhai App Pack，历史原稿）

| 字段 | 值 |
|------|-----|
| **文档** | 薄 DI 基座 + 随发布携带的 Rhai 应用包 + 可选远程互动更新 |
| **日期** | 2026-08-06 |
| **状态** | 讨论稿 rev1 |
| **关联** | `plan/design-rhai-rust-boundary.md`（封装边界 SSOT）、`plan/design-release-base-vs-apps.md`、`plan/design-llm-gateway-rhai-logic-pack.md`、`plan/ARCHITECTURE.md`、`prd/PRD_02_10_rhai_scripting.md` |

---

## 1. 想法复述（用户提案）

```text
┌─────────────────────────────────────────────────────────────┐
│  Rhai App Pack（随发布带一份；可远程互动更新）                  │
│  · 产品行为、导航、Hub/CC 策略、主题应用逻辑、编排…              │
│  · 跨平台同一套脚本 → UI/UX 语义更对齐                        │
└───────────────────────────┬─────────────────────────────────┘
                            │ host API（DI）
┌───────────────────────────▼─────────────────────────────────┐
│  Thin DI Base（少发版）                                       │
│  · agenterm-platform 机制 + 极薄 bootstrap PE                 │
│  · server / PTY / 渲染循环 / IPC / 凭据 / 签名 / pack 加载器    │
└─────────────────────────────────────────────────────────────┘
```

**运行时：**

1. 若无本地 app pack → 从随 Base 安装的归档 **解压** 到用户目录；
2. 启动时加载 pack manifest + 入口 Rhai；
3. **嗅探**远程是否有新 pack → 用户确认 → 下载/替换 → reload（尽量无需重装 Base PE）。

**动机：** Base Candidate 贵、发得慢；产品进化（CC、Hub、LLM 路由、空态文案）要快；Rhai 已成熟，希望 **一条脚本线** 拉齐 Win/Linux/macOS 体验。

---

## 2. 结论先行（可行吗？）

| 范围 |  verdict |
|------|----------|
| **整体方向** | **可行，但必须分层**；不能理解为「整个 agenterm GUI 用 Rhai 重写」 |
| **Thin DI Base + 可更新 Rhai 包** | **推荐**，与已有 Logic Pack、App 分轨一致，是后者的 **统一打包形态** |
| **Rhai 统一跨平台 UI/UX** | **部分成立** — 统一的是 **语义与行为**（导航、动作、空态、布局参数）；**像素级渲染**仍在 native host |
| **远程互动更新** | **可行**，但必须 **签名 + 显式确认 + 回滚**；不能做成 silent auto-update |
| **替代 Base 频繁发布** | **只能替代「产品层」发布**；协议/PTY/渲染/host API 变更仍要 Base |

**一句话：** 把「应用包」做成 **受管、可热换的 Rhai 产品层**，而不是 npm 式依赖树，也不是把终端内核脚本化。

---

## 3. 与现有架构的对齐

### 3.1 三层边界（`plan/ARCHITECTURE.md`）— 谁进 pack、谁留 Base

| 层 | 典型内容 | Rhai App Pack？ |
|----|----------|-----------------|
| **机制** `agenterm-platform` | 窗/键鼠/PTY/IPC/截图 | ❌ 永远 native |
| **产品语义** `src/frontend/*` | 手势、geometry、action id、CC 导航 | ⚠️ **渐进**：行为策略、布局常量、视图状态机 → pack；热路径渲染循环 → native |
| **Fleet 权威** `agenterm server` | tab 树、PTY、journal | ❌ 永远 native |

### 3.2 与「Base vs Apps」分轨的关系

| 之前 App 形态 | Rhai App Pack 统一后 |
|---------------|----------------------|
| `agenterm-cc.exe` PE | 可保留 **Native CC Shell** PE；**内容/导航** 来自 pack |
| LLM Logic Pack | pack 内子树 `llm/` 或独立 pack id |
| WebView assets | 仍可选；**不**与 Rhai pack 混为一谈（HTML 壳 vs 脚本语义） |
| 内置皮肤 JSON | pack 或 `app.skins` 子包 |

**Rhai App Pack 是 App 分轨的「内容载体」**，不是第三种发布哲学。

---

## 4. 建议目标形态

### 4.1 两个产物

```text
agenterm-base-<ver>-<plat>.zip
  bin/agenterm.exe          # thin bootstrap + server + 渲染 host
  bin/agenterm-cli.exe
  bin/agenterm-rhai.exe     # 仍保留：开发 task、用户脚本、pack 诊断
  pack/agenterm-app-<app_ver>.agp   # 密封 Rhai 应用包（随首次安装携带）

~/.local/share/agenterm/app-pack/   # 解压后运行副本（可写）
  manifest.json
  entry.rhai
  views/ …
  providers/ …
```

### 4.2 与现有 Rhai 供应的关系（「host API」是什么、缺什么）

**先答：** 对 **自动化 / smoke / 构建 task / 用户脚本**，现有 Script API v2 **已经够用**——`std.*`、`rhai::http`、`fleet.*`（含 `ui.snapshot`、tabs、terminal capture、events、operations 目录）在 `docs/agenterm-rhai-runtime.md` 里已 shipped。

**「host API」不是第二套 Rhai，也不是缩减版 runtime。** 它仅指：当 Rhai 从 **「跑完即 exit 的 task」** 变成 **「嵌在 GUI/网关进程里、长生命周期的 product pack」** 时，native 侧还需补的那几条 **嵌入钩子**——仍应 **挂进同一 Script API catalog**，用 `product.*` / `pack.*` / `cc.*` 等稳定 ID，**不**另起权限/profile 体系。

| 能力域 | 现有 Rhai | App Pack 是否够用 |
|--------|-----------|-------------------|
| 读写在册文件、子进程、HTTP | `std::fs` / `process` / `rhai::http` | ✅ 够用 |
| 观察/变更 Fleet（经 broker） | `fleet.*` | ✅ 自动化够用；❌ **不适合** CC 每帧 UI 热路径（IPC 延迟 + 非 in-process） |
| 读主 GUI 语义快照 | `fleet.ui.snapshot()` | ✅ 观察够用；❌ 不能 **驱动 CC 绘制** |
| 侧栏 show/hide | `fleet.ui.tabs.*` | ✅ 主 GUI 侧栏；❌ 不含 CC `hyper_control` / nav |
| 本地 task 清单 | `agenterm.tasks.json` | ✅ 开发/CI；❌ 不是产品 app pack manifest |
| Pack 加载 / reload / 版本 | — | ❌ **缺**（需 `pack.*` 或 CLI 等价，native 实现） |
| 远程 pack 下载 / 签名 / 回滚 | — | ❌ **缺**（需 `update.*` 或 softmgr 契约） |
| CC 每帧行生成 / 指针 hit 语义 | — | ❌ **缺**（今天 Rust `control_center.rs` 内做；pack 化需 **in-process** `product.cc.present(lines)` 或等价） |
| LLM 网关 SiteAdapter | — | ❌ **缺** `llm.*`（见 gateway pack 设计；与通用 fleet 无关） |

**结论：**

1. **不必重写 Rhai**——App Pack 应 **复用** 现有 `std` / `fleet` / `http` / `json` / task 模块机制。  
2. **缺口是「嵌入模式」+「产品面」**，不是「脚本语言不够」：  
   - **嵌入**：长驻 Engine、与帧循环/输入泵同进程、reload 不杀 PE；  
   - **产品面**：CC 呈现契约、pack 生命周期、（可选）LLM `llm.*`。  
3. 新能力 **增量注册** 到 catalog（与 v0.1.9 以来做法一致），**禁止** 平行搞一套 `app-host-rhai`。

### 4.3 Native 嵌入钩子（仅列缺口，非全量 API）

仅在 App Pack 落地时，向 **同一 catalog** 增补（名称待定，示意）：

| 钩子 | 用途 |
|------|------|
| `pack.version()` / `pack.reload()` | 生命周期 |
| `product.cc.on_frame(ctx) -> lines[]` | CC Native-A 行合成 |
| `product.cc.on_key` / `on_click` | 与 `ControlCenterKey` 对齐 |
| `llm.*` | 网关 pack（已另文） |

**Fleet 事实仍走 server**；pack 内可 **调用** `fleet.*`，但 CC 热路径应 **native 缓存 snapshot + pack 只算 presentation**，避免每帧 broker 往返。

### 4.4 DI 分工（修订）

| 域 | Native | Pack（现有 + 少量增补） |
|----|--------|-------------------------|
| 窗口/帧循环/绘制 | ✅ | 提交 lines / 状态 |
| Fleet 变更 | server | `fleet.*` 调用 |
| Pack/update | ✅ loader | Rhai 业务逻辑 |

**禁止：** pack 直接 OS API；**禁止** 为 pack 单独做「阉割 Rhai」。

### 4.5 统一 UX 如何实现（ realistic ）

Rhai **不能** magic 掉 winit vs Win32。跨平台对齐靠：

1. **单一语义模型** — 例如 CC 的 `selected_view`、`hyper_control` 分区状态机在 pack；
2. **单一 geometry 契约** — pack 输出与 `design-control-center-ux.md` 一致的 layout spec，native **只负责** hit-test 与绘制；
3. **单一 copy/空态表** — i18n 键在 pack；
4. **Native-A 合成行** — 今天 CC Phase A 已是「composed monospaced lines」；pack 生成行内容 **比 pack 画像素** 更贴现状。

远期若 CC Phase C WebView：Web 壳是 **另一个 App**，不是 Rhai pack 替代 renderer。

---

## 5. 好处（为什么值得做）

| # | 好处 | 说明 |
|---|------|------|
| G1 | **Base 发布降频** | Candidate/stress 主要封印 thin PE + server；app pack 可 weekly |
| G2 | **进化空间** | 超控 Tab、LLM 路由、Hub 空态、快捷键表、实验功能 flag 在 pack |
| G3 | **跨平台行为一致** | 同一 `entry.rhai` + 同一 manifest；减少 Win/Unix **双份 Rust UI 漂移** |
| G4 | **与 Script 生态一体** | 用户 automation 与产品 app 共享 `fleet.*` 语义；dogfood 自己的 Rhai |
| G5 | **热修复通道** | 站点/API 变更（LLM、Hub connector）可 pack 补丁，无需等 0.1.x |
| G6 | **体积与职责** | Base PE 保持 4 MiB 预算；pack 可含更多 Rhai/JSON/assets 而不胀主 binary |
| G7 | **已有先例** | 构建/qualification 已 Rhai 自托管；LLM Logic Pack 设计已写 |

---

## 6. 坏处与坑（必须正视）

### 6.1 架构坑

| 坑 | 严重度 | 说明 |
|----|--------|------|
| **P1 范围蠕变** | 🔴 | 「整个 GUI 脚本化」→ PTY/渲染延迟、证据门崩溃。**必须写清 pack 边界** |
| **P2 嵌入钩子增量** | 🟠 | 缺口小但 **必须在 Base 注册**（pack/cc/llm）；用 catalog 扩展，勿平行 runtime |
| **P3 双调试栈** | 🟠 | 生产 bug 在 Rhai 还是 native？需 pack 行号映射、结构化 panic、`app-pack doctor` |
| **P4 两套 truth 风险** | 🔴 | pack 若缓存 Fleet 状态 → 第二权威。**pack 只投影** server snapshot |
| **P5 CC 与主 GUI 耦合** | 🟠 | 一个 pack 还是多个（`app.shell` vs `app.control-center`）？建议 **monorepo pack + 模块**，可配置拆分 |

### 6.2 性能与体验坑

| 坑 | 说明 |
|----|------|
| **热路径** | 终端 60fps、键鼠延迟不能把 parser/layout 全放 Rhai；pack 参与 **帧间策略**，不参与 cell 级解析 |
| **启动** | 首次解压 + Rhai compile → 冷启动增加；需 **字节码缓存** 或 AOT 预编译（远期） |
| **内存** | 多窗口 + pack Engine 副本；需单进程单 Engine + reload 纪律 |

### 6.3 安全与更新坑

| 坑 | 说明 |
|----|------|
| **远程更新** | 无签名 = 远程代码执行。**必须**：Publisher 密钥、hash、用户对话框、离线可拒绝 |
| **互动更新 UX** | 「嗅探到更新」不能太骚扰；需 settings：手动/每日/仅安全补丁 |
| **降级攻击** | 禁止回滚到已知漏洞 pack 除非用户显式 |
| **AGENTS.md** | pack **不是** Rhai permission sandbox；审批/配额仍在 harness/native |
| **供应链** | pack 与 Base **分开 receipt**；Promotion 不能假设「pack 随 tag 永远一致」 |

### 6.4 工程与发布坑

| 坑 | 说明 |
|----|------|
| **Qualification 分裂** | Base green + pack broken → 用户仍坏。需 **pack smoke** 在 publish 前 |
| **版本矩阵** | `base 0.1.15 + app_pack 2026.08.06` 组合测试爆炸 → manifest `requires_host` 窄区间 + CI 采样 |
| **agenterm-rhai 角色** | 保留为 **通用 runtime**；产品 pack 用 **embedded engine** 还是 **spawn rhai**？embedded 更一致，spawn 更易隔离崩溃 |
| **开发体验** | 工程师改 UI 要改 Rhai + 懂 host API；需本地 `pack dev --watch reload` |

### 6.5 「像 npm 又不是 npm」坑

用户已明确 **不走 npm/PyPI 依赖图**。pack 更新是 **整包替换 sealed 目录**，不是 `install deepseek-adapter`。若未来有「pack 依赖 pack」，也应 **manifest 显式 bundled**，不做传递求解。

---

## 7. 与远程「互动更新」的产品设计

```text
1. Base 启动 / 每日 / 用户点「检查更新」
2. GET curated manifest（channel: stable/beta）
   → { app_pack_version, sha256, signature, release_notes, requires_base }
3. 若本地旧 && requires_base 满足：
   → 非阻塞通知（CC 或设置）
4. 用户确认 → 下载 .agp → 校验 → staging 目录
5. agenterm-cli app-pack apply --staging （或 GUI 等价）
   → drain UI → reload Engine → 失败则 rollback
6. audit: app_pack_update_applied
```

**非目标：** 静默 overnight 替换、无签名的 URL、pack 内自更新 Bootstrap。

---

## 8. 推荐分期（避免一口吃成）

| 阶段 | 范围 | Base 改动 |
|------|------|-----------|
| **A0 文档** | 本文 + host API 草案 | 无 |
| **A1 LLM pack** | 已有 gateway pack 设计落地 | gateway PE loader |
| **A2 CC chrome pack** | 视图 ID、nav 状态、空态 copy、layout 行生成 | CC host `ui.render` 回调 |
| **A3 Settings/主题** | 皮肤 token 应用逻辑 | 读 settings JSON |
| **A4 随 Base 带 .agp** | 安装器解压 | pack 安装路径规范 |
| **A5 远程更新** | 签名 channel + 用户确认 | `update.*` host |
| **A6 主 GUI 非终端 chrome** | toolbar/strip 行为策略 | **大**；最后做 |

**终端网格、ConPTY、server、parser：不在 pack 路线图内。**

### 8.1 占位 pack + 从 Rust 渐进迁移（Strangler，推荐）

**可以，且应这样做。** 第一版 Rhai App Pack 故意 **极小**——验证生命周期与嵌入链路，不承担产品逻辑；Rust 仍是 **source of truth**，pack 逐步「接管线」。

```text
Phase 0（占位 pack，~几十行 Rhai + 一个 manifest）
  native CC/GUI 行为 100% 不变
  pack 只做：pack.version()、noop entry、一次 fleet.protocol.info() 或读 settings JSON
  证据：cc-snapshot 多字段 app_pack_version；smoke 证明 load/reload 不杀 PTY

Phase 1（接一条竖线）
  例如：空态 copy / footer 一行 / 某 reason 字符串 从 pack 来；Rust fallback 同文案
  feature flag：pack 失败 → Rust 路径（永远有 fallback）

Phase 2+（按模块迁）
  nav 状态机 → hyper_control 分区 → LLM 路由表 …
  每迁一块：删 Rust 重复 + 黑盒断言不变
```

| 原则 | 说明 |
|------|------|
| **Rust fallback** | pack 编译失败 / panic / 超时 → 当前 Rust 行为；用户无感 |
| **双路径短存** | 同屏只允许一种 authority；迁移完成再删 Rust 分支 |
| **先数据后逻辑** | 先迁 JSON/copy/constants，再迁状态机 |
| **先 CC 后主 GUI** | CC 已是独立 PE + composed lines，最适合 Strangler |
| **永不迁移** | PTY、parser、server、platform 机制、渲染 blit |

**v0 占位 pack 最小 native 钩子（仅 3 类）：**

1. **loader**：解压/定位 manifest、内嵌 Engine、`pack.version`  
2. **reload**：CLI `app-pack reload` 或开发 `--watch`（不必 v0 做远程更新）  
3. **一条回调（可选）**：如 `product.cc.footer_line()` → 一行字符串；无则 Rust 默认  

不必 v0 就上 `on_frame` 全屏。占位跑通后，再 **按 PR 把 Rust 函数改成调 pack**。

**迁移顺序建议：**

```text
1. footer / version 字符串 / about 文案
2. unavailable reason → user_message 映射表
3. CC selected_view 默认值与 nav 标签（仍 native hit-test）
4. hyper_control 空态分区 copy
5. layout 行生成（大块，最后与 geometry 测试一起迁）
```

这与「先 LLM gateway pack、后 CC chrome」可并行：**gateway 不依赖 CC on_frame**。

---

## 9. 决策表（帮助拍板）

| 问题 | 选项 | 建议 |
|------|------|------|
| Pack 粒度 | 单 `agenterm-app` vs 多 pack | **单 monorepo pack + 模块**，LLM 可拆 id |
| Engine | 内嵌 vs `agenterm-rhai` 子进程 | **内嵌** product engine；CLI 仍用 `agenterm-rhai` |
| CC 原生 PE | 保留 vs 合并进 agenterm | **保留** `agenterm-cc` thin PE + pack 内容 |
| 远程更新默认 | 开 vs 关 | **关**；仅检查提示 |
| UX 统一目标 | 像素 vs 语义 | **语义 + layout 契约**；像素由 native/theme 保证 |

---

## 10. 开放问题（RA-*）

| ID | 问题 |
|----|------|
| RA-1 | 首版 pack 是否只含 CC + LLM，不含主 GUI toolbar？ |
| RA-2 | pack 字节码缓存是否进 v1？ |
| RA-3 | 远程 channel 自建 vs GitHub Release 资产？ |
| RA-4 | 是否与 `app.control-center` PE 分轨文档合并 manifest 名？ |
| RA-5 | Rhai app pack 是否开源随 repo，还是部分闭源 channel？ |

---

## 11. 交叉引用

- 发布分轨：`plan/design-release-base-vs-apps.md`
- LLM pack：`plan/design-llm-gateway-rhai-logic-pack.md`
- CC 超控：`plan/design-cc-hyper-control-agent.md`
- Script 包契约：`prd/PRD_02_10_rhai_scripting.md`（package-ready，非 registry）
- 架构三层：`plan/ARCHITECTURE.md`

---

## 12. 摘要对照（给用户讨论用）

**可行：** 薄 DI Base + 密封 Rhai App Pack（随装携带、可签名互动更新），作为 **产品层** 统一载体，减少 Base 发版频率。

**不可行/不应做：** 用 Rhai 替代 terminal/server 内核；无签名远程脚本；把 pack 做成 npm 依赖树；期望 pack  alone 消除 native 渲染差异。

**最大坑：** 范围蠕变 + Host API 版本耦合 + 远程更新的信任模型。控住边界则收益大于成本。
