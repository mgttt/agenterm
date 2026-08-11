# AgenTerm v0.1.17 公开计划

状态：**迁移收敛中，待 v0.1.16 发布链收口后开工**（2026-08-10）
不创建 tag / Candidate / Release，除非人工明确授权。
版本列车停在 **0.1.16 代码线**；本文件是 **下一列车执行投影**，不替代 PRD。

**主题：承接 v0.1.16 未完成产品证据 + 发布链证据 + 安装尾 + 脚本引擎深化 + 低成本卫生。**

本版只承接 v0.1.16 仍未完成或明确推迟的叶，不引入新的产品面大叶。
v0.1.16 已完成的大规模并行工作（agenterm-con 产品化、
QuickJS 引擎、跨引擎共享层、SQL 后端）不再重复列出，仅记录其留下的未收
缺口。

> 上版工作树与证据：[`plan-v0.1.16.md`](plan-v0.1.16.md)。
> 结构 SSOT：[`ARCHITECTURE.md`](ARCHITECTURE.md)。

---

## 0. 基线事实（2026-08-10）

### 0.1 从 v0.1.16 迁入的已推迟项

v0.1.16 执行中发生了三件计划外的大规模并行工作（agenterm-con 产品化、
QuickJS 引擎 M0→M5d、跨引擎共享层 Common-M1→M7），合计占掉了本版绝大部分
实际工时，导致以下项被明确推迟到 v0.1.17：

| 推迟项 | 原属 v0.1.16 泳道 | 推迟原因 |
|--------|-------------------|----------|
| **W1–W4** | W 多 GUI / 多窗产品面 | v0.1.16 未开工，迁入后以黑盒与可观测合同收口 |
| **U2** | Ux Windows 尾账 | 空 composer 连点 tab 的假刷新真机回归未收证据 |
| **O-evidence** | O Unix 多实例可达 | 代码已落地，macOS 真机 strip / 第二窗 attach 证据未收 |
| **R1e/R2e/R4e** | R′ 发布链证据收口 | 仅承接 v0.1.16 最终仍未闭合的 Candidate / rehearsal 证据；已在 v0.1.16 证明的项不重复 |
| **T-debt** | R′ 集成红认领 | linux_package / supply_chain 偶发红，需独立调试 |
| **G1** | G′′ 安装尾 | macOS `curl\|bash` happy path；G-P1 已拍板 |
| **H2** | G′′ 安装尾 | install.sh 消费 `releases.json`，依赖 H1 稳定 |
| **G7b/c/d** | G′′ 安装尾 | 升级遇 running server 默认策略；G-P2 已拍板 |
| **L′ (全组)** | L′ 低成本尾账 | L7/L1/L5/L6/L4/L2/L3 全部，工期紧砍 |
| **U4** | Ux Windows 尾账 | 可选协议优化，明确标「工期紧可砍」 |
| **S4** | Ux Windows 尾账 | 同窗热切换权威边界，标「默认不进 must-ship」 |
| **Rh-M23** | Rh 脚本引擎 | 已在 `plan-rh-3.md` 全部完成，作为基线而非迁入叶 |
| **QJS-M6** | QJS 脚本引擎 | API 级静态校验（`shipped_surfaces` 对账），新发现缺口 |
| **C10d** | C 控制台宿主 | 回看搜索、OSC 8 超链接、脏行重绘，标「有余力再挑」 |
| **C-residual** | C 控制台宿主 | 脚本拖拽、真实 TUI 方向键/备用屏滚轮、IME、GUI 输入调用点与 DECKPAM 仍缺 |
| **Engine-debt** | 脚本引擎 | qjs pack 身份、lua fail-closed entry、rh stale surfaces、`agenterm cli script` 弃用入口删除、测试孤儿进程需显式归档或修复 |
| **M/N/CC/NET** | 跨版轨 | 多 agent 观察 / platform facade / Control Center / 去中心化网络，均推 v0.2.x |

### 0.2 v0.1.16 留下的已知缺口（非本版引入，仅记录）

- agenterm-con 方向键在真实 shell 里不生效（ConPTY 翻译疑点）
- agenterm-con native host 已接 IMM32 preedit/commit 与候选框 client-anchor；真实中文输入法端到端仍需人工验收
- qjs `check` 无 `shipped_surfaces` 级 API 静态校验（QJS-M6）
- qjs `pack` 字节码 hash 是指纹而非加载依据（与 lua 同因不同由）
- lua `check_many` 的 `--project-root`/`--timeout-ms` 已修复（Common-M7），但 lua 无 fail-closed entry 契约

