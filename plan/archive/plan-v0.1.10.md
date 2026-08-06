# AgenTerm v0.1.10 公开计划

> ## ⚠️ 已归档（2026-08-05）
>
> **本文是 v0.1.10 时期的历史执行记录，保留仅为追溯，不要作为执行依据。**
> 其里程碑证据仍被 `prd/PRD_02_18_roadmap.md` 引用，故整档保留原文未改。
>
> - 上一已发布版本复盘：`plan/archive/plan-v0.1.14.md`；在制：`plan/plan-v0.1.15.md`
> - 在制版本：`plan/plan-v0.1.15.md`
> - 发布链要求（版本无关权威处）：
>   `prd/PRD_02_17_delivery_quality.md` §Release-chain operating requirements
> - 结构 SSOT：`plan/ARCHITECTURE.md`


状态：已完成内部交付与候选演练；未创建 v0.1.10 tag 或公开 Release；
原 v0.1.9 Release 因 server-loss 故障撤下，
`0.1.9+hotfix.1` 是恢复 v0.1.10 主线前的强制稳定性门；
43/43 PowerShell 迁移项完成删除；Windows batch 业务逻辑迁移完成
工作主题：**Rhai 完整接替仓库脚本业务逻辑、建立可验证的只读 Agent
桥梁，并收敛三平台原生适配边界**
版本定位：在 v0.1.9 完善通用 Rhai 运行时、模块任务与机器可读工具
schema 后，让 AgenTerm 首次用自己的脚本运行时驱动完整开发生命周期，
同时把同一份 Fleet 事实稳定地开放给外部 Agent 客户端。

### 三平台发布收口补充（2026-07-31）

v0.1.10 的公开发布不只验证“六个 target 能编译”，还要证明 Windows、
macOS、Linux 的真实 GUI 热路径消费可观察、可复用、不过度抽象的
`platform/` 合同：

```text
platform/ 发布闭环
├─ 共享产品语义
│  ├─ action ID 与工具栏顺序
│  ├─ committed text / shortcut 分类
│  ├─ window lifecycle 与 client-size 边界
│  ├─ logical / physical scale 与 geometry
│  └─ screenshot clip 与 typed capability status
├─ 三平台原生适配
│  ├─ Windows：Win32 / GDI / ConPTY
│  ├─ macOS：Cocoa-winit / Retina / POSIX PTY
│  └─ Linux：X11-Wayland-winit / headless / POSIX PTY
├─ 自反馈
│  ├─ protocol-info 公开 adapter、contract revision、八类 capability
│  ├─ Unsupported/Failed 不伪装 Available
│  └─ snapshot 与 PNG 对齐
└─ 发布门
   ├─ Windows/Linux/macOS × x86_64/ARM64 CI
   ├─ Windows 完整公共接口与 no-activate GUI smoke
   ├─ build identity 对应干净且准确的 commit
   └─ Windows/Linux stable 与 macOS unsigned preview 清晰分流
```

共享层只拥有跨投射仍成立的语义、数据、校验和错误合同；Win32、Cocoa、
X11/Wayland 的句柄、事件循环、渲染和系统调用继续留在各自 adapter。
不为了目录整齐制造空 trait，不把 server 状态迁入 GUI 平台层，也不以
“最低共同能力”掩盖某个平台尚未交付的功能。公开 tag 只能在
`prd/PRD_02_20_native_platform.md` 的完成门、完整本地资格门和最新
六平台 CI 同时有真实证据后创建。

### v0.1.9.1 紧急稳定性门（2026-07-29）

用户现场出现旧 GUI 仍响应但 server、监听端口和 instance registration
全部消失的断裂状态。失效的 `UiClientModel` 仍被保留，造成界面继续呈现旧
终端并接受输入；关闭入口又在打开本地确认前尝试同步 composer，最终表现为
“终端卡死且窗口无法关闭”。

恢复 v0.1.10 功能开发前必须发布一次窄范围热修复：

```text
server 消失
├─ 立即撤销 stale client projection
├─ 隐藏/禁用 terminal composer 与 server-owned controls
├─ 显示 Offline / Recovering，不伪装 Connected
├─ 窗口关闭与 Keep Server Running 始终由 GUI 本地完成
├─ endpoint 无 listener 时限频启动 replacement server
├─ endpoint 有 listener 但握手失败时拒绝竞争启动
└─ 同一 GUI PID/HWND 连接新 server PID/epoch/lease
```

Cargo 不接受四段核心版本 `0.1.9.1`，因此机器身份、tag、metadata 和制品采用
严格一致的 SemVer `0.1.9+hotfix.1` / `v0.1.9+hotfix.1`；GitHub Release
标题使用人类可读的 “AgenTerm v0.1.9.1 Hotfix”。旧 `v0.1.9` tag 保留，
不得覆盖或复用。

热修复发布门：

- `remote-ui-smoke` Rhai 任务必须真实停止 server，观察输入 controls 隐藏；
- 在 server 不可用时通过原生 `WM_CLOSE` 打开并取消本地三选确认；
- 测试不得手工启动 replacement server，必须由同一 GUI 自动恢复；
- 新 server PID、epoch、UI lease 必须全部变化且因果一致；
- cleanup 必须 zero-orphan，测试期间不得新增 WER crash；
- 运行中 GUI/server 与下一版 staged binaries 的升级/回滚旅程必须通过；
- 完整 release gate 通过后才能创建热修复 tag/Release。
- Apple 分发凭据恢复前，macOS 仅允许发布显式
  `-unsigned-preview` 资产；ZIP 内、独立 Release 文件和页面正文三处都必须
  标明未签名/未公证、SHA-256/provenance 验证和一次性
  `Privacy & Security → Open Anyway` 路径，禁止用 stable 文件名或建议全局
  关闭 Gatekeeper。

首次 `v0.1.9+hotfix.1` tag workflow 没有生成 Release：两个 macOS jobs
暴露 `package-client-release.sh` 的 heredoc/process-substitution 组合在
runner 上解析失败；Windows release gate 则在设置和 PTY 已完成 resize 时
读取到一帧旧 client geometry。修正合同为：平台清单先写入显式 staging
TSV，再由 shell 读取；Tabs resize 旅程使用公开 `wait-ui
--terminal-grid-changed-from` 有界等待。修正后的源提交和 tag 必须一致，
不得把失败 tag 的结果冒充为已发布热修复。

本版不可拆分的第一交付目标是 **PowerShell 归零**。MCP 是可并行推进的
第二产品线；若资源、时间或共享热点发生冲突，先保证 Rhai 自举、迁移与
归零闭环，缩减 MCP 表面而不是保留 PowerShell 尾巴。

