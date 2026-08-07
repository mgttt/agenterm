# AgenTerm v0.1.16 公开计划

状态：**已定稿，待授权开工**（2026-08-07）  
不创建 tag / Candidate / Release，除非人工明确授权。  
版本列车仍停在 **0.1.15 代码线**；本文件是 **下一列车执行投影**，不替代 PRD。

**主题：多 GUI 产品化收口 + Unix 多实例可达 + 0.1.15 尾账。**

比 v0.1.15 更窄：发布链降本与 install 卫生主波已在 main；本版把用户
已踩到的「多窗 / 多 server」体验做成**可重复、可讲清**的产品面，并补齐
Unix 侧仍缺的多实例入口，顺带关掉最贵的未验证据与测试腐化。

> 产品不变量（已拍板，不得回退）：**GUI 不独占 server**。同一 server 允许多个
> 并发交互 GUI（`ui-lease` 多租约，上限 16）。`As Window` = 再开一扇窗，
> **不是**抢唯一租约、也不是 handoff 到现有窗。

上版工作树与证据：[`plan-v0.1.15.md`](plan-v0.1.15.md)（must-ship 主体已合 main；
公开发版仍未授权）。结构 SSOT：[`ARCHITECTURE.md`](ARCHITECTURE.md)。

---

## 0. 基线事实（2026-08-06 → 08-07）

### 0.1 v0.1.15 已在 main 的主波（不重做）

| 组 | 已合要点 |
|----|----------|
| **R/A′** | cache slim + restore-keys、net-research 出 release 门、script-smoke 左移、step summary |
| **G′** | `--version`、orphan symlink、releases keep、升级提示文案 |
| **H′** | releases.json 派生、provenance 补值（H2 消费端仍后置） |
| **S′/U′** | server strip、同窗 attach、U1/U3 假刷新止血 |
| **B′** | buffer/send-keys 主路径；mux/mcp **独立 PE 移除**，CLI 子命令保留 |
| **租约** | multi-lease + `As Window` 强制 `--ui-client`（`94f0990`） |
| **Unix** | 逐终端 Settings（pri-1）、顶栏 server strip（`dd2bc29`） |
| **rh** | rh-3a…3d + corpus 扫描已合；**M22f 默认 rh 后端** + `agenterm-rhai` 薄壳已合；M23 扩面轨见 [`plan-rh-3.md`](plan-rh-3.md) §5 |

### 0.2 用户现场仍开的痛点（驱动本版主题）

1. **激活标签 As Window「没效果」/ 警告框** — 根因组合：旧 server 独占逻辑 +
   launcher handoff（无 `--ui-client`）+ 进程未退干净。代码已修；**证据与
   重启纪律**仍缺产品化（本版 **W1**）。
2. **「奇怪问题」** — 多窗/多实例路径上仍有边角（菜单 z-order 曾盖、strip 布局、
   脏进程混跑）；本版只收**可复现、可证伪**叶，不扩成大重构。
3. **Unix 多实例 UX** — Settings 与 strip 已开始补；**instance picker /
   open-instance / As Window 语义**在 macOS/Linux 仍不完整（0.1.15 §11.3 优先
   级 2/4 未齐）。

### 0.3 已知测试/证据债（不阻塞写代码，但阻塞「声称全绿」）

- 集成/发布链偶发红：`linux_package`（缺 SBOM 类产物）、`supply_chain` 计数 pin
  —— 需认领，勿在 GUI 叶里「顺手改断言」。
- R1/R2 配置已合，**连续 Candidate `worker.state=reused` + cache &lt;8GB** 仍缺
  观测勾选。
- U2 真机回归、R4 dry-run 真跑：配置/代码在，**人工证据**未收。

---

## 1. 收敛工作树（**可执行清单**）

选择原则（继承 v0.1.14/15）：**宁可少而全绿，不要多而半途**。  
叶定义：用户问题 · 不变量 · 可观察证据 · 安全失败 · 黑盒 owner · 非目标。

### W. 多 GUI / 多窗产品面（本版第一优先）