### 0.3 agenterm-con 轻量多会话与稳健性（新增，串行实现）

用户问题：独立控制台宿主需要多个独立终端、树式标签、外置输入区及可被
本地工具直接驱动的固定交互面；运行高输出或异常 TUI/harness 时，单会话故障
不能带走整个窗口。

不变量：一个 `agenterm-con` GUI 进程内每个 stable `@N` 标签独占 PTY、解析器、
滚动视图和关闭状态；父标签关闭时仅提升直接子标签。控制端点只在 GUI 存活期
监听，不登记实例、不恢复会话、不成为 server/mux/Fleet/Rhai authority。所有
CLI 请求与输出都有尺寸、数量、时限边界；解析错误、无效目标、截图/I/O 失败、
子进程退出和异常 VT/Unicode 输入必须返回局部错误并保持其他标签可用。

| 叶 | 状态 | 可观察证据 | 排除范围 |
|---|---|---|---|
| C-con-tree | [~] | `ConApp` 已持有 stable ID 树和独立 `ConTerminal`/PTY 表；左侧常驻树栏按父子层级绘制并统一占用终端 left inset，可用快捷键或点击创建、切换和关闭。关闭父节点时直接子节点保持原 PTY 并提升；已有像素 offset 单测和公开 CLI 黑盒，待补树栏鼠标关闭控件 | 不引入持久工作区或 server |
| C-con-composer | [~] | 底部外置输入区可选择焦点、支持键盘和 IME commit，Enter 经当前会话同一 PTY 写路径提交；待补粘贴、目标选择和黑盒 | 不做脚本语言或任务系统 |
| C-con-cli | [x] | 固定 GUI-lifetime `cli --control ENDPOINT` 已实现 `list-tabs`、new/select/close、capture、screenshot、send-text、send-keys、send-mouse、send-wheel、wait-text、perf-stats/reset-perf-stats；Windows 命名管道黑盒覆盖多 PTY 隔离、后台 2000 行压力、截图、异常请求、父节点提升和清理。PTY 队列有界、wake 合并且每 GUI turn 有解析预算，GUI 子系统的 pipe stdout 也已修复 | 不复用主程序 mux/control plane |
| C-con-font | [ ] | 采用主程序同一原生字体/格宽度量路径，默认优先新宋体并验证 ASCII 1 格、CJK/Unicode 宽字符 2 格 | 不在 custom rasterizer 中混拼字体 face |
| C-con-harness | [ ] | Windows 黑盒覆盖高输出、异常 ANSI/Unicode、重入输入、并发 CLI、建关标签、resize/DPI 风暴、IME/鼠标异常；进程与非目标会话持续可用 | 不承诺不可证明的“绝对不崩” |
| C-con-native-host | [~] | `crates/agenterm-con` 为独立 package，Windows 默认 User32/GDI，Linux/macOS 默认 portable；resolved normal graph 无 winit/softbuffer/Rhai/HTTP/TLS/script engine。Native 已接 IMM32 preedit/commit、候选框 client-anchor、pointer capture/loss、mouse-leave、checked DPI suggested-rect，并修复 GCS_CURSORPOS UTF-16→char。截图使用流式 stored-DEFLATE PNG，生产图移除 png/miniz/fdeflate/crc32fast。字体新增 neutral `RasterGlyph` contract：Windows 用 bounded GDI glyph index/gray8 outline，Linux/macOS 的 ab_glyph file-font 路径收进 platform portable adapter；Windows 生产图移除 ab_glyph/ttf_parser。配置/script/snapshot/control 共用 bounded strict JSON codec，serde_json 只作 dev oracle。compact 16/32/64 icon 把 `.rsrc` 从 90,112 B 降到 8,704 B，并有 16 KiB source budget。73 con 单测、185 platform lib tests、control GUI 黑盒、terminal 黑盒 16 pass/2 known-gap ignored/0 fail、Linux portable check 通过。x86_64 release PE 515,584 B，低于 512 KiB budget 8,704 B；release-fast 543,744 B。真实中文输入法和最终字体观感仍需人工验收；ARM64 release size 因本机误命中 Unix `link.exe` 尚无证据 | 不把 Win32 泄漏进产品层；GDI 本轮不拆 surrogate，补充平面返回缺字；接受截图文件较大；不把无 LTO 快版冒充 release |
- rh `shipped_surfaces.rs` 声明的 76 条 fleet.* 中有 32 条在 host `OPERATION_CATALOG` 不存在（stale 声明）
- `agenterm cli script` 已弃用并在 v0.1.17 待删除；公开引擎入口统一为
  `agenterm rh|lua|qjs|sql`，现存调用者、help 与 catalog 仍待迁移