本文是版本执行计划与决策记录，不是产品事实，也不得成为实现依赖。
评审接受的能力、边界和验收条件必须同步进入对应 `PRD.md` 模块；完成后
保留本文作为交付历史。

### 已冻结的版本决策（2026-07-29）

v0.1.10 必须完成 Rhai 对仓库自有 PowerShell 自动化的替代，这不是尽力而为
的候选项，也不能顺延为后续版本：

- 截至 2026-07-29 的发布前滚动快照为 **43 个受 Git 跟踪的 `.ps1`**：
  根目录 3 个、`scripts/` 活跃脚本 17 个、
  `scripts/archive/powershell/` 2 个、`tests/` 21 个；这只是便于评审的
  当前快照，v0.1.10 开工时必须由
  `git ls-files '*.ps1'` 自动生成并冻结真实基线，v0.1.9 收口期间新增并
  最终纳入 Git 的脚本同样必须迁移；
- v0.1.10 开工提交冻结最终迁移清单；从该提交起，zero-PS1 漂移门既拒绝
  新增 `.ps1`，也拒绝从清单中遗漏、改名规避或把 PowerShell 藏进字符串；
- 完成时 `git ls-files '*.ps1'` 必须返回空结果，archive 不作为例外；
- Rhai task graph、`.bat` 薄入口、CI workflow 和发布链路不得再启动
  `powershell.exe` / `pwsh.exe`，也不得通过命令字符串、临时脚本或下载
  脚本变相保留 PowerShell 业务逻辑；
- 已迁移实现由 Git 历史保存，不在活动工作树维护 PowerShell 影子副本；
- `.bat/.cmd`、Unix shell 和 CI YAML 可以作为平台薄入口，但不得包含业务
  规则；v0.1.10 进一步把现有四个 Windows batch 入口的构建、清理、测试、
  资格与发布编排迁入 Rhai，只保留一个通用 stage-0 bootstrap 机制和可选的
  一行式人类入口；
- 如果某个迁移暴露 Script Runtime 能力缺口，先补稳定 typed API，再继续
  迁移；不得从 Rhai 反向调用 PowerShell 绕过缺口；
- MCP 是本轮并行产品线，但不得以它为理由降低 PowerShell 归零完成门；
- 若版本时间受限，先缩减 MCP 的非核心表面，不缩减 Rhai 自举与归零目标。

### “替代 PowerShell”的范围边界

这里的“替代”是一个可测量的仓库与交付合同，不是对 Windows 平台能力的
禁用：

```text
必须归零
├─ 受 Git 跟踪的仓库自有 .ps1
├─ build / lint / test / qualification / package / release 中的 PowerShell 调用
├─ Rhai、batch、CI YAML 中隐藏或转写的 PowerShell 业务逻辑
└─ 仅为回退而保留在活动树中的 PowerShell 影子实现

允许保留
├─ 用户在 AgenTerm 终端里主动运行 powershell.exe / pwsh.exe
├─ 对 PowerShell shell 行为的终端兼容性测试（由 Rhai harness 驱动）
├─ Git 历史中的旧实现
└─ 不承载产品规则的最薄平台 bootstrap；Windows 默认入口为 .bat，非 .ps1
```

因此进度不按“已经写出多少 `.rhai` 文件”计算，而只按完成闭环的迁移项
计算：Rhai parity 证据通过、全部 caller 已切换、对应 `.ps1` 已删除、漂移
门已覆盖。最终同时检查静态仓库清单和动态进程树，防止“文件归零、运行时
仍偷偷启动 PowerShell”的假完成。

## 〇、产品判断

AgenTerm 不需要通过堆叠可见按钮与竞品竞争。v0.1.10 延续以下准绳：

> 界面简单实用，软件稳定可靠，编程接口丰富，并为扩展保留足够空间。

这一轮的能力主要出现在工具链与接口层，而不是 GUI：

- 默认工作台不增加 MCP 面板、连接列表、授权弹窗或常驻状态动画；
- `agenterm-rhai.exe` 接替仓库自有 PowerShell 脚本的业务编排职责；
- 构建、lint、测试、资格认证和发布使用同一套 Rhai task graph；
- `agenterm.exe` 继续只负责窗口、终端、标签与 Fleet authority；
- `agenterm-mcp.exe` 是按需启动、可独立退出的 stdio sidecar；
- 首个版本只提供只读资源与一个有界等待工具；
- “Agent 可以看见并等待真实状态”先于“Agent 可以修改状态”；
- 自然语言回答永远不是成功证据，机器可读的 epoch、sequence、stable ID
  和 post-state 才是。

v0.1.10 不自动继承 v0.1.9 的所有未完成想法。任何 carry-over 都必须重新
说明用户价值、依赖、证据和版本必要性，否则留在原模块或未来计划中。

## 一、版本目录树