```text
W. Multi-GUI productization
├─ [ ] W1 重启纪律 + 状态可观测（用户/agent 能分辨新旧 PE 与 lease）
├─ [ ] W2 As Window 黑盒：激活标签 → 第二 GUI + 第二 lease（非 handoff）
├─ [ ] W3 ui-lease status 多 clients 可观测（CLI / snapshot 不谎称独占）
└─ [ ] W4 残留独占文案/路径审计（错误串、handoff 消息、PRD 措辞）
```

- [ ] **W1 重启纪律与版本可观测**
  - **用户问题**：混跑旧 server/GUI → 警告框或「没反应」，误判产品坏了
  - **做法**：文档/状态栏/错误文案明确「须退干净 server」；可选用
    `server-list` + `--version` 对照表写进 agent 指南短节；不自动杀会话
  - **验收**：干净重启路径写进 README/Agents 短段；用户按步骤可复现 W2
  - **非目标**：静默 `taskkill` 全部 agenterm；削弱 keep-server
  - **成本**：小；**依赖**：无

- [ ] **W2 As Window 黑盒（激活标签）**
  - **用户问题**：右键 As Window 必须**真开第二窗**
  - **不变量**：spawn 带 `--ui-client`；允许 `--endpoint`+`--instance`；
    multi-lease attach 成功
  - **验收**：隔离 workspace：附着 strip 激活芯片 → As Window →
    进程数 +1、`ui-lease status` clients≥2、两窗均可交互；失败弹框文案可理解
  - **成本**：中（黑盒/smoke）；**依赖**：W1 干净环境

- [ ] **W3 多 clients 可观测**
  - **做法**：`ui-lease status` / 相关 snapshot 字段诚实列出 `clients[]`；
    文档不写「唯一 GUI」
  - **验收**：两 GUI 附着时 status JSON `attached=true` 且 clients 长度≥2
  - **成本**：小–中；**依赖**：W2

- [ ] **W4 独占语义清扫**
  - **做法**：全仓搜 `exclusive` / `already attached` / handoff 误导文案；
    产品路径不回退 `2d1c235` 式「只 focus 不双开」作为 As Window 默认
  - **验收**：As Window 路径单测/源码锁仍要求 `--ui-client`；PRD multi-lease 一致
  - **成本**：小；**依赖**：无

### Ux. Win 现场尾账（从 0.1.15 迁入）

```text
Ux. Windows residual UX
├─ [ ] U2 标签切换假刷新真机回归（0.1.15）
├─ [ ] U4 TabSelected 不重推整屏 cells（可选，工期紧可砍）
└─ [ ] S4 同窗热切换权威（默认不进 must-ship；仅文档边界）
```

- [ ] **U2** — 空 composer 连点 tab：无 ComposerDraft 风暴；可选黑盒
- [ ] **U4** — 可选协议优化；不阻塞发版叙事
- [ ] **S4** — 明确「同窗热切换」边界：默认 **新窗 / As Window**，不重做权威

### O. Unix 多实例可达（OSX 主责 `unix/frontend`）

> 对照 [`plan-unix-gui-win-parity.md`](plan-unix-gui-win-parity.md) 与
> 0.1.15 §11.3。Settings（pri-1）与 server strip 已开；本版收 **可达闭环**。

```text
O. Unix multi-instance reachability
├─ [x] O-P2 Instance picker（模态 + 6 个 ui-action 接线）
├─ [x] O-P4 open-instance / 新窗拉起（含 As Window 语义对齐）
├─ [x] O-P3 strip 右键菜单深度（Close / As Window 与 Win 行为契约）
└─ [ ] O-evidence macOS 真机：strip 切换 + 第二窗 attach
```

- [x] **O-P2** — 已消灭。Unix 画 + 6 个 action 接进 **shared `control_dispatch`**
  （不是 Unix adapter），两端一份实现。实测：6 行、next/prev/select --name
  可用、confirm 开窗后关闭模态、cancel 关闭；坏名字报
  `instance \`nosuch\` is not in the picker list`。
  **`WINDOWS_ONLY_UI_ACTIONS` 归零**（三个提交前是 14），SHARED 58。