- 测试运行会泄漏 `agenterm.exe server` 孤儿进程锁住构建输出

---

## 1. 收敛工作树（可执行清单）

选择原则（继承 v0.1.14/15/16）：**宁可少而全绿，不要多而半途**。  
叶定义：用户问题 · 不变量 · 可观察证据 · 安全失败 · 黑盒 owner · 非目标。

### W. 多 GUI / 多窗产品面（从 v0.1.16 迁入）

```text
W. Multi-GUI closeout
├─ [ ] W1 重启纪律 + 新旧 PE / lease 状态可观测
├─ [ ] W2 As Window 黑盒：第二 GUI + 第二 lease
├─ [ ] W3 ui-lease status / snapshot 多 clients 诚实投影
└─ [ ] W4 独占语义与回退路径清扫
```

- [ ] **W1 重启纪律与版本可观测**
  - **用户问题**：旧 server/GUI 混跑会表现为警告或无响应，用户无法判断当前
    PE 与 lease 身份。
  - **不变量**：不静默杀会话，不削弱 remain-on-exit / keep-server；版本与实例
    身份必须从公共 CLI 可读。
  - **证据 / owner**：从干净环境按 README/agent 指南运行 `server-list`、
    `--version` 与 lease 状态，再进入 W2；公共黑盒测试拥有该路径。
  - **安全失败 / 非目标**：身份不一致时明确停止并提示干净重启；不实现全局
    `taskkill` 或自动迁移旧进程。
- [ ] **W2 As Window 黑盒（激活标签）**
  - **用户问题**：As Window 必须真正打开第二窗，而不是只 focus 原窗。
  - **不变量**：spawn 带 `--ui-client`；endpoint / instance 选择互斥且可验证；
    第二 lease attach 不破坏第一窗。
  - **证据 / owner**：隔离 IPC/workspace 下从 strip 激活项执行 As Window，证明
    GUI 进程数 +1、`ui-lease status.clients` 至少 2、两窗均可交互；GUI smoke
    与公共 CLI 共同拥有。
  - **安全失败 / 非目标**：启动/attach 失败保留原窗与原 lease 并给出可理解错误；
    不把同窗热切换或强制接管做成替代品。
- [ ] **W3 多 clients 可观测**
  - **用户问题**：多窗已存在时，status/snapshot 不能继续谎称唯一 GUI。
  - **不变量**：`attached` 与 `clients[]` 来自同一 lease 权威；稳定 ID 不以标题
    或索引代替。
  - **证据 / owner**：W2 场景中公共 CLI 与结构化 snapshot 均报告至少两个
    client，关闭一窗后只移除其自身记录。
  - **安全失败 / 非目标**：过期记录显式 stale/unavailable；不靠 UI 文案推断状态。
- [ ] **W4 独占语义清扫**
  - **用户问题**：残留 `exclusive` / `already attached` 文案会让已支持的多 lease
    路径看似失败。
  - **不变量**：As Window 源码锁仍要求 `--ui-client`，产品路径不得回退为
    “只 focus 不双开”。
  - **证据 / owner**：全仓语义审计 + 现有 As Window 单测/源码锁 + PRD
    multi-lease 对账。
  - **安全失败 / 非目标**：发现一端语义不一致则保持功能 unavailable 并登记
    `parity-gap:`；不借清扫之名重构 lease 核心。

### U/O. 跨主机证据尾账（从 v0.1.16 迁入）

- [ ] **U2 Windows 标签切换假刷新回归**
  - **用户问题**：空 composer 连点 tab 不应制造 `ComposerDraft` 事件风暴。
  - **不变量**：无草稿变化就无草稿写入；tab 激活与草稿持久化相互独立。
  - **证据 / owner**：Windows 真机或隔离黑盒连续切换，事件日志与 snapshot
    证明无额外 draft mutation；直接 owning smoke 负责。
  - **安全失败 / 非目标**：无法取得真机时保持未完成，不以单元测试代替；不改
    composer 产品语义。
