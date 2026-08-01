# AgenTerm v0.1.12 公开计划

状态：产品建设期（2026-08-01 重基线）；远未进入 RC，Candidate/Promotion
仅是 Wave D 的远期交付门，不是当前实施主线，也不构成本阶段进展定义。
工作主题：**收敛 v0.1.11 基础、折叠候选到发布的等待时间，并让三平台
Control Center / native IPC 进入可持续演进状态**

本文是执行计划和决策记录，不替代产品事实。接受后的产品范围、状态与
验收证据必须同步进 `PRD.md` 及对应 `prd/PRD_*.md`；实施中允许按证据调整
波次，但不得用计划中的愿景冒充已经发布的能力。

## 当前执行主线（持续更新）

```text
已完成前置
└─ revision-4 Platform Facade 全量 OS 抽象
当前产品纵切
├─ [~] Control Center Cockpit 可用只读事实
│  ├─ server/build/epoch/sequence
│  ├─ running/dead/active tab health
│  └─ component availability + native renderer agreement
├─ native IPC / LogicalInstance 行为收敛
├─ Script REPL hardening（主体已 shipped）
├─ agenterm-net N2 experimental 纵切
└─ system-WebView research（不得替代 native CC）
远期交付门
└─ Wave D Candidate → 人工批准后的 Promotion/Release
```

近期顺序以产品价值和依赖为准：先让共享 Cockpit 合同成为可用诊断面，再
推进可独立验证的 Script、network 和 Web host 纵切。Candidate workflow
合同可以维护，但在产品阶段成果、三平台证据和用户明确意图之前不 dispatch，
也不以“具备 RC 条件”替代 v0.1.12 产品建设。

2026-08-01 首个建设期增量：Cockpit snapshot 新增明确的
`tab_counts.{total,running,dead}`，native shell 同源显示 logical instance、
server PID/version、build commit/profile/cleanliness、epoch/sequence、active stable tab ID/title 和四类 component
availability。390 项 Quick tests、七产物 dev build 和完整 Windows
`control-center-smoke` 通过；加入 build identity 行后的 renderer-owned
760×480 PNG 为 43,509 bytes，
no-activate、因果刷新、server recovery、typed close 与 orphan-free cleanup
保持通过。native renderer inspect/select 导航和 Linux 原生 renderer 证据仍是后续叶。

2026-08-01 第二个 Cockpit 纵切已合入前验证：`agenterm-cc inspect/select --tab
@ID` 只接受 canonical stable ID；inspect 保持当前 selection，select 复用
server-owned `select-window` typed control receipt，并在同一 PID/epoch 上重读
权威状态后返回 typed tab facts 与 `post_state_verified`。聚焦 parser/contract
测试 28 项、391 项 Quick tests、七产物 dev build 和完整 Windows
`control-center-smoke` 通过；新增 `control-center.typed-navigation` 证据，
renderer-owned 760×480 PNG 为 58,125 bytes。headless server 下 missing target
保持 active tab 且不创建 CC registry。native Cockpit pointer/keyboard 导航仍
未实现，不以 CLI entry point 冒充 renderer 交互。

2026-08-01 live dogfood 新增阻断项（结论与修复证据必须回写对应 PRD）：

- [ ] P0：Windows terminal 内容与 native frame 持续闪烁；先区分无状态变化的
  redraw/invalidate loop、背景擦除和 resize/DPI feedback，不以降低刷新率掩盖。
  白箱审计已定位 replaceable GUI 直接在 window HDC 上清空四区后逐层画回，
  没有 offscreen frame + single BitBlt；高输出 delta 约 10Hz 暴露半成品帧。
  此外 2 秒 lease heartbeat 被误算为 visual change，idle 也触发全窗重绘；
  `CS_HREDRAW|CS_VREDRAW`、NULL dirty region 与同尺寸 resize 是次级放大器。
  修复顺序冻结为 heartbeat/redraw 解耦 → GDI 双缓冲原子提交 → 同尺寸
  resize/event 去噪 → 可选 dirty-region 优化，并用 telemetry + 60fps 真机观察验收。