- [x] **O-P4** — `spawn_gui_for_instance` 已落地。路上修了个真 bug：原来同时传
  `--instance` 和 `--endpoint`，被 `parse_gui_launch_target` 判为冲突选择器，
  **子进程其实起不来**；现在二选一。
  ⚠️ **未对齐 `--ui-client`**：Unix 嵌入式 frontend 没有 lease rebind，
  `As Window` / confirm 一律**开新窗口**而不是原地切换。这是有意的语义差异
  （假装切换但没切比明确开窗更糟），不是遗漏 —— 要真对齐需要先给 Unix 做 lease。
- [x] **O-P3** — 右键菜单 `As Window` / `Close` 已上线，菜单最后绘制所以压在
  strip 和工作区之上。菜单 item bounds 进 `ui-snapshot`，agent 可用
  `ui-input pointer` 驱动。
  ⚠️ **Close 没有确认框**：Unix 无 `ModalSurface::ServerClose`，写一半会让用户
  卡在无法 confirm/cancel 的死状态，所以改为直接执行 + 两道 guard（stale 行、
  自己的 server 都拒绝，实测 GUI 存活）。已留 `TODO(macos)`。
- [ ] **O-evidence** — 真机表：切换 instance、As Window、keep-server 后再附着

**禁区**：Lnx 与 OSX **不同时**写 `unix/frontend/**`（继承 0.1.15 §2.2.1）。

### C. 控制台宿主（agenterm-con.exe）

> 动机：cmd.exe 不稳定 + agenterm.exe 自身开发时被锁无法覆盖 → 需要一个
> **最小可用、不依赖 agenterm server** 的控制台宿主（对标 Windows conhost.exe）。
> 基于 `crates/agenterm-platform` 薄封装，预留未来作为 Windows server attach 体。

```text
C. Console host (agenterm-con.exe)
├─ [x] C1 最小 ConPTY 窗：开窗、起 shell、pty 泵、渲染（platform 直调）
├─ [x] C2 键盘输入 + 鼠标选择 + 剪贴板（复用 platform 的 input/clipboard 封装）
├─ [ ] C3 滚动缓冲区 + 字体/DPI 跟随（复用 platform 的 font/screenshot）
└─ [ ] C4 server attach 预留：实现与 remote frontend 相同的 IPC 帧协议（仅接线，不接 Fleet）
```

- [ ] **C1 最小 ConPTY 窗**
  - **用户问题**：agenterm 开发/锁住时没有可靠终端
  - **做法**：`src/bin/agenterm-con.rs`，用 `agenterm-platform` 的 window/pty/input
    创建单窗、起 `cmd.exe`（或 `%COMSPEC%`）、pty 读写泵、blit 渲染
  - **依赖**：C2（输入）、C3（渲染完整性）
  - **验收**：`agenterm-con.exe` 双击启动 → 出现 cmd 窗 → 可输入命令 → 输出正确渲染
  - **非目标**：tab、workspace、Fleet、CC、server 进程

- [ ] **C2 键盘输入 + 鼠标选择 + 剪贴板**
  - **做法**：复用 platform 的 `input` adapter（键盘→ConPTY 写入，鼠标→选择/滚轮）、
    `clipboard` adapter（选中文本 Ctrl+C → Win32 剪贴板 UTF-16）
  - **验收**：文本可选、Ctrl+C/V 工作、滚轮滚动缓冲区
  - **非目标**：raw mouse 模式（无应用接管需求）、bracketed paste

- [ ] **C3 滚动缓冲区 + 字体/DPI**
  - **做法**：复用 platform 的 `font`（字形栅格化）、`screenshot`（区域截图）封装；
    字体大小/DPI 变化时重建 grid 并重绘
  - **验收**：拖窗口边缘改变大小 → 行列自适应；Ctrl+滚轮改字体 → grid 重算
  - **非目标**：主题系统、皮肤、多字体混合

- [ ] **C4 server attach 预留**
  - **用户问题**：未来可能在 Windows 下需要轻量 attach 到 agenterm server
    （类似 Unix 下 `agenterm-cli` attach 到 headless server 的终端体）
  - **做法**：`agenterm-con.exe --attach <instance>` 模式下，实现与
    `src/platform/adapters/windows/remote_frontend` 相同的 IPC 帧协议
    （loopback 连接 → protocol handshake → blit 帧消费 → 输入帧产出）
  - **验收**：`agenterm-con --attach <name>` 连接成功，server 侧 tab 内容渲染到 cmd 窗
  - **非目标**：本版不发 C4；仅「协议接线预留」，不阻塞 C1–C3
  - **成本**：大（需端到端验证 server↔cmd 帧往返）；优先度低于 W/O