- [ ] **O-evidence macOS 多实例真机闭环**
  - **用户问题**：已实现的 picker/open-instance/strip 菜单仍缺用户可操作证据。
  - **不变量**：切换 instance、As Window、keep-server 后再附着均指向所选权威；
    失败不得关闭仍 live 的 tab/server。
  - **证据 / owner**：macOS 原生 GUI smoke 通过公共 UI/CLI 完成 strip 切换、
    第二窗 attach、keep-server/re-attach，并保留 snapshot 与 PNG。
  - **安全失败 / 非目标**：无原生主机或 TCC 权威时诚实未验证；不把交叉编译、
    existence-only 或 Linux X11 结果冒充 macOS 真机证据。

### R′. 发布链证据收口（从 v0.1.16 迁入）

> 配置已在 v0.1.15 合 main；v0.1.16 正在修复并重跑发布链。本节只接收其
> 最终仍缺的观测，不重复已由 exact-SHA CI / Candidate / rehearsal 证明的项。

```text
R′. Evidence closeout
├─ [ ] R1e Candidate bootstrap.worker.state==reused 连续两次 + cache 配额
├─ [ ] R2e cargo-home restore-keys 前缀命中日志
├─ [ ] R4e release dry_run 真跑一次（无 tag/draft）
└─ [ ] T-debt linux_package / supply_chain 集成红认领
```

- [ ] **R1e Candidate worker/cache 连续证据** — 用户问题是单次 cache 命中不能证明
  下一次 Candidate 可复用；同一配置连续两次 Candidate 必须在第二次记录
  `bootstrap.worker.state==reused` 且配额未驱逐。owner 是 Candidate timing/cache 摘要；
  无第二次同配置 run 时保持未完成，不为制造证据重写 cache 策略。
- [ ] **R2e cargo-home restore 前缀证据** — 用户问题是 exact-SHA key 每次变化时可能
  退化为冷下载；必须以 R1e 第二次 run 的 restore 日志证明
  `cargo-home-candidate-v2` 前缀命中。owner 是 Candidate cache 日志；日志缺失或 key
  不同则 fail-closed 为未证明，不把推测的命中写成成功。
- [ ] **R4e release rehearsal 无副作用证据** — 用户需要在 Promotion 前证明 sealed
  bytes、身份与参数可验证；真实运行 `release.cmd --rehearse` 并证明无 tag、draft、
  Release 或远端写。owner 是公开 rehearsal 入口及其黑盒测试；任一身份/资产错误须
  停止且保持远端不变，非目标是本叶执行公开 Promotion。
- [ ] **T-debt-linux-package** — `linux_package` 缺 archive/SBOM/receipt 等产物时，
  由 Linux packaging workflow 与 manifest 黑盒给出最小复现和确定修复；缺件必须
  阻止封存，非目标是借机改 GUI 或重排全部 workflow。
- [ ] **T-debt-supply-chain** — `supply_chain` 计数/catalog pin 漂移由 owning
  supply-chain gate 以 resolved lock graph 证据收口；不一致时输出 typed diff 并失败，
  非目标是放宽计数或跳过依赖审计。

**非目标**：不重做 cache 策略设计；不扩 scope 到工作流重构。

### G′′. 安装/更新体验尾账

> G-P1/G-P2 已拍板，G1 与 G7b/c 可执行；H2 仍依赖 H1 至少一轮稳定证据。

| 叶 | 条件 | 说明 |
|----|------|------|
| **G1** | G-P1 已拍板：自动回落且强制警告 | macOS `curl\|bash` happy path |
| **H2** | H1 稳定一版后 | install.sh 消费 `releases.json` |
| **G7b/c/d** | G-P2 已拍板：提示、保会话、不自动 kill | 升级遇 running server 的默认策略 |

- [ ] **G1** — macOS 无 signed asset 时，install 必须自动选择 unsigned-preview，
  并在执行前输出不可静默的多行信任警告；缺 preview 或身份不匹配即停止，绝不把
  unsigned 包装成 stable，README 命令不能替代安装器行为证据。
- [ ] **H2** — install.sh 只从已校验的 `releases.json` 选择版本与资产；index 必须
  绑定 sealed manifest SHA、source SHA、tag、version 与六 artifact。解析、身份或
  checksum 失败即停止，不回退到猜测文件名。
- [ ] **G7b 版本不一致提示** — running server 与目标版本不同时，升级前明确展示
  两端版本与继续后果；无法探测时诚实标 unknown，不假定相容。
- [ ] **G7c 保持 server/会话** — 默认 keep-server 且不自动 kill；继续安装必须说明
  旧进程仍运行、何时生效，失败不得损坏现有会话。
- [ ] **G7d 显式 apply** — 一键 apply 只能由显式 flag 开启且默认 off；其 owner
  必须覆盖确认、精确目标和失败保会话，非目标是后台自动重启。