- [ ] alternate-screen harness 无法本地向上滚动；初步证据指向 `vt100`
  alternate grid 的零 scrollback，需要在 application raw-mouse ownership 之外
  评估把 wheel/PageUp 语义转交前台 TUI，不能破坏普通 scrollback。
- [~] terminal focus 下 `Shift+Tab` 修复正在集成：共享 named-key encoder 已按
  xterm modifier 参数覆盖 Tab/方向/Home/End/Insert/Delete/Page/F1–F12；Unix
  两条输入路径保留 modifiers，Windows `WM_KEYDOWN` 显式处理 Tab/Insert/Delete
  并屏蔽 Tab/Escape 的 `WM_CHAR` 重复回声。commands 与 Windows mapping 聚焦
  单测、395 项 Quick library tests、all-target Clippy、alignment partial
  fail-closed 集成测试与七产物 dev build 通过。Windows live GUI 仍运行旧映像；
  重启后真实 terminal byte dogfood 仍待验证，不能只以编码函数测试宣称交互
  已收口。Windows host 的 Linux `cargo check` 仅因缺少
  `x86_64-linux-gnu-gcc` 停在 `ring` 构建脚本，Unix adapter 仍需原生 CI/host
  证据。
- [ ] terminal 鼠标选区无法可靠建立，导致已实现 copy/paste 无法使用；复查
  selection ownership、drag threshold/capture、raw-mouse arbitration 和复制黑盒。
  白箱审计定位当前选区绑定整个 `screen.generation`：持续 output delta 在 100ms
  reconcile 时清空 drag/completed selection，paint/copy 也因 generation 不等而
  拒绝；drag 中取消还可能漏掉 `ReleaseCapture`。修复需区分 same-grid 内容推进
  与真实尺寸/tab 失效，缓存完成态复制文本，并在 drag 中注入输出验证 phase、
  Ctrl+C/system-menu Copy 和 capture release。现有“先等输出静止再同步拖拽”的
  smoke 不足以证明该行为。

这些 dogfood 缺陷优先于新增 Cockpit 装饰和远期 Candidate 工作；修复必须保留
结构化 snapshot 与 PNG/公开 input journey 证据，并避免多个 agent 并发编辑
Windows remote frontend 等热文件。

## 一、树式精华