```text
v0.1.10  Rhai 完整接替 PowerShell 与可验证的只读 Agent 桥梁
│
├─ 最高优先级：Rhai 完整接替仓库 PowerShell
│  ├─ 建立全部 .ps1 入口、职责、调用者、输入输出和副作用清单
│  ├─ 公共 helper 下沉为稳定 Rhai modules，不逐文件机械翻译
│  ├─ build / lint / test / qualification / package / release 全部迁移
│  ├─ 每项先双跑比较，再切换唯一入口并删除对应 .ps1
│  ├─ agenterm.tasks.json 成为开发任务唯一机器可读目录
│  ├─ 禁止 Rhai / CI / 薄入口反向调用 powershell.exe 或 pwsh.exe
│  └─ 完成门：仓库自有 .ps1 与 PowerShell 执行依赖均为零
│
├─ 最高优先级：可自举且跨平台的开发入口
│  ├─ stage 0 仅定位 Rust 工具链并构建 agenterm-rhai.exe
│  ├─ stage 1 起全部业务判断交给 Rhai task
│  ├─ build.bat 只保留 Windows 薄入口，不承载测试/发布逻辑
│  ├─ CI 只负责环境准备和调用同名 Rhai task
│  ├─ Windows/Linux/macOS 共用任务语义，平台差异进入 typed adapter
│  └─ 干净 checkout、无 PowerShell 环境仍可构建、测试和生成资格收据
│
├─ 最高优先级：agenterm-mcp.exe 公共入口
│  ├─ --help / --version / capabilities --json 完全离线
│  ├─ serve --stdio 是唯一首发 transport
│  ├─ 固定并公开 MCP protocol revision 与 AgenTerm schema 版本
│  ├─ stdout 只写 MCP JSON-RPC，诊断只写 stderr
│  └─ 明确选择 server；多实例歧义时失败并列出候选
│
├─ 最高优先级：只读 Fleet resources
│  ├─ instance inventory
│  ├─ workspace inventory
│  ├─ tab inventory
│  ├─ one causal fleet snapshot
│  ├─ stable ID + epoch + sequence + schema identity
│  └─ 默认不暴露 pane text、Composer、环境值或 secret
│
├─ 最高优先级：唯一的只读等待工具
│  ├─ tools/list 只公布 agenterm_wait
│  ├─ 输入为 epoch、after sequence、allowlisted predicate、timeout
│  ├─ 返回匹配事件、新位置和可验证 post-state identity
│  ├─ restart / gap / timeout / cancel / target closed 分型
│  └─ disconnect、取消与超时后无残留 waiter
│
├─ 最高优先级：协议与故障隔离
│  ├─ initialization、capability negotiation、initialized、ping
│  ├─ UTF-8 newline-delimited stdio JSON-RPC
│  ├─ frame、并发、等待、输出和错误详情全部有硬上限
│  ├─ malformed peer、oversize、sidecar crash 不影响 GUI/PTY/server
│  └─ sidecar 重启从新 snapshot 恢复，不伪造连续性
│
├─ 第一优先级：同源 typed adapter
│  ├─ 复用公共 operation/event/snapshot contracts
│  ├─ MCP 不解析 CLI 人类文本，也不读取 Win32 私有状态
│  ├─ resource/tool schema 由一个 typed catalog 驱动
│  ├─ 保留 v0.1.9 domain/group/callable 层级，不重新压平成另一份清单
│  ├─ unavailable 能力显式可发现，不静默消失
│  └─ 为后续 Rhai control 与 agenterm-agent.exe 保留复用边界
│
├─ 第一优先级：消费组件事实，不承担组件管理
│  ├─ 复用 v0.1.9 的 version/capability/availability 描述语言
│  ├─ capabilities 解释缺失、不兼容和 degraded role
│  ├─ 不把 package inventory、下载、安装或升级变成 MCP 首发资源
│  └─ 为未来 softmgr/市场/desktop 工具保留 typed adapter 边界
│
├─ 第一优先级：自反馈与兼容资格
│  ├─ Rhai 黑盒 harness 能启动、观察、断言并清理 GUI/PTY/server
│  ├─ 迁移前后对同一 fixture 生成可比较的规范化证据
│  ├─ 原始 JSON-RPC 黑盒覆盖完整生命周期
│  ├─ MCP resource 与 agenterm-cli 同时读取并逐字段比较
│  ├─ 外部 CLI 触发事件，MCP wait 只观察并返回证据
│  ├─ restart / gap / cancel / crash / malformed / privacy 故障矩阵
│  ├─ no-activate、首窗口、二进制大小和 orphan 门不回退
│  └─ 失败保留有界诊断包，成功清理全部测试资源
│
├─ 第一优先级：公开使用体验
│  ├─ 最小配置示例和五分钟只读接入旅程
│  ├─ capabilities --json 解释当前能力与明确不可用能力
│  ├─ 错误给出 server address/session/epoch 诊断但不泄密
│  ├─ README 保持简短，详细协议契约进入 PRD
│  └─ 发布仍消费同一份合格字节并需要用户明确批准
│
└─ 明确延后与未来计划
   ├─ create/send/close/kill/shutdown 等 MCP control tools
   ├─ agenterm-agent.exe、审批 UI、角色与 agent 权限系统
   ├─ MCP client/federation、网络 transport 与远程监听
   ├─ resource subscriptions、prompts、sampling、elicitation 与 experimental tasks
   ├─ pane text/content resource 和默认终端内容暴露
   ├─ MCP 调用 Rhai、Rhai event handlers、brain/flow 与 durable scheduling
   ├─ fleet-wide proxy、持久 proxy profile 与 secret 分发
   ├─ agenterm-net.exe、libp2p/IPFS 和去中心化应用
   ├─ agenterm-mux.exe 原生 mux server、完整 pane 与多后端
   ├─ agenterm.exe 与 agenterm-cli.exe 单文件合并
   ├─ agenterm-rhai.exe 完整 Node/Bun 级标准库的剩余扩展
   ├─ agenterm-softmgr.exe、签名包/应用市场与联网软件分发
   ├─ agenterm-desktop.exe companion 与可选 Shell Replacement
   └─ 安装器、自动更新、联网组件安装与未单独批准的公开发布
```

## 二、北极星演示

v0.1.10 必须能够通过以下两条完整旅程解释自身价值。

### 旅程 A：Rhai 驱动自身交付

```text
一个干净 checkout，没有依赖仓库 .ps1
  -> build.bat / CI 完成最小 stage-0 bootstrap
     -> agenterm-rhai.exe task run check
        -> Rhai task graph 执行 fmt、lint、unit 与公共黑盒
           -> GUI 测试保持 no-activate，并通过 typed wait 获取结果
              -> agenterm-rhai.exe task run qualify-release
                 -> 生成绑定 commit、源码状态和制品 hash 的资格收据
                    -> package/release 只消费这批已验证字节
                       -> 全程无 PowerShell 业务逻辑和残留测试资源
```

该旅程必须证明：

- 从干净 checkout 到 release candidate 不执行任何仓库自有 `.ps1`；
- 开发机与 CI 调用同名 task，任务图、默认值、错误分类和清理语义一致；
- 失败可定位到稳定 task/step/evidence ID，修复后可只重跑安全子树；
- stage 0 不复制 lint、测试、打包或发布判断，只解决自举循环。

### 旅程 B：只读 MCP 桥梁

```text
一个真实 AgenTerm server 正在运行
  -> MCP client 启动 agenterm-mcp.exe serve --stdio
     -> initialize 协商成功
        -> resources/list 只看到声明的只读资源
           -> resources/read 读取 tabs 与 fleet snapshot
              -> client 调用 agenterm_wait 等待 tab.note 事件
                 -> 人或独立 agenterm-cli 修改一个标签注释
                    -> MCP 返回匹配事件、stable tab ID 和新 position
                       -> client 再读 snapshot，post-state 与事件一致
                          -> client 关闭 stdin
                             -> sidecar 有界退出，无 waiter / process / GUI 残留
```

演示必须同时证明：

- MCP 自己没有执行那次修改；
- MCP 与 CLI 看到同一个 server epoch、event sequence 和 stable tab ID；
- pane text、Composer、proxy URL、环境值和凭证没有进入 MCP 输出；
- sidecar 被强制结束时，AgenTerm server、窗口、PTY 与标签继续正常工作。

## 三、进入条件与完成定义

### 进入条件

开始主实现前必须确认：

1. v0.1.8 候选的普通资格门全绿，专业选择、Tabs、proxy 和 no-activate
   不存在未归属的 P0/P1 回退。
2. Observable Fleet 已证明 snapshot-to-follow、epoch restart、journal gap、
   bounded wait 和 waiter cleanup。