**非目标**：不改 keep-server 默认行为；不引入 delta 更新。

### L′. 低成本尾账（从 v0.1.14→15→16 连续迁入）

> 顺序：L7 → L1 → L5 → L6 → L4 → L2/L3。每叶继续开放，不能用
> “待从旧计划展开”代替可执行定义。

- [ ] **L7 多文件格式前置纪律** — 多文件 Rust 改动在昂贵编译前运行
  `cargo fmt --check`/仓库 lint；证据是 agent/dev 清单与 fail-fast gate 一致；
  格式失败安全停止，非目标是新增另一套入口。
- [ ] **L1 身份真机回归** — 干净二进制以 custom instance 启动后，`server-list`
  必须显示用户 scope 标签而非误标 main；失败保留原记录并显式报身份不一致，
  owner 是原生 instance discovery 黑盒，非目标是改命名策略。
- [ ] **DOC-PRD capability 对账** — 逐项把 v0.1.16 最终 shipped/planned 状态同步到
  owning PRD 与稳定 catalog；证据是 plan/PRD/alignment 三方无漂移，冲突时停在
  planned 而不虚报 shipped，非目标是另造第二份架构地图。
- [ ] **L5 Control Center smoke 进 CI 评估** — 写明进入/不进入哪个 lane、墙钟
  预算与唯一 owner；若进入，workflow 名称与实际 GUI 工作一致；不把 GUI 工作
  藏入声称跳过 smoke 的 lane。
- [ ] **L6 stale 注册记录体验** — `server-list`/cleanup 或明确引导可识别 stale，
  但绝不误杀 live server；黑盒覆盖 stale 与 live 并存。
- [ ] **L4 Control Center 矮窗 tab 条** — 约 480px 高度仍能折叠/滚动/导航；
  Win smoke 保留结构化几何与 PNG；不借此启动 Control Center 大改版。
- [ ] **L2 persistent-worker dedup 上限** — 为长期增长的 dedup 集合先记录预算/
  淘汰决策，再实现有界行为与饱和测试；不得把 robustness budget 解释成权限。
- [ ] **L3 无 HOME/XDG 的实例目录** — 决定并验证 fallback 的私有目录、符号链接
  与祖先目录完整性；失败必须拒绝不可信共享位置，不增加 Rhai 路径 allowlist。

### Ux. Windows 尾账余量

- [ ] **U4 generation-aware TabSelected delta** — 仅 active 变化且客户端 screen
  generation 一致时省略整屏 cells；generation 落后、未知或断档必须 fail-closed
  拉全量。owner 是 delta 公共协议与兼容黑盒，证据是 select payload 显著缩小且
  stale 客户端恢复正确；非目标是脏矩形 cell 协议。本叶可砍但不得缩写成无合同优化。
- [ ] **S4 同窗热切换权威（默认 v0.2）** — 若以后拍板实现，完整状态机必须是
  确认 → detach 当前 lease → 换 endpoint → 新 bootstrap；失败回到原 context 或
  诚实断开，绝不串 PTY。v0.1.17 只保留原实现叶与去向，不把“边界文档”冒充 S4
  完成；默认新窗 / As Window，非目标是无确认的横向 server tab。

### Rh. 脚本引擎基线

- [x] **Rh-M23** — AOT 扩面、check parity、caller wave 1 与 shim 硬化均已完成；
  证据 SSOT 为 [`plan-rh-3.md`](plan-rh-3.md) §5。v0.1.17 不重复迁移；新的
  rh 工作只来自 E3 或明确的新里程碑。

### QJS. QuickJS 引擎缺口

- [ ] **QJS-M6 operation catalog 静态对账** — 以
  `prd/PRD_02_10_script_runtime.md` 的 `OPERATION_CATALOG` 为权威：已知 literal
  operation 在 `check`/`check-many` 通过，未知 literal 返回 typed fail-closed；动态
  表达式必须标为不可静态证明且不得虚报通过。owner 是 QJS checker 与跨引擎 catalog
  parity 黑盒；`capability` 仅为发现/兼容元数据，绝非授权。非目标是动态 `import()`、
  `fleet.js` export 迁移或给 API 加权限 profile。

### C. 控制台宿主余量

- [ ] **C10d-search（可选）** — 回看搜索有明确命中/无命中/跨行证据与有界失败。
- [ ] **C10d-osc8（可选）** — OSC 8 链接解析、命中与无效 URI 安全失败独立验收。
- [ ] **C10d-dirty-lines（可选）** — 脏行重绘以相同 frame 结果和减少重绘量验收。
  三叶均不阻塞 must-ship；非目标是 server attach（C4）或完整 conhost 替代。