```text
v0.1.12  Convergence & Fast Promotion
│
├─ P0：候选只验证一次，发布只提升已验证字节
│  ├─ 普通 CI、候选 qualification、tag promotion 职责分离
│  ├─ 同一 commit 不再本地、CI、Release 三次重复完整门禁
│  ├─ exact-SHA receipt + artifact hashes + SBOM + provenance 成为提升凭证
│  ├─ 六平台候选产物在批准前生成；tag 后只验证并发布
│  ├─ tag-to-Release 目标：热缓存 p50 ≤ 3 分钟、p95 ≤ 6 分钟
│  ├─ 失败在 tag 前暴露；失败 tag / 半成品 Release 不进入正常流程
│  └─ 缓存、runner、队列、编译、测试、打包、上传均有分段计时
│
├─ P0：native IPC 与 LogicalInstance 发布后收敛
│  ├─ main/dev 单例、隔离、发现和显式 endpoint 行为一致
│  ├─ Windows named pipe 与 Linux/macOS Unix socket 权限事实可验证
│  ├─ stale socket / stale registration / PID reuse 安全恢复
│  ├─ schema-v1/v2、旧 TCP 与新 native endpoint 混合升级/回滚
│  ├─ server-list 不把测试残骸长期显示成真实 server
│  └─ 所有 GUI/CLI/CC/MCP/Mux/Script 继续复用同一 resolver
│
├─ P0：三平台 GUI 与 Control Center 可用性收敛
│  ├─ 主工作台工具栏、Tabs、Composer、locale、font 行为对齐
│  ├─ Control Center 选择调用者的同一 logical instance
│  ├─ Cockpit 从壳升级为有用的只读 Fleet 诊断面
│  ├─ Unix Control Center 获得真实 snapshot + renderer-owned PNG 证据
│  ├─ incompatible / renderer failure / server-retained 故障矩阵补齐
│  └─ GUI/CC 仍是可替换投射，server/PTY/workspace 权威不进入 UI
│
├─ P1：开发反馈继续折叠
│  ├─ lint → 定向测试 → 平台 quick → candidate 的逐级反馈
│  ├─ Rust/Cargo 缓存按 OS、arch、toolchain、lock、profile 正确分层
│  ├─ 评估 sccache/远程缓存，但缓存 miss/损坏不能改变正确性
│  ├─ GUI 黑盒按隔离资源并行，禁止窗口风暴、固定 sleep 和残留进程
│  └─ 慢门、缓存命中率、CPU/IO 利用率进入机器可读报告
│
├─ P1：付费/自托管 runner 有证据试验
│  ├─ 先用当前 workflow 记录 3 次冷/热基线
│  ├─ 比较 GitHub 8-core larger runner、Depot 与可信自托管 Windows
│  ├─ 用真实 AgenTerm qualification 比较时间、价格、队列和失败率
│  ├─ 第三方 action 固定 commit，最小权限，不向不可信 PR 暴露凭据
│  └─ 只有端到端收益显著且可回退时才切换默认 runner
│
├─ P1：脚本与二级产品继续准备
│  ├─ [x] agenterm-script 已交付真正的持久 REPL
│  │  ├─ ReplSession 会话内核与 CLI 输入适配解耦，可供 CC/Agent 复用
│  │  ├─ 变量、函数、多行单元、错误恢复、reset 和内存 history
│  │  ├─ TTY 提示与 pipe/NDJSON 自动化输出分离
│  │  ├─ 单元失败不提交语言状态，外部真实副作用不伪装回滚
│  │  ├─ 普通 worker 与 REPL 复用同一 Engine/API 配置
│  │  └─ [~] Ctrl+C、箭头历史与 kill/restart 长驻协议继续 hardening
│  ├─ 评估把 canonical Rhai 入口从 agenterm-script 重命名为 agenterm-rhai
│  │  ├─ 名字直接表达语言/runtime 身份，不再暗示抽象的通用脚本沙箱
│  │  ├─ 先冻结 CLI、task、worker、包名、文档和第三方调用者影响清单
│  │  ├─ 若本轮实施，agenterm-script 作为有期限的兼容转发入口，不复制 runtime
│  │  └─ 构建、测试、Candidate、Promotion 必须在 canonical 名切换后保持自举
│  ├─ Control Center 的 Workflows/Extensions/InfoHub 保持真实空状态
│  ├─ agenterm-net 启动 N2 受控纵切（独立常驻 full node）
│  │  ├─ 显式 start/status/stop；持久身份、block store 与可观测资源账本
│  │  ├─ DHT、pubsub、relay 各自 capability / budget / 失败语义，不以编译成功冒充可用
│  │  ├─ 仅用户拥有节点间、显式配对的 read-only Remote Fleet attach
│  │  ├─ GUI / server / PTY 不链接 libp2p；网络 node 崩溃不得影响本地 Fleet
│  │  └─ 先证明本机与两节点闭环，再讨论公网默认、远程控制或稳定发布
│  └─ system-WebView / Tauri-compatible host spike 启动
│     ├─ 首个本地打包、只读 Cockpit Web UI；native CC 仍是可靠 fallback
│     ├─ Windows WebView2 / macOS WKWebView / Linux WebKitGTK 分别实测
│     ├─ 量化 EXE、archive、runtime 依赖、冷启动、RSS、DPI、截图与崩溃恢复
│     └─ bridge 只允许 versioned facts / fleet snapshot；无 eval、shell、网络逃逸
│
└─ 明确延后与未来计划
   ├─ executable consolidation 决策树
   │  ├─ 首选：共享 Rust runtime/library + 多个职责清晰的薄入口
   │  ├─ 可研究：agenterm-rhai 被宿主进程内嵌，但独立 CLI 合同仍可用
   │  ├─ 可研究：兼容入口按使用证据退场，而不是永久增加同义 EXE
   │  └─ 不做：为“少一个文件”牺牲 GUI/Console 子系统、管道退出码或故障隔离
   ├─ 完整 Workflow/Pipeline 设计器、调度器与跨机恢复
   ├─ PluginHub/AppHub 公共市场、交易、静默安装与自动更新
   ├─ InfoHub 自动执行外部信号
   ├─ 未经 N2 跨平台、恢复与资源证据即把 agenterm-net 宣称为 stable full node
   ├─ 未经生产证据即把系统 WebView 设为唯一 Control Center renderer
   ├─ 未经认证/加密/威胁模型即开放公网远程控制
   └─ Agent harness 的权限、审批、凭据与策略
```