---

### R′. 发布链证据收口（配置已合，只收证 + 最小修）

```text
R′. Evidence closeout
├─ [ ] R1e Candidate bootstrap.worker.state==reused 连续两次 + cache 配额
├─ [ ] R2e cargo-home restore-keys 前缀命中日志
├─ [ ] R4e release dry_run 真跑一次（无 tag/draft）
└─ [ ] T-debt linux_package / supply_chain 集成红认领
```

- 不重做 cache 策略；只**观测或修自己引入的红**。
- **T-debt**：发布相关 pin/产物；与 GUI 叶并行，文件域互斥。

### G′′. 安装尾（政策解锁才做）

| 叶 | 条件 | 说明 |
|----|------|------|
| **G1** | G-P1 已拍板可回落 unsigned | macOS `curl\|bash` happy path |
| **H2** | H1 稳定一版后 | install.sh 消费 `releases.json` |
| **G7b/c/d** | 等 G-P2 | 升级遇 running server 的默认策略 |

未拍板：**只做文案/文档**，不改 keep-server 默认。

### L′. 低成本尾账（工期紧按序砍）

砍叶序：**L7 → L1 → L5 → L6 → L4 → L2/L3**（定义见 0.1.15 §1.5 L′）。  
本版 **must-ship 默认只认 L7 + L1**；其余可选。

### Rh. 脚本引擎矩阵（rh / lua / qjs，并行轨，不挤 W/O）

**FYI（2026-08-07 用户口头同步，非本版执行序，先落盘防 compact 丢上下文）**：
脚本引擎侧现在是 **三引擎路线图**，非本 plan 主责 agent 驱动，仅记录以防跨
agent/跨 session 撞车或重复造轮子：

```text
rhai (agenterm-rhai)       — 已取消作为前进方向（2026-08-07）。兼容薄壳仍随
                              M22f 保留、继续吃 shim 硬化修复（Rh-M23d），
                              但不再获得新能力投资；见 PRD §「Script engine
                              family」
rh  (crates/agenterm-rh)   — Lnx 现场 agent 负责，自研语言：语法/对象模型参考
                              rhai 与 Rust std，但不是解释器——checked subset
                              transpile→rustc AOT，比 rhai 更深入底层（生成
                              pack 原生 i64 入口 ABI，不带解释器运行时）；
                              M22f 已默认、M23 扩面进行中（本节原表）
lua (agenterm-lua，新)      — Win 现场 grok.ds（另一个 Grok Build harness，
                              非本 plan 协调的 agent 池成员）负责实现，
                              目标「能力对齐 rh」（见下）
qjs (agenterm-qjs，已开工)  — 2026-08-07 用户拍板 **不等 lua 雏形，即刻开工**
                              （见 §4 QJS-go 更新）；由 agenterm 主协作
                              agent（本 assistant）负责；基于 QuickJS；
                              能力对齐 rh（lua 为并行参照，非阻塞依赖）
```

四引擎详细谱系、各自状态与 shipped/partial/planned 判定，SSOT 现在是
[`prd/PRD_02_10_rhai_scripting.md`](../prd/PRD_02_10_rhai_scripting.md)
「Script engine family」节——本节只记执行序/泳道，不复述 PRD 内容。

**「能力对齐」当前理解**（以 `plan-rh-3.md` 已验证的 rh CLI 契约为基准，
lua/qjs 达到雏形后应比对）：

- 同一套 **L2 facade / catalog**（`fleet.*`、`std.*` 等，见
  `design-scripting-boundary-comparison.md` §2.1/§6）——引擎只换 **L3 执行
  后端**，不各自重新定义宿主 API 表面；
- CLI 动词对齐 `agenterm-rh`（check / eval / pack / check-many / task 等，见
  `plan-rh-3.md` M15/M18/M25a）——同样的 typed JSON 输出、退出码、project
  root 校验契约；
- worker / framed-worker 集成点对齐（`RhRunContext`、`host_eval`/
  `host_run_script` 一类注入点，见 `plan-rh-3.md` M22b/M26c）；