3. operation、event、protocol feature 和 evidence catalog 继续通过漂移检查。
4. MCP 所需的数据全部可从公共 IPC/typed adapter 获得；不能为了赶进度
   读取 `AppState`、HWND 或 renderer 私有字段。
5. 先冻结首发 MCP 方法、resource URI、tool schema、错误分类和预算，再
   并行写 transport、adapter 与测试。
6. v0.1.9 Script API、task manifest、process/fs/env/time/JSON、typed error、
   cancellation、temporary ownership 和 atomic result 已足够承载首批迁移；
   缺口必须先补入 runtime/catalog，不允许在 Rhai 文件中调用 PowerShell
   绕过。

### 完成定义

v0.1.10 public-ready 必须满足：

- 仓库自有、可执行 `.ps1` 文件为零；
- 从 bootstrap 到 release rehearsal 的进程树中不存在 PowerShell；
- build、check、test、qualification、package、release 均有公开 Rhai task；
- `agenterm.tasks.json` 可离线列出上述 task、依赖、平台和副作用分类；
- 干净 Windows checkout 在不要求 PowerShell 的条件下完成 release
  qualification，Linux/macOS 使用相同任务语义；
- CI、开发入口与 release workflow 不复制 Rhai 中的业务规则；
- 迁移过程每删除一个 `.ps1` 都有等价或更强的公共证据；
- 一个新发布制品 `agenterm-mcp.exe`；
- 一个 stable MCP protocol revision；
- 四类只读资源；
- 一个且只有一个 `agenterm_wait` tool；
- 零 MCP mutation tools；
- 零网络 listener；
- 零默认 pane/content 暴露；
- 完整公共 JSON-RPC、故障隔离、隐私和 orphan 证据；
- PRD、capability catalog、README、构建清单和发布资产完全对齐；
- 普通资格和 clean release qualification 均通过；
- 是否创建 tag/Release 仍由用户单独批准。

## 四、Rhai 接替 PowerShell 的迁移合同

### 迁移范围

“完整接替”指仓库拥有并执行的 PowerShell 逻辑全部退出主线，包括：

```text
build
├─ dev / release build
├─ artifact copy、size budget 与 locked cleanup
└─ target cache / qualification evidence retention

quality
├─ fmt / Clippy / lint / unit
├─ PRD、catalog、manifest、SBOM 与文档对齐
└─ source-dirty、commit identity 与 byte identity

black-box
├─ startup / no-activate / UI / terminal / Fleet
├─ script / MCP / mux / compatibility
├─ stress / fault injection / privacy / orphan cleanup
└─ screenshot、structured snapshot 与首错诊断包

delivery
├─ qualification receipt
├─ package / checksum / SBOM
├─ non-publishing release rehearsal
└─ tag / push / publish 前置验证
```

基线迁移批次按依赖方向执行：

```text
波次 A：纯规则与报告（低副作用）
  2 个 archive + build identity + target/artifact/version/report helpers
    -> 稳定 fs/env/json/process API
    -> 规范化结果逐字段对比

波次 B：构建、lint 与静态对齐
  root lint/check 编排 + scripts build/stage/supply-chain
    -> Rhai task graph 成为唯一规则源
    -> build.bat/CI 退化为 stage-0 薄入口

波次 C：公共黑盒测试
  当前 21 个 tests/*.ps1（含测试 helper/manifest/fixture server）
    -> 共享 Rhai harness
    -> typed wait、诊断包、资源所有权与清理
    -> 每个 evidence ID 等价或增强后删除原脚本
    -> 已删除从未接入 smoke/check/qualification 的 JourneyManifest 原型；
       机器可读 step manifest 由共享 Rhai harness 正式实现，不保留平行模型

波次 D：资格、打包与发布
  qualification/package/public-candidate/release
    -> receipt 绑定 commit、task graph、runtime 与 artifact hash
    -> package 不重建，release 保留显式用户批准
    -> 已删除无调用者且硬编码 v0.1.8 的旧 public-candidate policy

波次 E：归零与防回流
  删除最后一个 .ps1
    -> 文档、AGENTS、CI、Cargo metadata 和所有调用者切换
    -> zero-ps1 lint gate
    -> Windows clean checkout 无 PowerShell 资格测试
```

要求删除所有受跟踪 `.ps1`；冻结基线中
`scripts/archive/powershell/` 的两个历史副本已在首批迁移提交删除，其内容
只由 Git 历史继续保存。不机械要求删除 `.bat/.cmd`、shell 或 CI YAML，
但它们不得保留任务选择、依赖、预算、构建、清理、测试、打包或发布规则；
可删除的入口应删除，必须保留的 Windows 入口只能透明启动同一个通用
stage-0 bootstrap
这些平台入口，但它们只能：

1. 定位或安装明确版本的 Rust 工具链；
2. 构建/定位 `agenterm-rhai`；
3. 把参数和退出码原样转交给公开 Rhai task。

入口不得包含能力选择、测试清单、预算、制品判断、发布条件或清理策略。
Git 历史就是已迁移 `.ps1` 的归档，不在活动树保留第二套实现。

### 迁移台账与进度口径

v0.1.10 开工时由公开 Rhai task 从 `git ls-files '*.ps1'` 生成并冻结迁移
台账。台账不是第二份手写文件清单；它必须记录并校验：

```text
源脚本路径
├─ stable migration ID
├─ 职责与所有调用者
├─ 输入、输出、副作用、预算与平台条件
├─ 原 evidence ID / fixture
├─ 对应 Rhai module / task / typed API
├─ parity evidence identity
└─ 状态：inventory -> parity -> cutover -> deleted
```

进度只按“已经切换全部调用者并删除源 `.ps1`”计算完成；只有 Rhai
实现、仍保留 PowerShell 主入口的项目不得计入完成率。每完成一个台账项，
必须在同一小提交中完成调用者切换、等价或更强证据、源文件删除和目录漂移
校验。若 v0.1.9 在 v0.1.10 开工前继续增加 PowerShell 测试，最终冻结基线
按真实 Git 清单自动上调，归零目标不变。

迁移采用“完成一个、归档一个”的短闭环，不等待全部 Rhai 实现完成后再
集中切换：

```text
一个可独立验收的脚本或强耦合脚本组
  -> 冻结原行为与证据
     -> 补齐共享 typed API / Rhai module
        -> 对同一 fixture 双跑并比较
           -> 切换所有调用者
              -> 删除对应 .ps1
                 -> 小提交并更新自动台账
```

这里的“归档”表示从活动工作树删除，由 Git 历史保存；不得移动到新的
`archive/` 目录继续维护。只有无法独立切换的共享 harness/fixture 才允许
以一个有明确边界的脚本组迁移，不能用“仍有其他脚本依赖”为理由长期双轨。