## 二、版本 outcome

> v0.1.12 发布候选应在用户批准 tag 前，已经由同一 Git commit 产出并验证
> 六平台归档、完整 Windows 行为 qualification、平台原生 smoke、SBOM、
> provenance 和 exact-byte receipt。用户批准后，tag workflow 只验证 tag
> 与候选身份并提升已有字节，正常情况下数分钟内出现完整 Release。与此同时，
> v0.1.11 引入的 native IPC 和 Control Center 在 Windows、Linux、macOS 上
> 具备一致、可诊断、可恢复的基础行为，而不会把 UI 生命周期重新耦合到
> server/PTY。

## 三、为何本轮优先做“收敛与提速”

v0.1.11 的实际发布给出了可量化事实：

- 本地普通全套门禁约 **376.8 秒**；
- 本地 stress-inclusive qualification 约 **512.7 秒**；
- tag workflow 中 Windows x64 又执行一次完整 release quality gate；
- 同一 tag 同时触发普通 CI 和 Release workflow；
- Linux/macOS 构建先完成，最终 Release 被 Windows x64 重复门禁串行阻塞；
- Linux GUI wrapper 缺陷直到首个 tag matrix 才被真实 Unix package step
  发现，说明“tag 前六平台候选”仍未形成闭环。

这不是单纯的“机器慢”，而是工作被放在了错误时间重复执行。优先级应为：

```text
先消除重复与 tag 后发现
  → 再建立正确缓存
    → 再调整并行拓扑
      → 最后用更快 runner 放大已经正确的流程
```

## 四、候选与发布流水线

### 4.1 建议拓扑

```text
push main
├─ Fast CI
│  ├─ lint / fmt / PRD alignment
│  ├─ warnings-denied Clippy
│  └─ 定向 unit + representative public smoke
│
└─ Candidate workflow（显式触发或满足候选条件）
   ├─ Windows x64：一次完整 stress-inclusive qualification
   ├─ Windows ARM64：build + package
   ├─ Linux x64/ARM64：native/cross build + package + archive fixture
   ├─ macOS x64/ARM64：native build + package + unsigned/signed lane
   └─ aggregate
      ├─ exact-SHA qualification receipt
      ├─ six-platform asset manifest
      ├─ hashes / SBOM / provenance
      └─ immutable candidate run identity

用户明确批准
└─ tag vX.Y.Z 指向同一 exact SHA
   └─ Promotion workflow
      ├─ 验证 tag/version/SHA/candidate run/receipt
      ├─ 下载并复核候选字节
      ├─ 可选 GitHub artifact attestation
      └─ 创建 Release，不重新 Cargo build、不重跑完整 GUI suite
```