- **不要求** AOT/原生 codegen 对齐——那是 rh 特有的 T0–T3 分层执行策略
  （`plan-rh-3.md` §1 第 3 条），lua/qjs 各自用自身 VM/字节码即可，只要
  L2 契约与 CLI 行为一致。

**本版（v0.1.16）不认领** qjs 实现的验收——不占 §2.2 泳道、不进 §6 验收
总门；相当于用户口中「提前给 v0.1.16 打基础」的**并行地基工作**，进度自
行记录，不阻塞/不被 W/O 阻塞。**已知并接受的风险**（2026-08-07 用户拍板
接受）：lua 是目前唯一验证「能力对齐 rh」规格的独立实现，qjs 与它并行
而非顺序，若规格里有模糊点，两边可能各自解读、后续需要对账——不再等
lua 雏形来去规避这个风险。

| 叶 | 说明 |
|----|------|
| **Rh-M22** | [x] `agenterm-rhai` 薄壳 + **M22f 默认 rh**；Candidate 六 cell 改名仍待人审 |
| **Rh-M23** | AOT 扩面 + check parity + caller wave 1 + shim 硬化（[`plan-rh-3.md`](plan-rh-3.md) §5） |
| **Rh-default** | [x] **M22f 已默认** `AGENTERM_SCRIPT_BACKEND=rh`；显式 `=rhai` 可回退 |
| **Lua-proto** | FYI；Win 现场 grok.ds 实现中，目标能力对齐 rh；无本 plan 验收叶 |
| **QJS-M0** | [x] `crates/agenterm-qjs` 骨架 + QuickJS 绑定选型（`rquickjs` 0.12.2，bundled quickjs-ng，MSVC `cc` 自动探测编译）+ 最小 eval 跑通（算术/字符串/语法错误捕获，3 单测绿）；**暂未接入根 workspace**——lua 侧当时在同一工作树有未提交的 `Cargo.toml`/`Cargo.lock` 改动，用嵌套空 `[workspace]` 表隔离，避免撞车；lua 已提交（`8b3764f5`），接入根 workspace 留给 QJS-M1 |
| **QJS-M1** | [~] `check`/`eval`/`check-many` 三个动词已对齐 `agenterm-rh`：`check` 用 `Module::declare`（真·parse-only，不执行顶层代码，已用会抛异常的顶层语句验证）；`eval` 遵循 rh 现行的 `fn entry()` 强约定（无 entry 直接 fail-closed，不猜整脚本补全值——对齐 rh 的前进方向，不是 rhai 的兼容期整脚本回退）；`check-many` manifest/report JSON 形状、失败 code 分类、exit_class→退出码映射与 rh 逐字段一致，只把 `kind` 换成 `agenterm-qjs-*`（诚实标注引擎，不冒用 rh/rhai 的 kind 字符串）。18 个单测 + clippy 零警告 + 端到端 CLI smoke 全绿。仍差：`pack`/`qualify`/`task`/`run`（见 QJS-M2）；`check` 无项目级 import 图校验（rh 有，见风险表） |
| **QJS-M2** | [~] 已接入根 workspace（去掉隔离用的嵌套 `[workspace]` 表，crate 只留 `[lib]`，真正的 `agenterm-qjs` 二进制在根 `Cargo.toml [[bin]]` 声明、path 指到 `crates/agenterm-qjs/src/main.rs`——和 `agenterm-rh` 同一形状，为将来接 `agenterm::` internals 铺路）；`cargo check --workspace`/`cargo build -p agenterm --bin agenterm-qjs`/`cargo test -p agenterm-qjs` 均绿，Cargo.lock diff 验证过是纯增量、没碰 lua 并发改动。仍差：L2 facade/catalog（`fleet.*`/`std.*`）+ `task`/`run`/`pack`/`qualify` 动词——这部分要读懂 `agenterm::script_backend`/worker 协议才能动，尚未开始 |
| **QJS-risks** | [~] 7 条已知风险，1 条已解——「根 workspace C 依赖冲突」：接入后 `cargo check --workspace` 干净（rquickjs 的 quickjs-ng 和 lua 侧 C 依赖共存无 MSVC CRT/符号冲突），风险解除。其余 6 条仍开放（并行摸索规格对账、线程模型、无 AOT 性能特征、unrestricted 哲学是否走样、版本/哈希可复现性、CI 构建耗时）；详见 PRD §「Script engine family」→「Future」→**qjs execution backend**；QJS-M2 深入 host API 前逐条回访 |