### 迁移方法

每个脚本按同一状态机推进：

```text
inventory
  -> extract contract
     -> fill typed runtime/API gap
        -> implement Rhai module/task
           -> dual-run same fixture
              -> compare normalized evidence
                 -> switch every caller
                    -> delete .ps1
                       -> add no-regression inventory gate
```

禁止逐行翻译 PowerShell。重复的进程、环境、文件、JSON、等待、诊断和清理
逻辑必须进入 `agenterm-rhai` 的 typed API 或共享 Rhai module。平台差异由
明确的 adapter/capability 表达；脚本不得通过 shell 字符串拼接模拟 argv。

### Task graph

建议冻结以下公开任务面，最终名称以 catalog 为准：

```text
bootstrap-info
build-dev
build-release
lint
test-unit
test-smoke
test-stress
check
qualify-release
package
release-rehearsal
release
clean
```

task 必须公开 stable ID、依赖、平台、输入、产物、副作用、预算、是否允许
联网以及 evidence ID。`release` 是唯一可修改远端的任务，必须要求用户明确
批准；其它任务不得因迁移而扩大外部权限。

### 自举与失败恢复

- stage 0 使用 Cargo 只构建 `agenterm-rhai` 及必要共享库；
- stage 0 失败输出工具链/编译错误并原样返回非零退出码；
- stage 1 由 Rhai 校验 runtime/build identity 后接管 task graph；
- runtime 源码变化时自动重建一次，禁止递归自举；
- task 失败保留有界、脱敏的 step/evidence 诊断；
- cleanup 在成功、失败、取消、超时和父进程退出时都执行；
- qualification receipt 记录 task catalog/runtime/schema/commit/artifact
  identity，release 只消费匹配收据的既有字节。

### 迁移验收矩阵

| 维度 | 必须证明 |
|---|---|
| 行为 | 原 fixture 的成功、失败和边界结果等价或更强 |
| 错误 | 稳定 class/code/step，自动化不解析自然语言 |
| 等待 | 无固定 sleep；使用 typed process/Fleet/UI wait |
| 清理 | 无 child、GUI、PTY、server、temp、locked artifact 残留 |
| 隐私 | argv、环境值、credential、pane content 不进入诊断 |
| 平台 | 共享 task 语义；unsupported 明确失败而非静默跳过 |
| 性能 | 增量路径不显著慢于旧路径，独立记录 bootstrap 与 task 时间 |
| 漂移 | CI 拒绝新增 `.ps1` 或入口重新承载业务规则 |

## 五、MCP 协议基线

实现基线固定到官方当前 stable revision `2025-11-25`。官方 `latest`
当前解析到该 revision：