- [ ] **C-residual** — 收口 v0.1.16 仍缺的行为/证据：脚本化拖拽黑盒、真实 TUI
  方向键与备用屏滚轮、IME 端到端、GUI `terminal_input` 调用点迁移与 DECKPAM。
  - **不变量**：输入仲裁、selection ownership、bracketed paste 与安全失败继续
    复用 platform 合同；不在 product bin 复制 OS 机制。
  - **证据**：真实 Windows PTY 黑盒覆盖 shell、IME、拖拽与请求方向键/备用屏鼠标
    的 TUI，并核对 GUI 调用点；任何缺主机证据的项保持 `[ ]`。
  - **非目标**：C4 server attach、完整 conhost 替代、与 C10d 捆绑交付。

### E-debt. 脚本与测试基建剩余债

- [ ] **E1 qjs pack 身份语义** — 决定字节码 hash 是仅 provenance 指纹还是加载
  权威并写入公开 contract；不能把未消费 hash 描述成可执行字节绑定。
- [ ] **E2 lua fail-closed entry** — 明确并测试缺 entry/坏 entry 的稳定错误与退出码；
  不要求 lua 复制 rh AOT 机制。
- [ ] **E3 rh shipped surfaces 对账** — 对 32 条 host catalog 缺失声明逐项删除、实现
  或标 planned/unavailable；未实现 API 是产品缺口，不是权限裁剪理由。
- [ ] **E4 测试孤儿进程** — owning tests 必须清理其 `agenterm server` 子树并证明
  构建输出可覆盖；安全失败保留诊断但不做宽泛进程清理。
- [ ] **E5 删除已弃用的 `agenterm cli script` 入口（to delete）**
  - **用户问题**：同一脚本能力同时暴露 generic CLI 路径与按引擎路径，会让帮助、
    文档、测试和错误提示长期漂移，并继续诱导新调用者依赖待退役入口。
  - **不变量**：公开入口统一为 `agenterm rh|lua|qjs|sql`；删除 alias 不得删除任何
    已发布引擎能力，也不得把 `capability` 元数据解释为授权。
  - **证据 / owner**：先迁移仓库全部调用者，再由 CLI dispatch/help/catalog 与跨引擎
    黑盒证明四个按引擎入口仍可发现和执行；`agenterm cli script` 返回稳定、typed
    unknown-command 失败，且全仓不再把它写成可用命令。
  - **安全失败 / 非目标**：任何仍有 owner 的调用者未迁完时不得先删 dispatch；
    不借此重写引擎执行器、删除 Script API 或引入新权限/profile 语义。

### M / N / CC / NET（跨版轨）

| 轨 | 本版态度 |
|----|----------|
| **M** 多 agent 观察 | 文档/约定可补；大功能仍推 v0.2.x |
| **N1** platform facade | 可选小叶；不阻塞其他 |
| **L-CC** | 设计稿已有；实现默认 **v0.2.0** |
| **L-NET** | 研究继续，**不进**本版 must-ship |

---

## 2. 排序与泳道

### 2.1 建议执行序

| 序 | 叶 | 理由 |
|----|-----|------|
| 1 | **L7 + L1 + DOC-PRD + W1 + W4** | 先清身份、文档、PRD 与独占语义，给黑盒稳定基线 |
| 2 | **T-debt + E4** | 集成红与孤儿进程先恢复 CI 可信度 |
| 3 | **W2 → W3** | 同一隔离多窗 journey，前者产生后者证据 |
| 4 | **U2 / O-evidence** | 各自需要真实 Windows/macOS 主机，可并行 |
| 5 | **R1e → R2e → R4e** | 发布链证据；需 Candidate / rehearsal 窗口 |
| 6 | **QJS-M6 / E1–E3 → E5** | 先收引擎/catalog 债，再迁调用者并删除 deprecated alias；Rh-M23 已完成 |
| 7 | **G1 → H2 → G7b → G7c → G7d** | 安装尾；政策已定，H2 依赖 H1 稳定证据 |
| 余量 | **L5/L6/L4/L2/L3、U4、S4、C-residual、C10d 三叶** | 仍是已登记叶；不满足证据就保持未完成，不静默丢弃 |

### 2.2 泳道