细节 SSOT：[`plan-rh-3.md`](plan-rh-3.md)、[`design-rh-aot.md`](design-rh-aot.md)、
[`design-scripting-boundary-comparison.md`](design-scripting-boundary-comparison.md)。

### M / N / CC / NET

| 轨 | 本版态度 |
|----|----------|
| **M** 多 agent 观察 | 文档/约定可补；大功能推 v0.2.x 除非用户加急 |
| **N1** platform facade | 可选小叶；不阻塞 W/O |
| **L-CC** | 设计稿已有；实现默认 **v0.2.0** |
| **L-NET** | 研究继续，**不进**本版 must-ship |

---

## 2. 排序与三端泳道

### 2.1 建议执行序

| 序 | 叶 | 理由 |
|----|-----|------|
| 1 | **W1 → W2 → W3** | 用户刚踩过；不变量必须可证 |
| 2 | **W4** | 防回退独占文案 |
| 3 | **U2** | 0.1.15 真机债；与 W 正交 |
| 4 | **O-P2 → O-P4 → O-P3 → O-evidence** | Unix 多实例闭环 |
| 5 | **R′ / T-debt** | 证据与发布红；可并行 |
| 6 | **L7/L1** | 极小成本卫生 |
| 7 | **Rh-M23** | 独立轨；不挡 GUI；M22 已 ship |
| 8 | **C1 → C2 → C3** | 后备终端；低优先但高实用；开发间歇可推进 |
| 砍 | U4、S4 实现、M 大叶、G7 策略、H2、C4 | 见表 §3 |

### 2.2 泳道（继承 0.1.15 纪律，略）

| 泳道 | 主机 | 叶 | 可写 | 禁区 |
|------|------|-----|------|------|
| **Win-UX** | Windows | W*、U2、T-debt 若本地 | `remote_frontend*`、lease 相关、最小 PRD | 不抢 workflow |
| **Unix-UX** | **OSX 单写** frontend | O-* | `unix/frontend/**`、shared 仅真共享 | 不与 Lnx 同写 frontend |
| **Lnx-env** | Linux | F 环境、Linux smoke 复验、T-debt | `adapters/linux/**`、环境笔记 | 不写 unix frontend 巨石 |
| **CI-R** | 任意独占 | R′ 观测/最小 workflow 修 | workflows / check.rhai | 不扩 scope 到 GUI |
| **Rh** | 任意 | Rh-M23 | `crates/agenterm-rh/**`、caller 清单、wave 1 CI/bootstrap | 不删 `agenterm-rhai` PE；Candidate 改名仍 HOLD |
| **C-fallback** | 任意 | C1–C3 | `src/bin/agenterm-con.rs`、`crates/agenterm-platform` consumer | 不引入 Fleet/server workspace；不扩成全功能终端 |

规则：一人一热域；shared-first；机制进 `agenterm-platform`；小步 push main。

### 2.3 并发波形

```text
时间 →
  Win-UX:  [W1][W2][W3][W4][U2]
  Unix-UX: [==== O-P2 → O-P4 → O-P3 → O-evidence ====]
  CI-R:    [R1e/R2e 观测][R4e dry_run][T-debt]
  Rh:      [........ M23a/b → M23c → M23d ........]
  C-fallback: [.......... C1 → C2 → C3 ..........]
```

---

## 3. 明确非目标

- 公开 **tag / Candidate / Promotion**（除非另文授权）
- GUI **独占** server 或恢复「As Window = focus 现有窗」为默认
- 夜间彩排 A1、Candidate 自动派发 A2
- gate 大分片、smoke 并行分片
- L-NET 实现、L-CC 大内容、computer-use
- 回退 M22f 默认 rh backend（除非显式 bugfix）；Cranelift JIT
- 结构 SSOT 大重构（S-struct HOLD，待用户通知）
- 静默杀死用户 keep-server 会话

---

## 4. 决策项（agent 不自主拍板）