### 4.2 安全合同

- 候选资格绑定完整 commit SHA，禁止只按 branch、tag 名或“最近成功”选择；
- promotion 必须验证 receipt、Cargo.lock、artifact manifest、SBOM 和每个
  archive hash；候选 artifact 缺失或过期时 fail closed，回到 candidate
  workflow，不现场悄悄重建另一套字节；
- release approval 仍是用户动作；优化等待时间不降低发布权限门槛；
- GitHub Actions 引用继续固定到完整 commit；
- 第三方缓存只影响速度，cache miss、eviction、poison 或服务不可用不得改变
  required gates、产物身份和结果；
- public PR 不接触 release token、签名材料、自托管可信 runner 或可写缓存；
- promotion 只拥有创建 Release 所需的最小 `contents: write`，candidate build
  默认只读源码并上传工作流 artifact；
- macOS signed stable 与 unsigned preview 继续严格分流。

### 4.3 SLO 与观测

每次 workflow 输出以下时间，不再只报告总时长：

```text
queue
checkout/toolchain
cache restore + hit/miss + bytes
compile
unit/Clippy
GUI/public smoke
stress
package
artifact upload/download
promotion
tag-to-public-Release
```

首轮先记录当前 runner 三次冷/热基线，再接受目标：

- 普通 push 首个有用失败：p95 ≤ 90 秒；
- 候选 workflow：热缓存 p50 ≤ 8 分钟，p95 ≤ 12 分钟；
- 已有完整候选的 tag-to-Release：p50 ≤ 3 分钟，p95 ≤ 6 分钟；
- 无 tag 后才首次发现的 package member、权限或 launcher 缺陷；
- exact SHA 正常路径只执行一次完整 stress-inclusive qualification。

## 五、缓存与 runner 试验

### 5.1 先做的无供应商优化

1. 审计当前 workflow 是否缓存 Cargo registry、Git checkout、toolchain 与
   `target`；先区分下载缓存和编译产物缓存。
2. key 至少包含 OS、architecture、Rust version、Cargo.lock hash、profile
   和影响 feature/target 的版本化 salt。
3. 不在互不兼容的 host/target/profile 间共享 `target`；只允许安全的
   fallback key。
4. 候选 workflow 与普通 CI 可复用依赖/编译缓存，但候选 archive 必须由
   exact SHA 的受控 job 产生。
5. 评估 `sccache` 时记录命中率、传输字节、压缩时间与总 wall clock，
   不能只看“cache hit”文本。

### 5.2 付费方案试验顺序

| 方案 | 优点 | 前提/风险 | 试验结论门 |
|---|---|---|---|
| GitHub larger runner | 官方托管、Windows 4–96 vCPU、自定义镜像 | 需要 GitHub Organization 的 Team/Enterprise；始终按分钟付费 | 先试 8-core Windows，端到端至少快 35% |
| Depot | Linux/Windows/macOS、快速缓存、按秒统计、改 runner label 较小 | 仓库需属于 GitHub Organization；供应商与镜像差异需验证 | 7 天试用跑真实冷/热候选各 3 次 |
| WarpBuild | 多规格 runner、兼容 Actions 生态 | Windows cache 支持边界需按当前文档核实 | 只在 Windows 实测胜过官方方案时进入候选 |
| 自托管 Windows | 持久 warm cache、硬件可控、可能最快 | 安全隔离、维护、可用性、密钥和公开 PR 风险最高 | 仅可信 push/tag；优先 ephemeral VM，不直接暴露开发机 |

`BuildJet` 不进入候选清单：其 GitHub Actions runner 服务已宣布于
2026-03-31 停止。

选择不是“最快一次”，而是：

```text
端到端 p50/p95
+ 排队时间
+ 每候选成本
+ 缓存冷启动
+ 六平台可用性
+ 故障率与诊断
+ 权限/供应链风险
+ 一行回退到 github-hosted 的能力
```