- [Lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)
- [Transports](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)
- [Schema](https://modelcontextprotocol.io/specification/2025-11-25/schema)

v0.1.10 不实现 2026 draft 中尚未稳定的 stateless discovery，也不公布
experimental tasks capability。协议升级必须成为独立、可测试的 catalog
变化，不能跟随依赖升级悄悄发生。

首发支持的方法：

```text
initialize
notifications/initialized
ping
resources/list
resources/read
tools/list
tools/call                  # 只接受 agenterm_wait
notifications/cancelled
```

明确不公布：

```text
prompts/*
sampling/*
elicitation/*
resources/subscribe
resources/templates/list
tasks/*
completion/*
logging/*                   # 诊断走 stderr，不污染协议 stdout
```

stdio 合同：

- 每个 JSON-RPC message 是一行 UTF-8 JSON，不含嵌入换行；
- stdout 只允许协议 frame，启动提示、日志、panic 和诊断进入 stderr；
- EOF 表示 client shutdown，sidecar 在有界 grace period 内取消等待并退出；
- malformed JSON、错误 `jsonrpc`、重复/非法 ID、初始化前越权调用、
  未协商 capability、未知 method 和无效 params 返回标准 JSON-RPC error；
- 超大 frame 在分配无界内存前拒绝，下一条合法 frame 能否恢复必须由
  明确 transport 策略决定并测试；
- panic 不得跨过 frame loop；sidecar 失败不得向 stdout 写半个 JSON。

## 六、Executable 与 server 选择

首发命令面：

```text
agenterm-mcp.exe --help
agenterm-mcp.exe --version
agenterm-mcp.exe capabilities --json
agenterm-mcp.exe serve --stdio [--address 127.0.0.1:PORT]
```

选择规则复用 `agenterm-cli.exe`：

1. 显式 `--address` 优先；
2. 否则使用显式 `AGENTERM_IPC_ADDRESS`；
3. 否则恰好一个 healthy instance 时选择它；
4. 零实例时返回 typed unavailable，不自动启动 GUI/server；
5. 多实例时返回 typed ambiguous，并列出经过清理的 address、PID、
   session、workspace 与 compatibility facts；
6. 不可达、stale、restart、incompatible 与 unknown identity 分开报告。

`capabilities --json` 必须离线输出：

- executable/version/build identity；
- MCP protocol revision 与 MCP schema identity；
- AgenTerm protocol、operation/event catalog schema；
- transport：只列 `stdio`；
- resource/tool catalog；
- frame、response、resource、wait、concurrency 上限；
- `read_only: true`；
- control、subscriptions、content、network、client、brain/flow 等
  unavailable capability 及稳定原因。

## 七、Resource 模型

首发使用固定、可读、无 mutable index/title 的 URI：

| URI | 内容 | 主要身份 |
|---|---|---|
| `agenterm://instances` | 本机已注册实例及健康/兼容状态 | address、PID、session、build identity |
| `agenterm://workspace` | 当前 server 的 workspace/session 摘要 | server epoch、workspace path identity、active stable ID |
| `agenterm://tabs` | 有界标签树与终端生命周期 metadata | stable `@id`、parent ID、state、exit code |
| `agenterm://fleet/snapshot` | 一个因果一致的只读 UI/Fleet snapshot | epoch、sequence、active/focus/layout identities |

所有 resource body 使用 `application/json`，包含：

- resource schema version；
- AgenTerm build/protocol identity；
- selected server identity；
- snapshot event position；
- `complete`、`truncated`、limit 和 degraded reason；
- 与 CLI 同源的数据字段。

默认必须删除或拒绝：

- pane/capture text、scrollback 内容和 terminal selection text；
- Composer、inline editor、settings draft 的文本；
- environment values、proxy URL、credentials、clipboard；
- IPC secret、PAT/token、命令原文和脚本 source/output；
- screenshot 像素或本地任意文件内容。

资源不得因为字段敏感而悄悄输出 `null` 假装完整。schema 要么根本没有该
字段，要么明确声明 redacted/unsupported；资源过大时返回 typed bounded
错误或显式 truncation，不允许生成不完整却标记成功的 JSON。

## 八、唯一工具：`agenterm_wait`

`tools/list` 首发只返回：

```text
agenterm_wait
```

建议输入 schema：

```json
{
  "epoch": "string",
  "after_sequence": 0,
  "kind": "allowlisted event kind",
  "tab_id": "@N or omitted",
  "timeout_ms": 1
}
```

建议输出 schema：

```json
{
  "outcome": "matched|timeout|cancelled|restart|gap|target_closed",
  "event": {},
  "position": {"epoch": "...", "sequence": 0},
  "post_state_identity": {},
  "truncated": false
}
```

约束：

- tool annotation 明确 read-only；不宣称不存在的幂等保证；
- `kind` 来自封闭 allowlist，不能注入私有查询或任意表达式；
- `tab_id` 只接受 stable `@N`；
- timeout 有最小值、默认值和硬上限；
- 每 sidecar 同时等待数有硬上限；
- 一个取消 token 只属于一个 MCP request ID；
- `notifications/cancelled`、stdin EOF、server restart、sidecar shutdown
  和 deadline 都能释放 waiter；
- 迟到结果不能覆盖已经完成的 cancelled/timeout outcome；
- 匹配成功返回事件和新 position，调用方可以立即重读 snapshot 验证。

初始预算建议，实施前用黑盒压力与二进制预算确认：

| 预算 | 初始建议 |
|---|---:|
| 输入 frame | 256 KiB |
| 单 response | 1 MiB |
| resource JSON | 768 KiB |
| 单次 wait | 30 s |
| 并发 wait | 8 |
| 单进程在途 request | 32 |
| stderr 单条诊断 | 4 KiB |

预算只能因实测正常场景不足而调整，并同步 capabilities、PRD 与测试。

## 九、架构边界

建议提取以下 Rust 边界：

```text
src/mcp_protocol.rs
  JSON-RPC / MCP typed envelopes
  initialization state machine
  method catalog and validation

src/mcp_catalog.rs
  offline capability/resource/tool catalog
  schema versions and hard budgets

src/mcp_adapter.rs
  AgenTerm public IPC -> MCP resource/tool results
  stable error mapping and redaction

src/mcp_stdio.rs
  bounded line transport
  cancellation and orderly EOF shutdown

src/bin/agenterm-mcp.rs
  argument parsing and process entry only
```

复用原则：

- MCP adapter 与 CLI/Rhai 共用 operation/event/snapshot typed contracts；
- 不复制一份手写 command manual；
- 不通过启动 `agenterm-cli.exe` 子进程并解析 stdout 实现 MCP；
- 不让 MCP 类型进入 `agenterm.exe` 的 Win32/render/ConPTY 路径；
- 不为首发一个 wait tool引入常驻 daemon 或通用异步框架；
- v0.1.10 可以复用 v0.1.9 的 component availability 语言解释自身依赖，
  但不读取未批准的软件清单，不提供 package resource，也不调用安装器；
- 如评估第三方 MCP Rust 实现，必须先证明协议 revision 可固定、依赖审计
  可接受、release binary 不超预算、panic/stdio 行为符合本产品合同；
  否则实现经过 golden/conformance 测试的最小 typed subset。

## 十、错误与隔离模型

MCP 标准 JSON-RPC code 与 AgenTerm typed details 分层：

- JSON-RPC code 表示 parse/method/params/internal 大类；
- `error.data.code` 使用稳定 AgenTerm/MCP 子码；
- `error.data.retryable` 明确是否可重试；
- `error.data.position` 在适用时给出 epoch/sequence；
- `error.data.candidates` 只用于多实例选择；
- 人类 message 可以改善，但自动化不得依赖 message 文本。

必须区分：

```text
mcp_parse_error
mcp_invalid_request
mcp_not_initialized
mcp_protocol_version
mcp_method_unavailable
mcp_invalid_params
mcp_frame_too_large
agenterm_no_instance
agenterm_instance_ambiguous
agenterm_unreachable
agenterm_incompatible
agenterm_restart
agenterm_journal_gap
agenterm_wait_timeout
agenterm_wait_cancelled
agenterm_target_closed
agenterm_response_too_large
```

隔离不变量：

- sidecar 不拥有 terminal、tab、workspace 或 server 生命周期；
- sidecar crash/kill/EOF 不触发 `kill-server`、close、save 或 GUI activation；
- malformed peer 只能伤害自己的 sidecar；
- 每个 request 的 buffer、deadline、cancel state 和 response 有界；
- stderr 诊断不包含 resource body、pane content、环境值或 credential；
- server restart 后旧 epoch 的 wait 必须失败，不能悄悄接到新 server；
- MCP client 断开后不保留跨连接 mutable state。

## 十一、公共黑盒与自反馈

MCP 测试作为 Rhai task/module 实现，只驱动发布制品和公开接口；不新增
`tests/mcp_smoke.ps1`。迁移期可以复用旧 fixture，但完成门前必须由 Rhai
harness 独立拥有启动、协议帧、等待、断言、诊断和清理。

### 协议生命周期

- 离线 help/version/capabilities 不启动 GUI/server；
- initialize 前非法 method 被拒绝；
- supported revision 协商成功，unsupported revision 分型；
- initialized 后 resources/tools 可用；
- duplicate initialize、非法 notification、未知 method、batch 策略明确；
- stdin EOF 后有界退出，stdout 每行都是完整 JSON-RPC。

### Resource 同源性

- 同时读取 MCP resource 与 `agenterm-cli ui-snapshot/server-list`；
- 比较 server identity、epoch、sequence、stable tab/parent/active ID；
- rename、note、tree、dead tab、detached window 后仍一致；
- 多实例选择与 explicit address 一致；
- resource size/truncation/degraded facts 真实。

### Wait 因果性

- MCP 建立 baseline；
- 独立 CLI 修改 note 或选择 tab；
- `agenterm_wait` 返回唯一匹配事件和新 position；
- 再读 resource 的 post-state 与事件相符；
- unrelated event 不错误满足 predicate；
- timeout、cancel、target close、journal gap、server restart 分型；
- 取消与断开后下一次 wait 仍健康。

### 对抗与隐私

- malformed UTF-8/JSON、oversize、深层 JSON、长 ID、重复字段；
- sidecar kill、backend disconnect、server kill/restart；
- 资源和错误中注入已知 secret sentinel，所有输出/日志/诊断扫描为零；
- pane/composer/environment/proxy/clipboard 字段不存在；
- 高并发请求和 wait 达到上限时 fail-closed；
- GUI 持续产生 terminal output，sidecar 压力不能阻塞渲染或 IPC。

### 清理证明

每次失败和成功都检查：

- 无测试拥有的 `agenterm-mcp.exe`；
- 无 MCP waiter、reader thread、pipe handle；
- 无新增 server/HWND/PTY；
- 无 instance registration；
- 无临时 workspace/settings/secret；
- 外部环境变量和前景窗口恢复。

首错诊断包保存：

- 已脱敏 JSON-RPC method/id/result class；
- MCP/AgenTerm schema 与 build identity；
- selected server、epoch、sequence；
- bounded stderr；
- cleanup proof；
- 不保存完整 resource body，除非 fixture 明确无敏感内容。

## 十二、交付与文档

构建清单增加：

```text
agenterm-mcp.exe
```

交付要求：

- Windows console subsystem；从 MCP client 启动不弹 GUI；
- `agenterm.exe` 第一窗口路径不加载 MCP 代码或依赖；
- 建议 release size 上限 2 MiB，超过时先分析依赖与 feature；
- SBOM、artifact manifest、hash、binary-role 检查与 release workflow 对齐；
- `dist/*locked*` 与 target 清理继续遵守现有构建策略；
- 全部 GUI 测试继续继承 `AGENTERM_NO_ACTIVATE=1`；
- release qualification 不因 MCP 增加公共网络访问。
- 发布清单和文档列出 Rhai task catalog/schema identity；
- 文档中的开发、测试和发布示例不再把 `.ps1` 作为主路径或备用路径。

README 只增加：

1. 一句只读 MCP 定位；
2. 一个通用 stdio client 配置片段；
3. 一个五分钟资源读取与 wait 例子；
4. 明确写出“无控制工具、默认无 pane text”。

详细 URI、schema、错误、预算和未来角色留在 PRD/协议发现输出中。

## 十三、依赖图与并行实施

```text
波次 0：串行盘点与冻结合同
  .ps1 responsibility/caller/evidence inventory
  Rhai task graph + bootstrap boundary
  MCP revision/method/resource/tool catalog
  URI/schema/error/budget
          |
          v
波次 1：可并行
  A. Rhai runtime/API gaps + shared modules
  B. metadata/lint/build task migration
  C. typed black-box harness + cleanup proof
  D. mcp_protocol + golden tests
  E. public IPC adapter + resource mapping
  F. wait/cancel core + race tests
          |
          v
波次 2：串行集成
  check / qualification / package task migration
  caller cutover + delete migrated .ps1
  agenterm-mcp.exe entry
  stdio loop + adapter + wait
  protocol-info/capability alignment
          |
          v
波次 3：并行验证
  clean bootstrap without PowerShell
  task parity / platform / cancellation / cleanup
  lifecycle/conformance
  resource same-source
  wait/restart/gap/cancel
  crash/privacy/load
          |
          v
波次 4：串行候选
  zero repo-owned executable .ps1 gate
  full check
  clean qualification
  byte-identical package
  non-publishing release rehearsal
```

并行所有权：

| 分支 | 首选文件所有权 | 不应同时修改 |
|---|---|---|
| Runtime | script runtime/API/catalog, typed unit fixtures | task implementations |
| Task migration | Rhai modules, task manifest | runtime internals |
| Harness | Rhai black-box helpers, fixtures | product private state |
| Protocol | `mcp_protocol.rs`, protocol unit fixtures | Win32 state machine |
| Catalog | `mcp_catalog.rs`, capability fixtures | runtime adapter |
| Adapter | `mcp_adapter.rs`, public IPC contracts | stdio parser |
| Wait | wait/cancel module与 race fixtures | resource schemas |
| Delivery | build/qualification manifests and Rhai tasks | protocol semantics |
| MCP black-box | Rhai MCP fixtures and harness calls | production internals |

`Cargo.toml`、`src/lib.rs`、PRD alignment、artifact manifest 和最终 binary
entry 是集成热点，只允许一个串行 owner 收口。

## 十四、验收门

### 门零：PowerShell 退出与自举闭环

- `git ls-files '*.ps1'` 返回空结果，测试、helper 和 archive 均无例外；
- clean-checkout 资格运行的进程证据中不存在 `powershell.exe` /
  `pwsh.exe`，Rhai、`.bat` 和 workflow 均无反向调用；
- lint gate 拒绝新增 `.ps1` 和入口脚本中的业务规则回流；
- 干净 checkout 只经 stage 0 + Rhai task 完成 build/check/qualification；
- 开发机与 CI 使用同一 task ID、依赖图、平台、副作用、预算和 evidence
  catalog；
- build、test、qualification、package、release 没有隐式 PowerShell 子进程；
- 文档、AGENTS、workflow、`.bat`、Cargo metadata 与 task catalog 不再引用
  已删除脚本；
- 失败、取消与超时后无 child/temp/GUI/server/locked artifact 残留。

### 门一：只读真实性

- resources 与 CLI 同源；
- stable ID、epoch、sequence 不漂移；
- 无 mutation tool；
- 无默认 content resource；
- unavailable 能力显式可发现。

### 门二：等待正确性

- 唯一 wait tool 能从 snapshot baseline 等到真实事件；
- restart、gap、timeout、cancel、closed 分型；
- 返回位置可继续读取；
- 无重复完成、迟到覆盖或残留 waiter。

### 门三：协议兼容

- stable revision 生命周期完整；
- stdio 每行合法 UTF-8 JSON-RPC；
- frame 与 response 有界；
- 初始化前后 capability 行为正确；
- 未实现 draft/experimental surface 不被广告。

### 门四：故障隔离与隐私

- sidecar crash/kill 不影响 GUI/PTY/server；
- malformed/oversize client 不造成无界资源；
- pane、Composer、环境、proxy、clipboard、credential 不泄露；
- no-activate、首窗口、remain-on-exit、显式关闭不回退。

### 门五：交付

- required gates/evidence 100%；
- `agenterm-mcp.exe` 进入 manifest、SBOM、size/hash；
- clean candidate receipt 绑定同一批字节；
- package 不重建；
- 用户明确批准后才允许 tag/Release。

## 十五、主要风险

| 风险 | 早期信号 | 应对 |
|---|---|---|
| 为赶进度逐行翻译 PS1 | Rhai 中出现 shell 字符串、重复 helper | 先提取合同和 typed API，再迁移 task |
| 自举形成循环依赖 | 构建 runtime 需要先运行 runtime | stage 0 只做一次 Cargo bootstrap，stage 1 验 identity |
| 双轨长期存在 | 同一测试同时维护 PS1/Rhai 两份真相 | 每项双跑通过后立即切 caller 并删除 PS1 |
| 平台差异泄漏进 task | 大量 OS 判断和命令字符串 | typed platform adapter + explicit unavailable |
| 新 harness 降低覆盖 | 只验证 happy path 或依赖 fixed sleep | 逐 evidence parity，typed wait，旧脚本删除前审核 |
| 迁移拖慢 MCP 主线 | 共享热点频繁冲突 | runtime/task/MCP 分文件所有权，波次 2 串行收口 |
| “只读”被等待工具偷换成控制 | tool schema 出现 action/command 字段 | allowlist 只接受 event predicate |
| MCP 与 CLI 形成两份产品事实 | adapter 开始拼人类文本或复制状态 | 强制复用 typed contracts并逐字段比较 |
| pane 内容意外泄露 | resource 直接复用完整 ui-snapshot | 建立专用 metadata DTO 与 secret sentinel 扫描 |
| stdout 被日志污染 | client 偶发 parse error | stdout protocol-only，stderr bounded diagnostics |
| 追逐 draft 造成不稳定 | 实现 server/discover/tasks 等未定 surface | 固定 stable revision，升级单独立项 |
| SDK 依赖拖大二进制 | MCP sidecar 超预算或引入 runtime | 先做依赖/size spike，保留最小 typed subset |
| wait 造成线程/IPC 堆积 | cancel 后仍有 waiter 或 GUI 延迟 | concurrency/deadline/cancel hard ceiling |
| 多实例选择误连 | 无 address 时随机选择 | 复用 zero/one/many fail-closed 规则 |
| UI 又开始膨胀 | 为 MCP 增加默认面板 | 首发不新增 GUI，能力由 CLI/catalog 发现 |
| 版本变成 agent 平台大爆炸 | 出现 control、brain、flow、LLM 工作项 | 保持一资源链 + 一 wait 工具纵向闭环 |
| MCP 偷变软件管理远程入口 | resources/tools 出现 install/update | 首发不公布 package inventory 或 mutation |

## 十六、第一次评审建议结论

建议直接接受以下默认决策：

1. 主题：**Rhai 自举工具链与可验证的只读 Agent 桥梁**。
2. 仓库自有 PowerShell 业务逻辑在 v0.1.10 完整退出，完成门为
   `git ls-files '*.ps1'` 返回空结果；测试与 archive 不豁免。
3. `agenterm.tasks.json` 和 Script API catalog 成为开发任务事实源。
4. stage 0 只解决 Rust/runtime bootstrap，不承载业务规则。
5. 新二进制：`agenterm-mcp.exe`。
6. transport：只做 stdio。
7. stable protocol revision：`2025-11-25`。
8. resources：instances、workspace、tabs、fleet snapshot。
9. tools：只做 `agenterm_wait`。
10. pane text：默认不提供，本版完全延后。
11. server 选择：复用 CLI explicit/zero/one/many 规则，不自动启动。
12. GUI：不增加 MCP 控件和状态动画。
13. MCP control tools、subscriptions、MCP client、brain/flow、Agent 权限
    全部延后；这里的 tasks 指 MCP experimental tasks，不是 Rhai task。
14. v0.1.10 只复用组件 availability 语言，不暴露包清单、市场或安装能力。

仍需在实现波次 0 用 spike 决定：

- 使用外部 Rust MCP implementation 还是自有最小 typed subset；
- 最终 frame/resource/concurrency 预算；
- resource envelope 是复用一个通用 schema，还是每个 resource 有独立
  schema ID；
- `agenterm_wait` 的首发 event kind allowlist；
- 是否把一个真实第三方 MCP host 的手工兼容验证作为 release evidence，
  还是仅作为非阻塞互操作报告。

## 十七、建议第一刀

```text
第一提交
  [done] PowerShell responsibility/caller/evidence inventory
    43 stable IDs and responsibility groups are frozen with caller,
    input/output, side-effect, budget, platform, parity, and deletion evidence
  [done] stable Rhai task graph
    repository manifest exposes all forty-six ready tasks plus validated
    dependency, platform, side-effect, and schema-v3 execution contracts
  [done] stage-0 bootstrap identity contract
  [done] no-new-ps1 migration audit gate
  [done] migrate four Windows root batch entries
    build/check/lint/release are exact one-line aliases; one generic 32-line
    stage-0 bootstrap owns worker build/copy/forward/cleanup, while build profile,
    identity, Cargo, staging, target cleanup, qualification and publication
    policy live in named Rhai tasks and are enforced by migration-audit
  [done] provide four matching Linux/macOS shell entries
    one generic bootstrap forwards build/check/lint/release; native client
    build, portable Quick check and validation-only release are explicit,
    while Windows qualification/package/publish remain unavailable on Unix

第二提交
  [done] shared Rhai build/test helpers
  [done] first low-risk build task migration: clean-locked-artifacts
  [done] public black-box behavioral evidence
  [done] caller cutover + delete fourth .ps1

此后每个迁移提交
  one independently verifiable script or tightly-coupled group
  parity evidence + all-caller cutover
  delete migrated .ps1 immediately
  refresh machine-readable ledger and remaining count

第三提交
  [done] offline mcp catalog
  [done] protocol revision + methods + resources + tool + budgets
  [done] capabilities --json
  [done] catalog invariant tests and exact public serialized lifecycle fixture

第四提交
  [done] bounded stdio JSON-RPC
  [done] initialize / initialized / ping
  [done] malformed / oversize / EOF tests

第五提交
  [done] instances / workspace / tabs / fleet snapshot resources
  [done] isolated public CLI/MCP fixture compares same-source server,
    epoch/sequence, workspace/window and every published tab field while
    rejecting private sentinels

第六提交
  [done] tools/list exposes only agenterm_wait
  [done] concurrent bounded event polling, timeout and cancellation
  [done] live ping-during-wait evidence against 48815
  [done] isolated public matched-event/post-state causality
  [done] typed restart/gap/future-sequence/target-close IPC fixtures
  [done] public restart/gap/future-sequence and EOF cleanup fixtures
  [done] waiter-ceiling recovery and force-killed-client orphan evidence

第七提交
  [done] migrate remaining build/test/qualification/package/release tasks
  [done] clean Windows/Unix bootstrap parity
  [done] remove final .ps1 + activate zero-ps1 drift gate

第八提交
  [done] public malformed/oversize recovery, wait load/cancel/orphan, forced
    sidecar-kill GUI/server/PTY isolation and same-source privacy qualification
  [done] artifact / SBOM / README / PRD alignment
  [done] final clean rehearsal
    a 2026-07-30 artifact-free clone of `7c88ff0` completed 34/34
    stress-inclusive qualification, package rehearsal, process-tree
    observation with zero PowerShell processes, remote-ref immutability,
    and owned-clone cleanup
```

这条切法让 AgenTerm 用自己的 Rhai 工具链完成自我构建、验证与交付，同时
在不引入自主控制、不扩大 GUI、不开放网络 listener 的前提下，第一次成为
任何兼容 MCP client 都能稳定观察的 Fleet。