| 泳道 | 主机 | 叶 | 可写 | 禁区 |
|------|------|-----|------|------|
| **CI-R** | 任意独占 | R′ / T-debt 观测与最小 workflow 修 | workflows / owning release scripts | 不扩 scope 到 GUI |
| **Docs** | 任意 | L7/L1/DOC-PRD/W1/W4 文档与审计 | PRD / plan / README | 不改产品代码 |
| **Multi-GUI** | Windows | W2/W3 | shared lease semantics + Windows adapter + owning smoke | 不改 Unix adapter |
| **Win-UX** | Windows | U2/U4/S4/C-residual | owning UI/con files | 不改 release workflow |
| **OSX evidence** | macOS | O-evidence | evidence/scripts only；代码缺口另立 owner | 不把 cross-build 当真机证据 |
| **Rh** | 任意 | E3 | `crates/agenterm-rh/**` | Rh-M23 已完成；不把未实现 API 变成限制 profile |
| **Script CLI** | 任意 | E5 | CLI dispatch/help/catalog + owning blackboxes | 先迁调用者，后删 alias；不改引擎能力 |
| **QJS** | 任意 | QJS-M6/E1 | `crates/agenterm-qjs/**` | 不引入新 unsafe/GC 路径 |
| **Lua** | 任意 | E2 | `crates/agenterm-lua/**` | 不复制 rh AOT |
| **Install** | Linux/macOS | G1/H2/G7 | `scripts/install.sh` | 不改 keep-server 默认 |
| **C-fallback** | 任意 | C10d-search/osc8/dirty-lines（可选） | `src/bin/agenterm-con.rs` | 不扩成全功能终端 |

### 2.3 并发波形

```text
时间 →
  Docs:     [L7/L1/DOC-PRD/W1/W4]
  CI-R:     [T-debt/E4][R1e/R2e 观测][R4e]
  Win-GUI:  .........[W2 → W3][U2]..........
  OSX:      .........[O-evidence]............
  QJS:      [.......... QJS-M6 / E1 .........]
  Rh/Lua:   [.......... E3 | E2 ...............]
  ScriptCLI:[........ caller migration → E5 delete]
  Win-UX:   [U4/S4/C-residual 可选]
  Install:  [G1][H2][G7b/c/d]
  C-fb:     [C10d-search/osc8/dirty-lines 可选]
```

---

## 3. 明确非目标

- 公开 **tag / Candidate / Promotion**（除非另文授权）
- qjs 真实字节码加载 + 执行（已知取舍，非本版）
- 夜间彩排 A1、Candidate 自动派发 A2
- gate 大分片、smoke 并行分片
- L-NET 实现、L-CC 大内容、computer-use
- 回退 M22f 默认 rh backend
- 新脚本引擎（SQL 之后的下一个）开工

---

## 4. 决策项（agent 不自主拍板）

| ID | 题 | 阻塞 |
|----|-----|------|
| **G-P1** | [x] 无 signed asset 时自动选 unsigned-preview，并强制多行信任警告 | G1 已解锁 |
| **G-P2** | [x] 保持 server/会话；版本差异必须提示；不自动 kill，一键 apply 默认关闭 | G7b/c 已解锁，G7d 可选 |
| **P1/P5** | agenterm.work / Pages 归属 | H5、E1 |
| **D1** | Candidate preflight 是否可祖先 SHA | 仅工具链 |
| **Rh-M22-go** | Candidate 六 cell 改名（M22f 薄壳已 ship，公开 rename 仍 HOLD） | 公开 rename |
| **S-struct** | 是否开 architecture 围栏重构 | HOLD |

---

## 5. 与其它文档的关系

| 文档 | 关系 |
|------|------|
| [`PRD.md`](../PRD.md) / `prd/*` | 产品真理；本 plan 收敛后同步 capability 状态 |
| [`plan-v0.1.16.md`](plan-v0.1.16.md) | 上版工作树与已完成项全文 |
| [`plan-v0.1.15.md`](plan-v0.1.15.md) | 上上版证据与推迟表全文 |
| [`plan-unix-gui-win-parity.md`](plan-unix-gui-win-parity.md) | Unix 对齐地图 |
| [`plan-rh-3.md`](plan-rh-3.md) | rh 并行轨细节 |
| [`../prd/PRD_02_10_rhai_scripting.md`](../prd/PRD_02_10_rhai_scripting.md) | QJS-M6 operation catalog 与 Script Runtime 权威 |
| [`design-scripting-boundary-comparison.md`](design-scripting-boundary-comparison.md) | 脚本引擎 L2 契约 |
| [`design-script-engine-trait.md`](design-script-engine-trait.md) | trait 统一设计 |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | 热文件 / 分层 |
| [`Agents.md`](../Agents.md) | 并发、观察、开发环 |