## 六、native IPC 与实例收敛

本轮不再新增第三种实例语义，集中把 `main|dev` 做实：

进入本节的代码结构前置已经收口：revision-4 Platform Facade 是生产原生
能力的唯一边界，IPC endpoint/transport 通过 typed contract、service、
selected adapter 装配；遗留的 Unix socket / Windows named-pipe 实现副本已
删除。这里剩余的是三平台原生运行证据与混合版本行为，不是再次创建平台分支。

- 在真实 Windows/macOS/Linux 上并发启动同 role，恰有一个 authority；
- 不同 role 的 endpoint、registration、workspace、settings、epoch 严格隔离；
- Unix socket 父目录 owner/mode、socket mode、symlink 拒绝与路径长度均有
  native evidence；macOS `/tmp` 与 `/private/tmp` canonicalization 不造成
  同一 authority 的双重身份；
- Windows named pipe DACL、local-only、竞争创建和 stale registration
  有 typed diagnostics；
- schema-v1/v2 混合发现时，typed endpoint 优先；legacy handshake 不因
  `tcp:` 表达差异误判 stale；
- `server-list` 区分 live、unreachable、stale-test-fixture，并提供安全、
  显式、可审计的 stale cleanup，而不是自动 kill 不确定 PID；
- GUI、CC、CLI、MCP、Mux 和 Script 使用同一 selector/resolver 表面。

## 七、三平台 GUI 与 Control Center 收敛

### 7.1 主工作台

Windows/remote Windows/Unix 的窗口、render、input 与 wake 实现已经物理归属
selected adapters；产品层不再选择 winit/softbuffer/Win32 或 PTY backend。
本节只继续收敛用户可见的跨平台行为与原生证据。

- 对齐 toolbar 顺序、`En|Zh`、字号动作、Tabs 双击编辑、tree lines、
  Composer 多行输入、scrollbar、selection、clipboard 和 no-activate；
- snapshot 的 semantic ID、bounds、visibility、focus 与 renderer PNG
  一致；高 DPI/Retina 使用 logical 与 physical 尺寸双事实；
- 平台适配器只拥有 OS 机制，产品动作与状态继续来自共享层；
- Linux/macOS 不为了“看起来相似”复制一套漂移的产品状态机。

### 7.2 Control Center

本版接受的首个深化仍是只读 Cockpit：

```text
Cockpit
├─ selected logical instance / endpoint transport
├─ server PID / build / protocol / health
├─ epoch / sequence / journal gap state
├─ tabs: total / running / dead / detached
├─ selected tab identity and health
└─ component capability / degraded reason
```

- open/focus/no-activate 必须选择调用者同一实例，不隐式启动另一 server；
- macOS/Linux 补齐真实 renderer-owned screenshot，而不是替代图或
  “capability available”假状态；
- 覆盖 incompatible sibling、renderer crash、server retained while GUI
  replaced、new epoch recovery 和 stale owner replacement；
- Workflows、Extensions、InfoHub 可以改进解释与导航，但没有 owning backend
  前继续显示真实 empty/unavailable，不造假数据。

## 八、`agenterm-net` N2 受控纵切

本版不把网络愿景继续停留在“研究”字样，但也不把一个能连网的二进制误称为
可公开运行的 IPFS 节点。交付对象是独立 `agenterm-net`：用户显式启动、显式
停止、可检查、可清理；它可以常驻，但安装、打开 GUI 或启动 server 均不会隐式
启动它。`agenterm.exe`、`agenterm-server`、`agenterm-cc` 不链接 libp2p。