| ID | 题 | 阻塞 |
|----|-----|------|
| **G-P2** | 升级遇 running server 默认策略 | G7b/c/d |
| **P1/P5** | agenterm.work / Pages 归属 | H5、E1 |
| **D1** | Candidate preflight 是否可祖先 SHA | 仅工具链 |
| **Rh-M22-go** | ~~是否本版替换 `agenterm-rhai` 入口~~ → **M22f 已 ship 薄壳+默认 rh**；Candidate 六 cell 改名仍 HOLD | 公开 rename |
| **S-struct** | 是否开 architecture 围栏重构 | HOLD |
| **QJS-go** | ~~`agenterm-qjs` 何时开工~~ → **2026-08-07 已拍板：不等 lua，即刻开工**（用户接受并行摸索规格的对账风险，见 §1 Rh 节） | 已解除 |

已拍板沿用：G-P1 unsigned 回落+警告；multi-lease；O Settings 对齐；mux/mcp 无独立 PE。

---

## 5. 与其它文档的关系

| 文档 | 关系 |
|------|------|
| [`PRD.md`](../PRD.md) / `prd/*` | 产品真理；本 plan 收敛后同步 capability 状态 |
| [`plan-v0.1.15.md`](plan-v0.1.15.md) | 上版证据与推迟表全文 |
| [`plan-unix-gui-win-parity.md`](plan-unix-gui-win-parity.md) | Unix 对齐地图 |
| [`plan-rh-3.md`](plan-rh-3.md) | rh 并行轨 |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | 热文件 / 分层 |
| [`Agents.md`](../Agents.md) | 并发、观察、开发环 |

---

## 6. 验收总门（本版「做完」定义）

未授权公开发布时，**开发完成** = 下列同时成立：

1. **W2 + W3** 在干净重启下可复现；W4 无独占回退  
2. **O-P2 + O-P4** 在 macOS 真机可达（Linux 复验可选）  
3. **U2** 真机或黑盒勾选  
4. **R′** 至少 R4e 或书面记录「本版不跑 dry_run 的原因」  
5. `lint` / `check --quick` 绿；不引入新的独占 lease 测试  

公开发版另走 Candidate → Promotion 双阶段合同（见 `skills/agenterm-release`）。

---

## 7. 决策记录

| 日期 | 决定 |
|------|------|
| 2026-08-07 | **QJS-go 拍板：不等 lua，本 assistant 即刻开工 `agenterm-qjs`**——用户主动提出「相当于提前给 v0.1.16 打基础」；本 assistant 建议分阶段（骨架先行、L2 对齐后置）并指出并行摸索规格的对账风险，用户选择接受风险、全部提前。仍不占本版 §2.2/§6 |
| 2026-08-07 | **脚本引擎三轨路线图**（FYI）：rh（Lnx 现场，迁移中）/ lua（Win 现场 grok.ds，实现中，目标能力对齐 rh）/ qjs（见上一条）。落盘防 compact 丢上下文；见 §1 Rh 节 |
| 2026-08-07 | 开立 **v0.1.16** 工作树：主题 = 多 GUI 产品化 + Unix 多实例可达 + 0.1.15 尾账；不默认公开发版 |
| 2026-08-06 | multi-lease + As Window `--ui-client` 合 main（`bd51eae`…`94f0990`）；用户确认「GUI 不独占 server」 |
| 2026-08-06 | Unix Settings pri-1 + server strip 合 main；picker/open-instance 仍为本版 O 组 |
| 2026-08-06 | v0.1.15 must-ship 主波合 main；**未**公开 tag/Release |
| 2026-08-06 | **M22f** 默认 `AGENTERM_SCRIPT_BACKEND=rh` + `agenterm-rhai` 薄壳合 main；v0.1.16 Rh 表同步 |

---

## 8. 开工检查单（每 agent 复制）

1. `git pull --ff-only origin main`  
2. 读本节 §1 自己泳道 + §3 非目标  
3. 声明 pathspec 热区；冲突让路  
4. 改 lease / As Window / strip 后：**提醒退干净 server 再测**  
5. 小步 commit；PRD 状态变更同步 owning 模块  
6. 不扩到 HOLD / §3 非目标  

---

*执行投影，非产品宪法。能力状态以 PRD 为准。*