---

## 6. 验收总门（本版「做完」定义）

未授权公开发布时，**开发完成** = 下列同时成立：

1. 先冻结 v0.1.16 最终状态快照；其中仍缺的 **R1e/R2e/R4e** 必须取得合同所列
   证据，不能以另一次不同配置 run 或书面猜测替代
2. **T-debt-linux-package / T-debt-supply-chain** 红已由各自 owner 修复，或带
   typed skip 原因与后续版本去向；不得用宽泛 skip 伪装绿色
3. **W1–W4** 的干净身份、多窗、multi-client 与独占语义证据全齐
4. **U2** Windows 真机/黑盒与 **O-evidence** macOS 原生真机证据均齐；缺主机
   证据时不得把版本标为完成
5. **L7 + L1 + DOC-PRD** 仓库卫生、身份真机与 PRD capability 状态已同步
6. **QJS-M6 / E1–E4** 均有实现证据或明确、可追踪的后续版本决定，不能只留在
   “已知缺口”叙述里
7. **E5** 已完成调用者迁移并删除 `agenterm cli script` 的 dispatch/help/catalog；
   `agenterm rh|lua|qjs|sql` 黑盒全绿，旧入口稳定 typed fail-closed
8. `lint` / `check --quick` 绿；涉及平台行为时 owning native smoke 也绿

---

## 7. 决策记录

| 日期 | 决定 |
|------|------|
| 2026-08-10 | `agenterm cli script` 正式标记为 deprecated、v0.1.17 **to delete**；先迁移全部调用者，公开脚本入口统一为 `agenterm rh|lua|qjs|sql` |
| 2026-08-10 | 用户要求把 v0.1.16 未完成工作全部迁入 v0.1.17：W1–W4、U2、O-evidence、C-residual 与已知 engine/test debt 现均有显式叶、证据、安全失败和非目标；不再用 §3 “非目标”把未完成工作从计划中消失 |
| 2026-08-10 | 开立 **v0.1.17** 工作树：从 v0.1.16 迁入所有已推迟项（R′/G′′/L′/U4/S4/QJS-M6/C10d/M/N/CC/NET）；Rh-M23 经复核已完成，仅保留为基线；主题 = 发布链证据 + 安装尾 + 脚本引擎深化 + 低成本卫生 |
| 2026-08-10 | 旧决定“v0.1.16 保留 W1–W4 + U2 + O-evidence、本版不重复”被同日的新迁移要求取代；这些叶由 v0.1.17 接管 |

---

## 8. 开工检查单（每 agent 复制）

1. `git pull --ff-only origin main`
2. 读本节 §1 自己泳道 + §3 非目标
3. 声明 pathspec 热区；冲突让路
4. 小步 commit；PRD 状态变更同步 owning 模块
5. 不扩到 HOLD / §3 非目标

---

*执行投影，非产品宪法。能力状态以 PRD 为准。*

- `agenterm-con` terminal viewport: add an always-visible right scrollbar with
  reserved grid width, page-click and capture-safe thumb drag. Add a hoverable,
  capture-safe horizontal resize grip for the left tab tree.
  Sidebar drag updates chrome immediately but coalesces PTY and VT grid resize
  through the same trailing-edge path as native window resize and font zoom;
  pointer-move frequency must never become PTY resize frequency.

Low-level follow-up order after review:

1. Move glyph alpha-mask compositing behind a shared row-kernel contract, with
   scalar parity plus x86_64 SSE2/AVX2 and aarch64 NEON implementations selected
   once outside the pixel loop. Inline assembly is justified only where emitted
   code proves intrinsics cannot preserve the required compact loop.
2. Replace per-read PTY `Vec` allocation with a bounded reusable SPSC byte-ring
   owned by `agenterm-platform`; preserve backpressure, edge-triggered wakeup,
   EOF, cancellation, and one-session isolation on Windows, Linux, and macOS.
3. Optimize screenshot RGB packing and IEEE PNG CRC only after measured capture
   evidence. Do not use x86 SSE4.2 `crc32` for PNG: that instruction computes
   CRC32C, not PNG's IEEE polynomial.

Do not hand-write assembly for VT parsing, JSON, Unicode width, tree/workspace
state, or rectangle fills. Those are branch-heavy policy or already lower to
optimized runtime primitives and would gain risk rather than a stable kernel.