```text
N2-M1：可控 full-node foundation
├─ identity / store
│  ├─ ephemeral 与 durable 身份显式二选一；备份/轮换/丢失诊断留痕
│  ├─ 有界 persistent block store：put/get/verify、pin、GC、损坏隔离
│  └─ node state、listener、peer、disk/RSS/连接数均为 typed snapshot
├─ mesh capabilities
│  ├─ Kademlia DHT：bootstrap / provide / find-provider（默认关闭公网 bootstrap）
│  ├─ GossipSub：具名 topic、消息大小/速率/队列上限、receipt
│  └─ relay：client 与受控 relay role 分离；不自动替用户公开 relay
├─ remote Fleet attach（只读）
│  ├─ 用户显式创建 pairing invite，绑定 peer identity、expiry、scope 与 nonce
│  ├─ 远端只投射 bounded Fleet snapshot / event digest；没有 shell、PTY 输入或控制动作
│  ├─ 双端签名/加密与 replay / wrong-peer / expired-invite 拒绝
│  └─ attach 断线、重连、node crash 的结果真实且不影响任一 local server
└─ evidence
   ├─ 两进程、两持久身份、DHT/pubsub/relay/attach 的 deterministic private-mesh fixture
   ├─ 无固定 sleep；超时、取消、kill、corrupt store、budget exhaust 均有 typed receipt
   ├─ Windows/Linux/macOS 的独立启动、停止、残留 listener/child 检查
   └─ package / SBOM / licence / binary and resource delta 先测量再决定稳定资产资格
```

边界：N2-M1 允许私有测试网和用户明确配置的监听地址；不默认连公共 bootstrap，
不自动 NAT 打洞，不承诺 Kubo API 兼容，不开放公网 Fleet control。真正“远程控制”
须另有 Agent/harness 的审批与凭据模型，不能借由网络 attach 绕过。

## 九、系统 WebView / Tauri-compatible spike

先以一个独立的 `agenterm-cc-web` 实验宿主验证系统 WebView，而不是把 Tauri
塞进主 GUI；它也不替换既有 native `agenterm-cc`。它是未来独立应用
（Control Center 的可选扩展视图、PluginHub、InfoHub、Workflow 等）可复用的
宿主技术储备，加载仓库内打包的静态 HTML/CSS/JS，首页只读展示 Cockpit。主产品
模型、Fleet authority 和业务逻辑仍在 Rust 侧，Web UI 只是并列投射。

```text
Web host M1
├─ implementation choice
│  ├─ 先做最小 Tauri v2 spike，并记录其 Rust/JS toolchain、lockfile 与 build-time 影响
│  ├─ 同时保留 direct-WRY 作为可比较备选，不预设 Tauri 必然胜出
│  └─ 不把 Node、前端 framework 或网络页面引入 core build path
├─ local-only surface
│  ├─ versioned packaged asset manifest + integrity hash + custom local origin
│  ├─ host.ready / host.facts / fleet.snapshot 三个 bounded typed bridge call
│  ├─ origin / main-frame / nonce / request-id / deadline 严格匹配
│  └─ no eval / shell / process / arbitrary navigation / download / network bridge
├─ platform evidence
│  ├─ Windows: installed WebView2 与 missing-runtime fallback；不 bundling fixed runtime
│  ├─ macOS: WKWebView local asset, Retina screenshot, crash/reload/fallback
│  └─ Linux: WebKitGTK availability/package diagnostic, renderer PNG or explicit unavailable
└─ size and performance decision
   ├─ measure: binary, archive, installer/runtime dependency, cold/warm startup, RSS and first paint
   ├─ compare: native CC baseline vs direct-WRY vs Tauri experiment on each native platform
   ├─ publish machine-readable receipt and threshold decision
   └─ promote only if fallback/isolation/security and six-target packaging all stay truthful
```

Tauri’s own model validates the system-WebView premise: it uses WebView2 on
Windows, WKWebView on macOS and WebKitGTK on Linux, dynamically linking the
system engine rather than embedding it in the executable. But packaging policy
matters: a Windows fixed WebView2 runtime alone can add about 180 MiB, so this
spike explicitly measures **installed-runtime / fallback** first and does not
bundle a browser. [Tauri process model](https://v2.tauri.app/concept/process-model/),
[Tauri Windows runtime options](https://v2.tauri.app/distribute/windows-installer/).

## 十、并行实施波次

```text
Wave A（共享合同，可并行）
├─ A1：workflow timing + duplicate-work audit
├─ A2：candidate/promotion schema 与 threat model
├─ A3：native IPC mixed-version/stale matrix
└─ A4：Control Center/GUI parity gap matrix

Wave N（网络纵切，与 UI/IPC 实现并行）
├─ N1：N2 node lifecycle、identity/store 和 typed local protocol
├─ N2：DHT/pubsub/relay private-mesh capability fixtures
├─ N3：只读 Remote Fleet attach pairing/snapshot contract
└─ N4：跨平台 resource/fault/isolation evidence 与 package decision

Wave W（Web host，与 network/IPC 实现并行）
├─ W1：Tauri v2 / direct-WRY dependency、toolchain、license 与 baseline measurement
├─ W2：local packaged Cockpit + bounded bridge + native fallback
├─ W3：three-platform runtime/PNG/crash/activation evidence
└─ W4：binary/archive/runtime/startup/RSS receipt 与 adopt/defer decision

Wave B（实现，可并行）
├─ B1：Fast CI 与 Candidate workflow 拆分
├─ B2：cache 基线与正确 key
├─ B3：Windows pipe / common resolver 收敛
├─ B4：macOS Unix socket + CC native evidence
└─ B5：Linux Unix socket + CC native evidence

Wave C（集成）
├─ exact-SHA candidate aggregate
├─ promotion workflow 与 fail-closed fixtures
├─ 三平台 mixed-version / no-activate / orphan matrix
└─ paid-runner A/B（不得阻塞免费默认路径）

Wave D（候选）
├─ clean main
├─ one complete qualification
├─ six-platform candidate assets
├─ non-publishing promotion rehearsal
└─ 用户批准后才 tag / Release
```

共享 workflow、artifact schema、endpoint resolver 与 PRD 文件由主代理协调，
平台代理优先提交自己 adapter、native fixture 和证据，避免同时大改同一
共享文件。每个小而完整的进展 review 后尽快进入 `main`，让其他平台及时
rebase 和验证。

## 十一、完成定义

- `plan` 中接受的能力均已同步 owning PRD，状态不靠版本号猜测；
- `main` clean、已推送，普通 CI 六目标通过；
- exact-SHA candidate workflow 产出完整 qualification receipt 和六平台资产；
- tag promotion 不执行 Cargo build，不重跑完整 GUI/stress suite；
- promotion 对错 SHA、错 tag、缺 receipt、缺平台包、篡改 hash、过期 artifact
  和不完整 matrix 全部 fail closed；
- Linux/macOS/Windows 的 native IPC 与 Control Center 关键 journey 有各自
  原生证据；
- `agenterm-net` 的 N2-M1 如在本版本宣称完成，必须有私有两节点 DHT/pubsub/
  relay/只读 attach、持久身份/store、资源与故障隔离的跨平台证据；未达此门则保持
  experimental，不能进入 stable asset 或远程控制面；
- system-WebView spike 必须给出 native CC、direct-WRY 与 Tauri 的可复核体积及
  启动/RSS 对比、三平台 runtime/fallback 证据和 bridge isolation 测试；否则只保留
  renderer-neutral 合同，不把 Web UI 宣称为 production renderer；
- 没有新增窗口风暴、固定 sleep、测试残留 server/socket/pipe 或隐式
  foreground activation；
- 付费 runner 即使试验失败，也可通过一处 label/config 回退，不影响免费
  github-hosted 正确路径；
- 未经用户最后明确批准，不创建 `v0.1.12` tag 或 GitHub Release。
