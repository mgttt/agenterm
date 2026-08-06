# AgenTerm v0.1.8 公开计划

> ## ⚠️ 已归档（2026-08-05）
>
> **本文是 v0.1.8 时期的历史执行记录，保留仅为追溯，不要作为执行依据。**
> 归档时全仓**零引用**（`git grep plan-v0.1.8` 除本文外为空），其结论已由
> 后续版本 plan 与 PRD 取代。
>
> - 上一已发布版本复盘：`plan/archive/plan-v0.1.14.md`；在制：`plan/plan-v0.1.15.md`
> - 在制版本：`plan/plan-v0.1.15.md`
> - 发布链要求（版本无关权威处）：
>   `prd/PRD_02_17_delivery_quality.md` §Release-chain operating requirements
> - 结构 SSOT：`plan/ARCHITECTURE.md`
>
> 注意：v0.1.12 / v0.1.13 虽有 plan 但**从未公开发布**，公开序列为
> v0.1.11 → v0.1.14。

状态：讨论稿（历史）
工作主题：**可编程的日用舰队**
版本定位：v0.1.7 完成内部控制面与交付整理之后，第一个面向公开使用的产品增量。

本文是公开的版本执行计划与决策记录，不是产品事实，也不得成为实现依赖。评审通过后，接受的范围、约束和验收条件应同步写入各自负责的 `PRD.md` 或 `prd/PRD_*.md`；版本完成后保留本文作为交付历史。

说明：正文使用中文。无法合理翻译且需要保持精确的程序名、命令名、文件名、协议字段和代码标识符保留原文。

产品取舍准绳：

> 界面简单实用，软件稳定可靠，编程接口丰富，并为扩展保留足够空间。

这意味着 AgenTerm 不以堆叠可见功能取胜。新增 UI 必须实际降低日常操作
成本；高级能力优先通过可发现的命令和编程接口提供；次要控件应尽量按
上下文出现或默认隐藏，让工作台保持安静，但不能以隐藏为代价制造不可
发现性。

## 〇、主要方法

默认采用：

> 树式思维 + 扩散思维 + 依赖感知的并行思维

```text
树式思维
  先定义一个版本成果
    再拆成可以独立验收的能力分支
      每个分支继续拆成行为、证据、交付和明确不做的叶子

扩散思维
  对每个分支先提出多个方案和边界情况
    比较用户价值、安全性、复杂度、复用价值和测试成本
      主动裁掉不能服务版本成果的诱人想法

并行思维
  实施前画出依赖图、共享前提和热点文件
    只把所有权独立、证据独立的分支并行执行
      在明确接口处汇合
        最终只在一个干净提交上串行资格测试
```

这套方法不是“列出一大堆任务，然后全部同时开工”，而是：

1. 用最小但完整的产品树定义结果。
2. 在每个分支内部先扩散探索，避免过早锁死实现。
3. 用版本目标、非目标和验收证据收敛。
4. 识别共享前提、热点文件、集成点和关键路径。
5. 只并行处理文件所有权互斥、接口稳定的分支。
6. 用小而完整的提交持续集成。
7. 对最终候选执行一次串行、完整、可复现的资格测试。
8. 打包只能消费已经通过资格测试的同一批字节。

每个准备交付的叶子都必须回答：

- 它解决了什么用户问题？
- 它受什么产品不变量或权限边界约束？
- 哪个可观察状态可以证明成功？
- 哪个明确结果可以证明失败得安全？
- 哪个公共黑盒测试拥有它的证据？
- 这个版本明确排除了什么？

## 一、版本目录树

```text
v0.1.8  可编程的日用舰队
│
├─ 最高优先级：专业终端交互
│  ├─ 选择文本拖出视口时自动滚动
│  ├─ 双击选词、三击选择可视行
│  ├─ 正向、反向、换行、中文和宽字符选择正确
│  ├─ 捕获丢失、切换标签、弹窗和关闭时安全取消
│  └─ 输入、缩放、转义序列、中文和长输出资格矩阵
│
├─ 最高优先级：通用 Rhai 脚本运行时 (agenterm-rhai.exe）
│  ├─ 产品能力对标 Node.js / Bun，而不是受限插件语言
│  ├─ 文件、路径、环境、进程、网络、定时器和流式输入输出
│  ├─ 本地模块、任务清单、命名命令和可发现标准库
│  ├─ AgenTerm Fleet 原生完整 typed API
│  ├─ 输出供未来 agenterm-agent.exe 消费的工具 schema
│  └─ pure / observe 保留为专用执行方式，但不构成平台能力上限
│
├─ 最高优先级：完整公共自反馈旅程
│  ├─ 使用发布制品跑一条日常用户旅程
│  ├─ 验证事后状态，不只验证退出码
│  ├─ 在关键转换点保存结构化状态和有限截图
│  ├─ 首次失败自动保留受隐私约束的诊断包
│  └─ 不留下任何测试拥有的 server、窗口、PTY、worker 或注册记录
│
├─ 最高优先级：正式资格测试与发布
│  ├─ 一个候选只构建一次、只产生一份资格收据
│  ├─ 打包只消费字节完全相同的合格制品
│  ├─ 在不削弱正确性的前提下增加缓存和阶段计时
│  ├─ 尽早返回可执行的失败信息
│  └─ 可靠演练 v0.1.8 标签到 GitHub Release 的完整路径
│
├─ 最高优先级：工作区 P0 proxy 正确性
│  ├─ tab-scoped、ephemeral、永不持久化或泄露 secret
│  ├─ Prepared → Submitted → Applied|Failed 由真实证据推进
│  ├─ GUI 可鼠标 Reveal/Re-mask、Prepare、Send Now
│  ├─ cmd/PowerShell/Bash 环境与真实 child 继承验证
│  └─ direct TUI/非交互拒绝、无隐式继承、typed receipt 与 orphan 门
│
├─ 第一优先级：工作区视觉和操作层级
│  ├─ 标签区底部按钮、全局状态条和输入区重新梳理
│  ├─ 标签名称/注释在本行原地编辑，不借用下方 Composer
│  ├─ 编辑行把 +/Edit/Close 原位切换为 Save/Cancel
│  ├─ Tabs 根 inset 和逐层 indent 左移紧凑，paint/hit-test 共用几何
│  ├─ 选中标签的名称和注释不被操作按钮挤压
│  ├─ 深层树、长文本、中文、180 px 和高缩放仍清晰
│  ├─ 键盘优先的命令发现和命名命令入口
│  └─ 整理 Settings 信息架构，为以后自定义保留稳定模型
│
├─ 第一优先级：因果上可以互相比较的证据
│  ├─ 模型序号、渲染代次和最后绘制序号
│  ├─ capture、snapshot、screenshot 使用一致身份信息
│  ├─ 所有 wait 命令统一稳定目标和重启语义
│  └─ workspace 原子保存并暴露 revision 与 hash
│
└─ 明确延后与未来计划
   ├─ agenterm-agent.exe 的具体实现、审批 UI 和 agent 权限系统
   ├─ 暂名 agenterm-net.exe：curl 兼容网络工具 + libp2p/IPFS 去中心化能力
   ├─ agenterm-mux.exe 完整多路复用终端与 mux|tmux|rmux|agenterm 多后端
   ├─ 探索合并 agenterm.exe 与 agenterm-cli.exe，但不牺牲可靠 CLI 语义
   ├─ 通用 Rhai 稳定后分阶段自托管仓库辅助脚本，PowerShell 保留到自举与回退通过
   ├─ 远程 proxy 分发、fleet-wide/global 默认值与持久 proxy profile
   ├─ 自主或破坏性的 MCP 控制
   ├─ npm 兼容、公共包仓库和不受治理的后台系统服务
   ├─ 把 agenterm-bash.exe 设置为默认 shell
   ├─ 默认联网安装可选组件
   ├─ 安装器、更新器和签名，除非另行批准
   ├─ LLM gateway 或智能 worker
   └─ 声称完整兼容 tmux 或 RMUX
```

### 明确延后与未来计划：口径和 Rhai 自托管迁移

- **明确延后**：产品方向已经认可，但明确不进入 v0.1.8 的实现、资格和
  发布范围。
- **未来计划**：只保留研究或架构挂钩；仍需独立立项、用户价值证据、
  威胁与失败模型、依赖门和版本归属，不能由本计划自动变成承诺。

仓库辅助脚本的长期 self-hosting 属于未来计划，不是 v0.1.8 交付项。
只有当 `agenterm-rhai.exe` 的通用标准库、进程、文件、JSON、流、
稳定退出码和 Windows 语义都通过公共资格门后，才按以下顺序迁移：

```text
低风险辅助脚本
  PowerShell 与 Rhai 双跑并比较结果
    -> 测试编排
       双跑、故障注入、取消与清理一致
         -> 正式资格 / 打包 / 发布关键路径
            最后迁移，要求字节身份、receipt 和紧急回退证明
```

- Rhai 自举尚未证明前，构建和资格不能只依赖待构建的 Rhai runtime。
- 紧急回退、干净机器和受限 Windows 环境尚未证明前，不删除 `.ps1`
  实现；双跑阶段 PowerShell 是可恢复的 last-known-good。
- 迁移必须保持命令退出码、stdout/stderr、路径、编码、流、取消、超时、
  子进程清理、签名/凭证隔离和无交互执行语义。
- 该方向不是现在删除 PowerShell，也不是把 Rhai 或
  `agenterm-bash.exe` 设为默认 shell。

### 未来挂钩：`agenterm-net.exe`

此处不是 v0.1.8 的实现承诺，而是一个有意保留的长期产品方向。暂名：

```text
agenterm-net.exe
```

北极星不是“再造一个下载命令”，而是：

> 为人、Rhai 脚本、agentic tools 和未来去中心化应用提供统一、可组合、可观察的网络能力；既能处理今天的 HTTP 世界，也能进入基于内容寻址和点对点协议的网络。

#### 长期能力树

```text
agenterm-net.exe
|
+-- 传统网络工具层
|   +-- curl 风格 CLI 与常用参数习惯
|   +-- HTTP / HTTPS
|   +-- header / cookie / redirect / auth
|   +-- upload / download / resume / range
|   +-- proxy / TLS / certificate / DNS
|   +-- stdin / stdout / file / stream 管道
|   +-- hash / checksum / retry / timeout / rate limit
|   +-- 结构化 JSON 结果和稳定退出码
|
+-- libp2p 点对点网络层
|   +-- peer identity
|   +-- multiaddr
|   +-- transport 与安全握手
|   +-- peer discovery
|   +-- stream multiplexing
|   +-- DHT
|   +-- publish / subscribe
|   +-- relay / NAT traversal
|   +-- 带宽、连接和资源预算
|
+-- IPFS 与内容寻址层
|   +-- CID
|   +-- block
|   +-- DAG
|   +-- add / cat / get
|   +-- pin / unpin
|   +-- gateway
|   +-- 内容验证和去重
|   +-- 本地缓存与存储预算
|
+-- 去中心化应用基础
|   +-- 内容发布与发现
|   +-- 可验证制品分发
|   +-- 节点间任务和结果交换
|   +-- 事件与消息通道
|   +-- 本地优先的数据同步
|   +-- 离线可读与重新连接
|   +-- 可组合的 dApp 工具协议
|
+-- AgenTerm 原生协作
    +-- agenterm-rhai.exe 的 net / ipfs / p2p 标准库
    +-- agenterm-agent.exe 的结构化网络工具
    +-- Fleet 标签中的传输任务与实时状态
    +-- 状态条中的受限进度和连接摘要
    +-- Observable Fleet 事件、receipt 和审计
    +-- MCP 或其他 agentic 协议的网络执行后端
```

#### 与 curl 的关系

目标是让常见 curl 使用习惯可以低成本迁移，而不是在没有兼容矩阵前声称完整兼容。

```text
第一阶段
  常见 HTTP/HTTPS 请求、header、body、文件、redirect、proxy、timeout

第二阶段
  upload/download、resume、cookie、auth、TLS、详细诊断和格式化输出

第三阶段
  建立公开兼容矩阵
  supported / partial / unsupported 明确列出
```

任何暂未支持的参数必须明确报错，不能静默忽略。

#### 与 `agenterm-rhai.exe` 的关系

```text
agenterm-rhai.exe
  通用脚本语言和任务编排
  |
  +-- 直接提供轻量 HTTP 标准库，满足普通脚本
  |
  +-- 需要复杂传输、IPFS 或 libp2p 时
      调用 agenterm-net.exe 的 typed protocol
```

这样可以同时满足：

- 普通 Rhai 脚本不必为简单 HTTP 请求启动复杂网络节点；
- libp2p/IPFS 的依赖、线程、连接和存储不进入 GUI；
- `agenterm-net.exe` 崩溃或升级不影响终端、workspace 和脚本引擎；
- script、agent 和 CLI 复用同一网络实现，不各自重新造一套。

#### 与 `agenterm-agent.exe` 的关系

```text
agenterm-agent.exe
  决定 agent 可以使用哪些网络工具、域名、peer、CID、预算和 credential
  |
  v
agenterm-net.exe
  负责可靠执行 HTTP、libp2p 和 IPFS 操作
```

权限、审批、目标约束和 credential policy 继续属于 agentic 治理层；网络 sidecar 不解析自然语言意图，也不决定某个 agent 是否有权联网。

#### 更大的产品野心

如果基础成立，AgenTerm 不只管理本机 PTY，还可以逐步形成：

```text
本地 Fleet
  -> 可验证的内容和制品交换
    -> 跨节点任务协作
      -> 去中心化的 agent / script 工具市场
        -> 本地优先、可离线、可验证的 dApp 工作区
```

潜在方向包括：

- 用 CID 发布和验证构建制品、脚本模块和证据包；
- 在用户控制的 peer 之间传递任务、结果和 Observable Fleet 事件；
- 基于内容寻址复用大型依赖和缓存；
- 让脚本或 agent 通过统一工具描述使用 HTTP、IPFS 和 p2p；
- 为以后去中心化身份、协作工作区和可验证计算保留接口；
- 让 AgenTerm 从“本机 agent 舰队终端”扩展为“跨节点可验证协作终端”。

#### 正式立项前置条件

在未来版本真正进入实施树之前，至少需要：

- 明确 `agenterm-net.exe` 与 GUI、server、script、agent 的进程和协议边界；
- 研究 Rust libp2p/IPFS 实现、许可证、维护状态和 Windows 可移植性；
- 定义 peer identity、私钥存储、轮换、备份和丢失模型；
- 定义连接数、带宽、磁盘、DHT、缓存和长期任务预算；
- 定义 proxy、企业网络、离线、NAT、relay 和防火墙行为；
- 建立传统 HTTP 与目标 curl 子集的公开兼容矩阵；
- 建立 CID、block、DAG、pin 和 gateway 的互操作测试；
- 证明网络 sidecar 不影响 GUI 第一窗口、PTY 延迟和本地 workspace；
- 明确默认是否启动节点；初始方向应是按需启动，而不是静默常驻；
- 先通过 threat model，再开放来自 agent 的网络工具。

#### v0.1.8 只保留什么

v0.1.8 只允许做不会偷跑组件实现的接口准备：

- `agenterm-rhai.exe` 的 HTTP 标准库不要封死未来 typed net backend；
- 工具 schema 能表达 network、peer、CID、stream、credential 和预算事实；
- executable/component manifest 可以在未来增加新的 sidecar role；
- Observable Fleet 的 operation、event 和 receipt 模型不把网络任务排除在外；
- PRD 保留该长期节点，但不把 libp2p/IPFS 依赖加入本轮构建。

明确禁止以“未来兼容”为理由，在 v0.1.8 中提前加入庞大网络依赖、常驻节点、密钥系统或未验收的下载逻辑。

### 未来挂钩：`agenterm-mux.exe` 完整多路复用终端

#### 产品野心

当前 `agenterm-mux.exe` 主要是面向 AgenTerm server 的 tmux/RMUX 兼容 CLI，一个 AgenTerm tab 仍然只对应一个 pane。

长期目标不能停在“兼容层”：

> `agenterm-mux.exe` 应成为统一的多路复用终端前端；既能平滑控制现有 tmux、RMUX 和 AgenTerm，也能通过原生 `mux` 后端完整拥有 session、window、pane、PTY、布局、detach/attach 和 server 生命周期，最终具备替代 tmux/RMUX 的能力。

#### 总体架构

```text
                         agenterm-mux.exe
                  统一命令入口 / 格式 / 脚本接口
                                  |
                         --backend <name>
                                  |
          +-----------------------+-----------------------+
          |                       |                       |
          v                       v                       v
     backend=mux             backend=tmux            backend=rmux
     AgenTerm 原生           连接系统 tmux           连接系统 RMUX
     多路复用内核            server/socket           server/socket
          |
          +-----------------------------------------------+
                                                          |
                                                          v
                                                   backend=agenterm
                                                   连接 AgenTerm server
                                                   标签树 / Fleet / GUI
```

前端与后端职责：

```text
agenterm-mux.exe 前端
  解析命令和别名
  统一 target / format / exit / error
  发现 backend capability
  把语义命令路由到 backend adapter
  明确报告 supported / partial / unsupported

backend
  拥有或连接真正的 session / window / pane
  负责 PTY 和进程生命周期
  负责布局、detach/attach 和持久 server
  返回统一 typed result 和 backend-specific detail
```

#### 多后端模型

```text
--backend mux
  agenterm-mux.exe 自己管理的原生多路复用后端
  目标是完整对标并最终替代 tmux / RMUX

--backend tmux
  调用或连接用户现有 tmux
  保持 tmux 是事实 authority
  不伪装 AgenTerm 拥有其 PTY

--backend rmux
  调用或连接用户现有 RMUX
  保持 RMUX 是事实 authority
  针对 RMUX 差异使用明确 adapter

--backend agenterm
  复用 agenterm-cli.exe 的命令语义、typed command 和 IPC 客户端
  使用当前 AgenTerm server、标签树和 Fleet 状态
  一个 tab 当前对应一个 pane
  不虚假模拟尚不存在的 pane 分割能力
```

#### `backend=mux`：一个 EXE，多个进程角色

可以不增加第二个可执行文件。`agenterm-mux.exe` 自己同时包含：

```text
agenterm-mux.exe
|
+-- 命令客户端模式
|   +-- 解析用户命令
|   +-- 连接已有 mux server
|   +-- 发请求、收结果、退出
|
+-- 自动启动器模式
|   +-- 发现 server 不存在
|   +-- 处理并发启动竞争
|   +-- 启动同一个 agenterm-mux.exe 的内部 server 模式
|   +-- 等待版本握手和 endpoint 就绪
|   +-- 把原始命令交给 server
|
+-- 内部 server 模式
    +-- 长期拥有 session / window / pane
    +-- 长期拥有 PTY 与子进程
    +-- 接受多个 client
    +-- client 退出后继续存活
    +-- 按 exit-empty / shutdown 策略退出
```

典型过程：

```text
用户执行
  agenterm-mux.exe --backend mux new-session
        |
        +-- 没有 server
        |     |
        |     +-- 启动同一个文件：
        |           agenterm-mux.exe <内部 server 参数>
        |                    |
        |                    +-- server process 长期存活
        |
        +-- client 连接 server
        +-- 创建 session
        +-- client 输出结果后退出

稍后执行
  agenterm-mux.exe --backend mux attach-session
        |
        +-- 连接刚才仍在运行的同一个 server process
```

关键区别：

```text
不需要新的 EXE
  是

不需要新的 server process
  不是
```

如果完全不保留独立 server process，那么启动命令的前台进程一退出，detach 后的 session 和 PTY 就没有 authority 可以继续拥有。除非强制第一个 client 永远不退出，否则无法成为真正的多路复用器。

Windows 上建议：

- server 使用同一个 `agenterm-mux.exe` 文件，通过内部参数进入 server mode；
- 自动启动时不弹新控制台窗口；
- client 与 server 使用有版本的本地 endpoint；
- server 有独立生命周期，不能被第一个 client 的 Job Object 连带终止；
- 并发 client 同时发现 server 不存在时，只有一个成功成为 authority；
- endpoint、PID、启动时间、版本和 namespace 有可验证 registration；
- server 启动握手必须有界，失败返回 typed error；
- `server-info`、`list-servers`、`kill-server` 或等价命令可发现并管理它；
- detach 后保持，只有显式 shutdown、策略满足或不可恢复错误才退出。

内部 server 参数不一定要成为普通用户 API。例如：

```text
agenterm-mux.exe --internal-server --endpoint ...
```

名称以后再定，但必须：

- 不与 tmux/RMUX 兼容命令冲突；
- 不允许不完整参数创建失控 server；
- 不把 credential 放进命令行；
- 能由自反馈测试直接确认身份和清理结果。

实现结构推荐：

```text
agenterm-mux.exe
  薄 main
    |
    +-- client frontend
    |
    +-- native mux server host
            |
            +-- shared mux core library
                +-- session/window/pane model
                +-- PTY runtime
                +-- layout
                +-- protocol/event/receipt
```

这样未来如果确实出现独立更新、安全隔离或服务部署需求，仍可以把同一个 shared mux core 包进 `agenterm-muxd.exe`；但没有证据前，不必为了形式预先增加第五个可执行文件。

#### `backend=agenterm` 是薄适配层

这个后端不需要再建设一套 server，也不需要重新实现 `agenterm-cli.exe` 已经拥有的控制面。

推荐调用关系：

```text
agenterm-cli.exe
  |
  +-- shared commands / typed operations / IPC client
  |                         |
  |                         v
  |                  AgenTerm server
  |
agenterm-mux.exe --backend agenterm
  |
  +-- AgentermBackend 薄适配
      +-- tmux/RMUX 命令语义转换
      +-- target / format 转换
      +-- capability / compatibility 分类
      +-- 调用同一 shared commands / typed operations / IPC client
```

不推荐把常规实现做成：

```text
agenterm-mux.exe
  -> 每条命令启动 agenterm-cli.exe 子进程
    -> 解析人类可读 stdout / stderr
```

后者虽然可以作为早期诊断或兼容 fallback，但会带来：

- 每次调用的进程启动成本；
- 参数二次转义；
- 人类输出与机器协议混合；
- typed error、receipt 和 event position 信息损失；
- CLI 输出格式变化导致内部 adapter 漂移。

因此“直接跟 `agenterm-cli.exe` 互动”在产品语义上成立，在代码结构上应理解为：

> `agenterm-mux.exe` 与 `agenterm-cli.exe` 复用同一套 library 和 AgenTerm IPC，而不是让一个 CLI 把另一个 CLI 当作文本子进程。

这个后端的新增工作量主要只有：

```text
薄 backend adapter
  + backend 选择和 discovery
  + 现有 mux 命令到 typed operation 的映射整理
  + capability / unsupported 矩阵
  + 跨前端结果一致性测试
```

它不包含：

- 新 PTY runtime；
- 新 session server；
- 新 workspace authority；
- 新进程持久化机制；
- native pane split；
- 新的 AgenTerm IPC transport。

所以 `backend=agenterm` 可以作为多后端框架的第一个参考实现，用最小工作量验证 backend trait、命令路由和一致性测试；真正的大工作量集中在原生 `mux` 后端，以及 tmux/RMUX 外部 authority adapter。

#### 后端选择与平滑使用

建议选择优先级：

```text
命令行 --backend
  > 环境变量 AGENTERM_MUX_BACKEND
    > 项目或用户配置
      > 当前稳定默认值
```

当前版本默认值继续保持 `agenterm`，未来是否改成 `mux` 必须经过迁移与兼容门，不能静默改变。

候选公共入口：

```text
agenterm-mux.exe --backend mux new-session
agenterm-mux.exe --backend tmux list-windows
agenterm-mux.exe --backend rmux attach-session
agenterm-mux.exe --backend agenterm list-windows

agenterm-mux.exe list-backends
agenterm-mux.exe backend-info --backend mux
agenterm-mux.exe backend-capabilities --json
agenterm-mux.exe doctor --backend tmux
```

“平滑无缝”应定义为：

- 同一个前端命令名、target 语法、format 语法和结构化结果尽量跨后端复用；
- backend 差异通过 capability discovery 和明确错误暴露；
- 用户脚本可以显式选择后端，不需要改用另一个控制程序；
- 配置 profile 可以给不同项目选择不同后端；
- 常用 session/window/pane 操作在兼容矩阵通过后保持相同行为；
- 不支持的功能立即失败，绝不静默降级。

“平滑无缝”不等于：

- 可以无损搬迁一个已经运行的 PTY 进程到另一个 server；
- 所有 backend 内部 ID、socket、环境变量和生命周期完全相同；
- 在兼容矩阵完成前声称百分之百兼容；
- 把一个后端的私有行为伪造成所有后端都有。

#### 原生 `mux` 后端能力树

```text
native mux backend
|
+-- server
|   +-- 按需启动
|   +-- 命名 socket / endpoint
|   +-- 多 client attach
|   +-- detach 后保持会话
|   +-- 明确 shutdown / exit-empty 策略
|   +-- crash recovery 与 server discovery
|
+-- session
|   +-- create / list / select / rename / kill
|   +-- attach / detach
|   +-- environment / cwd
|   +-- session groups 候选
|
+-- window
|   +-- create / list / select / rename / move / link
|   +-- window index 与稳定 ID
|   +-- automatic rename 与 terminal title
|
+-- pane
|   +-- split horizontal / vertical
|   +-- select / swap / move / join / break
|   +-- resize / zoom
|   +-- one PTY process per pane
|   +-- remain-on-exit
|   +-- capture / pipe / send / wait
|
+-- layout
|   +-- tiled / even-horizontal / even-vertical
|   +-- main-horizontal / main-vertical
|   +-- serialized layout
|   +-- resize constraints
|   +-- terminal resize propagation
|
+-- interaction
|   +-- prefix key
|   +-- key tables
|   +-- bind / unbind
|   +-- command prompt
|   +-- copy mode
|   +-- selection / search / paste buffer
|   +-- mouse mode
|
+-- status and format
|   +-- format expressions
|   +-- status line
|   +-- command result formatting
|   +-- hooks / alerts / activity
|
+-- automation
    +-- control mode
    +-- stable typed protocol
    +-- event stream
    +-- request receipt / idempotency / deadline
    +-- scripting and agent tool schema
```

#### `mux` 与 `agenterm` 后端为什么先分开

```text
mux backend
  终端多路复用器优先
  session -> window -> pane
  一个 window 内多个 pane
  headless server 是正常形态
  兼容 tmux/RMUX 行为是核心合同

agenterm backend
  人与 agent 的 Fleet workspace 优先
  workspace -> tab tree -> terminal
  当前一个 tab 一个 pane
  GUI、Composer、note、agent 上下文是核心合同
  remain-on-exit 和显式关闭服从 AgenTerm 产品语义
```

强行立刻合并会造成两个风险：

- 为了兼容 tmux，把 AgenTerm 的 Fleet、GUI 和显式关闭语义扭曲；
- 为了复用 AgenTerm tab，把真正 pane 分割和 headless multiplexer 做成假实现。

因此建议先建立共享内核，但保持两个 backend：

```text
共享候选
  PTY runtime
  terminal lifecycle
  stable identity
  event journal
  typed operation / receipt
  capture / wait
  layout pure logic

分别保留
  mux session/window/pane authority
  agenterm workspace/tab/Fleet authority
```

未来只有在数据模型、生命周期和兼容证据自然趋同时，才讨论把 `mux` 与 `agenterm` backend 合并；不能先假设一定合并，也不能禁止将来合并。

#### 后端适配层

推荐定义一个不泄漏具体实现的 backend 合同：

```text
MuxBackend
  identity()
  capabilities()
  connect()
  server_info()
  execute(typed_command)
  read_events(position)
  wait(condition, deadline)
  close()
```

统一 typed command 只描述已经收敛的公共语义。后端特有功能通过有名字的扩展 capability 暴露，不把所有差异压缩成一个最低公共分母。

结果建议包含：

```text
backend
backend_version
backend_instance
operation_id
resolved session/window/pane identity
outcome
typed result / error
before / after position
compatibility classification
```

#### 兼容策略

建立按 backend 分列的机器可读矩阵：

```text
command / behavior
  mux       supported | partial | unsupported
  tmux      supported | partial | passthrough | unsupported
  rmux      supported | partial | passthrough | unsupported
  agenterm  supported | partial | unsupported
```

测试来源：

- 公共 tmux 行为与文档；
- RMUX 已确认行为；
- AgenTerm 自己的 typed contract；
- native mux 的独立行为合同；
- 同一黑盒 fixture 在四个 backend 上执行后的差异报告。

“完整替代 tmux/RMUX”只有在以下条件满足后才可以公开声称：

- 核心 session/window/pane 生命周期矩阵通过；
- split、layout、resize、copy mode、key table、status、format 和 control mode 有证据；
- detach/attach、server crash、client crash 和长时间运行稳定；
- 常见 tmux 脚本与配置有明确迁移结果；
- unsupported 和语义差异已经缩小到公开接受范围；
- Windows 是一等平台，同时明确其他平台策略。

#### 后端实现的扩散方案

| 方案 | 结构 | 优点 | 风险 |
|---|---|---|---|
| 方案甲：`agenterm-mux.exe` 同时做 client/server | 同一 EXE 通过内部参数进入 server 模式 | 可执行文件少、部署简单 | client/server 边界容易混乱 |
| 方案乙：增加 `agenterm-muxd.exe` | 前端与原生 server 分离 | 生命周期和安全边界最清楚 | 增加组件、打包和版本协调 |
| 方案丙：复用 AgenTerm server | 在现有 server 中加入 session/window/pane | 最大化共享 | 容易扭曲 Fleet 模型并拖累 GUI authority |
| 方案丁：共享 Rust core，两个 server 外壳 | mux server 与 AgenTerm server 复用底层 crate | 边界和复用较平衡 | 前期抽象设计成本更高 |

当前倾向：

```text
对外部署
  采用方案甲
  只有一个 agenterm-mux.exe
  同一文件承担 client / launcher / internal server 进程角色

对内代码
  同时采用方案丁的思想
  把 session/window/pane、PTY、layout、protocol 提取为 shared mux core
  client/server 边界通过 typed protocol 保持清楚

以后
  只有独立更新、安全隔离或服务部署出现真实需求时
  才重新评估 agenterm-muxd.exe

  用数据模型和兼容测试的趋同程度
  决定 mux 与 agenterm backend 是否合并
```

#### 与 script、agent 和 net 的协作

```text
agenterm-rhai.exe
  通过统一 typed API 控制任意 mux backend

agenterm-agent.exe
  把 session/window/pane 操作包装成 agentic tools
  决定 agent 可见 backend、目标和破坏性权限

agenterm-net.exe
  为跨节点 attach、会话发现或同步提供未来网络能力
  但不能让 multiplexer 基础功能依赖去中心化网络

AgenTerm GUI
  可以把 mux backend 会话作为未来可视化对象
  但不自动声称拥有外部 tmux/RMUX 的状态
```

#### v0.1.8 只保留什么

v0.1.8 不实现 native mux server 或完整 pane 分割，只允许做不会制造假兼容的接口准备：

- 现有 `agenterm-mux.exe` 继续明确标注 AgenTerm backend 的真实能力；
- 命令解析、format 和 compatibility catalog 不写死只有一个 backend；
- 将来可以加入 `--backend`，但本轮不得发布没有实际 adapter 的空选项；
- stable ID、typed result、event、wait 和 receipt 可以被 backend contract 复用；
- PRD 保留原生 mux 与多后端方向；
- 现有“一 tab 一 pane”和 unsupported 行为继续明确失败。

明确禁止：

- 为了展示未来方向而伪造 split-pane；
- 把 tmux/RMUX 命令静默映射成近似但错误的 AgenTerm 行为；
- 在没有 server 生命周期设计前偷偷增加常驻进程；
- 在没有兼容矩阵前宣传完整替代；
- 让 v0.1.8 的关键路径被未来 multiplexer 内核拖住。

### 未来挂钩：探索合并 `agenterm.exe` 与 `agenterm-cli.exe`

此项体现“少即是多”的长期愿望，但不是 v0.1.8 的实现任务。当前继续保留两个很薄的入口：

```text
agenterm.exe
  /SUBSYSTEM:WINDOWS
  双击无黑框、负责 GUI 生命周期

agenterm-cli.exe
  /SUBSYSTEM:CONSOLE
  负责可靠的等待、退出码、管道、重定向和自动化语义

共享 library
  命令解析、协议、业务逻辑和状态模型只实现一次
```

#### 合并的潜在价值

- 减少一个用户可见的可执行文件和入口选择；
- `agenterm.exe <command>` 同时成为 GUI 与 CLI 的统一品牌入口；
- 降低安装、帮助文本和命令发现的表面复杂度；
- 如果实现可靠，可消除 GUI 启动器转介用户使用另一个程序的步骤。

#### `AttachConsole` 没有独自解决的问题

`AttachConsole(ATTACH_PARENT_PROCESS)` 只解决“进程能否附着已有控制台”，不能把
`/SUBSYSTEM:WINDOWS` 程序变成具有完整 Console CLI 语义的程序。正式研究必须分别验证：

```text
控制台附着
  父进程有控制台时能否输出
  父进程无控制台时能否静默保持纯 GUI

标准句柄
  保留继承的 stdout / stderr
  正确支持 >、2> 和管道
  不用 CONOUT$ 覆盖有效的重定向句柄

Shell 生命周期
  cmd / PowerShell 是否等待
  提示符和输出是否按正确顺序出现
  退出码是否可靠
  Ctrl+C 和控制台关闭事件是否符合预期
```

不能无条件执行 `freopen("CONOUT$", ...)`，因为这可能把原本应进入文件或管道的输出强行
写回控制台。应先检查和保留有效的继承句柄，只有缺失标准句柄且确实存在父控制台时，才考虑
附着并补齐句柄。

更根本的风险是：Shell 会在程序代码执行前依据 PE subsystem 决定等待方式。运行时调用
`AttachConsole` 无法反向改变这一决定，因此“看得见输出”不等于“可以可靠替代 Console CLI”。

#### 未来研究的黑盒资格矩阵

```text
启动
  资源管理器双击：无黑框、GUI 正常
  cmd、PowerShell 和 Windows Terminal 直接运行

组合
  stdout > 文件
  stderr 2> 文件
  stdout | 消费程序
  .bat 与 .ps1 调用
  无控制台但继承 pipe handle 的 CreateProcess 调用

语义
  Shell 等待行为
  输出与提示符顺序
  成功和失败退出码
  Ctrl+C 与控制台关闭
  Unicode 和大量输出
  GUI 消息循环与 CLI 阻塞操作共存
```

只有统一入口在这套矩阵中达到与原生 Console 程序相同的可靠性，才重新讨论删除
`agenterm-cli.exe`。如果需要用户使用 `start /wait`、额外 wrapper 或特殊 Shell 配置才能可靠
工作，就不算成功合并。

#### 当前决策

```text
v0.1.8
  不实施合并
  不改变两个 subsystem
  继续最大化共享 library

未来
  允许制作独立原型
  用黑盒资格矩阵而不是肉眼看到输出作为结论
  证据充分后再决定合并、保留或提供可选统一入口
```

这与 `agenterm-mux.exe` 不增加 `agenterm-muxd.exe` 并不矛盾：mux 客户端和内部 server
都属于 Console/headless 生命周期，可以由同一个 EXE 承担不同进程角色；GUI 与 CLI 的合并
则跨越 Windows subsystem 和 Shell 生命周期边界，必须采用更高的证据门槛。

## 二、版本主张与总体验收

### 版本主张

v0.1.7 让 AgenTerm 的内部状态和交付过程更诚实。v0.1.8 要把这种诚实变成日常用户可以直接感知的价值：

> 人可以把 AgenTerm 当作专业终端每天使用；`agenterm-rhai.exe` 可以像 Node.js / Bun 一样承担通用本地自动化，并原生控制同一套舰队；产品可以证明发布出去的那批字节完整通过了真实用户旅程。

### 版本级成功条件

以下三条必须同时成立：

1. 终端交互有可测量的专业化提升，尤其是文本选择、中文宽字符、长输出、缩放和取消行为。
2. tab proxy 不再把 Prepared 或 PTY byte write 冒充 On：只有 shell marker、真实 environment 和 child inheritance 共同证明 Applied。
3. Rhai 不再只是受限 sidecar：它具备文件、环境、进程、网络、模块、任务和 Fleet API 的实用闭环，并输出足够精确的工具描述供未来 `agenterm-agent.exe` 治理。
4. 一个干净候选完整通过公共日用旅程和资格门，随后打包与 GitHub Release 复用完全相同的合格字节。

任何一条不成立，都不能靠增加无关功能来“凑成一个版本”。

## 三、最高优先级分支甲：专业终端交互

### 用户成果

用户在真实日志、代码、中文、宽字符、换行内容和长回滚区中选择、滚动和复制文本时，行为稳定、可预测，不会干扰 PTY 输出，也不会留下卡住的鼠标或选择状态。

### 计划范围

- 建立明确的选择手势状态机：
  `准备 -> 拖动 -> 完成或取消`。
- 鼠标捕获拖动到终端视口上方或下方时，以有限速率自动滚动。
- 捕获丢失、切换标签、打开弹窗、替换终端和关闭窗口时，取消尚未完成的手势。
- 双击按终端单元语义选择词。
- 三击选择一条可视行；跨自动换行的逻辑行留待语义明确后再做。
- 复制时继续遵守宽字符续单元规则，并规范化正向、反向选择端点。
- 在缩放、最小化/恢复、拖动滚动条和选择同时发生时，保证 `ui-snapshot`、文本捕获和 PNG 证据一致。

### 扩散方案与初步收敛

| 问题 | 方案一 | 方案二 | 当前倾向 |
|---|---|---|---|
| 自动滚动时钟 | 复用 GUI timer | 独立 worker | 复用 GUI timer，状态继续由 GUI 单一所有 |
| 词边界 | Unicode 类别 | shell 风格 ASCII | Unicode 感知，加明确的终端标点表 |
| 三击范围 | 可视行 | 自动换行后的逻辑行 | v0.1.8 先做可视行 |
| 应用 raw mouse 仲裁 | 本轮一起做 | 等真实 TUI 夹具需要时再做 | 暂缓 |
| 矩形选择 | 本轮一起做 | 暂缓 | 暂缓 |

### 验收证据

- 纯逻辑测试覆盖端点规范化、词范围、可视行范围、中文宽字符续单元、边界夹紧和 timer 推进。
- 公共 UX 黑盒测试真实执行拖出视口自动滚动、双击、三击、捕获丢失和标签切换。
- 剪贴板文本与 cell dump、capture 和截图中的目标单元一致。
- 取消后不留下鼠标捕获、输入所有权或选择 timer。
- PTY 输出在选择和自动滚动期间继续前进。
- 建立输入、resize、ANSI、中文、宽字符和长输出的自动资格矩阵。

## 四、最高优先级分支乙：通用 Rhai 脚本运行时

### 产品纠偏

Rhai 的语言实现可以被隔离运行，但这不等于产品必须被定位成“安全脚本插件”。

本轮采用的新定位：

> `agenterm-rhai.exe` 是 AgenTerm 自己的通用本地脚本运行时。产品能力和开发体验对标 Node.js / Bun；Rhai 是语言，Fleet 是原生领域能力，专用受限执行方式只是附加能力。

“对标 Node.js / Bun”指用途和能力面，不声称 JavaScript、Node API、npm 或 Bun 二进制兼容。

### 用户成果

用户可以用一个轻量、单文件友好的运行时完成：

- 文件和目录自动化；
- 路径、文本、字节和 JSON 处理；
- 读取和设置环境；
- 启动、等待和管理子进程；
- 发起 HTTP 请求并处理流式结果；
- 使用 timer、任务和受控并发；
- 引用本地模块；
- 定义项目任务和命名命令；
- 观察并控制 AgenTerm 标签、终端、Composer、事件和 workspace；
- 从 CLI、命令面板或未来 agent 复用同一脚本入口。

这是一套正常的本地运行时能力，不应被逐项描述成“例外放行”。

### 能力树

```text
agenterm-rhai.exe
|
+-- Rhai 语言执行
|   +-- run / eval / check
|   +-- REPL 候选
|   +-- watch 候选
|   +-- 结构化错误与退出码
|
+-- 通用标准库
|   +-- fs       文件、目录、元数据、原子写入
|   +-- path     规范化、连接、相对路径、临时路径
|   +-- env      读取、设置、继承
|   +-- process  argv、cwd、env、stdin/stdout/stderr、timeout
|   +-- net      HTTP、loopback、流式响应
|   +-- time     timer、deadline、sleep、单调时钟
|   +-- text     Unicode、编码、正则候选
|   +-- bytes    有界字节缓冲和转换
|   +-- json     解析、序列化和文件辅助
|   +-- stream   管道、逐块读取和背压
|
+-- 模块与任务
|   +-- 本地 import
|   +-- 明确的解析根
|   +-- 循环与版本错误
|   +-- 项目任务清单
|   +-- 用户级命名命令
|   +-- 可发现 API / 文档
|
+-- AgenTerm 原生 API
|   +-- Fleet observe
|   +-- tab / tree / workspace control
|   +-- terminal / Composer I/O
|   +-- event read / wait / subscribe 候选
|   +-- typed receipt / deadline / cancellation
|   +-- status provider 候选
|
+-- 执行策略
    +-- local     正常的通用本地运行方式
    +-- pure      保留的确定性执行方式
    +-- observe   保留的只读 Fleet 执行方式
```

### 两个层次的协作设计

`agenterm-rhai.exe` 与未来的 `agenterm-agent.exe` 不是同一层，也不应互相吞并：

```text
人直接运行脚本
  |
  +------------------------+
                           v
                    agenterm-rhai.exe
                    通用执行能力层
                    文件 / 进程 / 网络 / Fleet
                           |
                           v
                    Windows + AgenTerm Server

agent 发起任务
  |
  v
agenterm-agent.exe
agentic 工具与治理层
  - 工具选择
  - 权限和审批
  - 参数约束
  - secret policy
  - 预算和配额
  - agent 审计
  |
  | 调用经过选择和约束的工具
  v
agenterm-rhai.exe
通用执行能力层
  |
  v
Windows + AgenTerm Server
```

职责边界：

```text
agenterm-rhai.exe
  负责“能不能可靠执行”
  不负责“这个 agent 有没有资格执行”

agenterm-agent.exe
  负责“是否允许某个 agent 调用某个工具”
  不重新实现文件、进程、网络和 Fleet 运行时
```

#### `agenterm-rhai.exe` 负责

- 完整、稳定、可发现的运行时能力；
- 用户明确运行一个本地脚本，就等价于运行一个普通本地程序；运行时不为每个文件、进程、网络或 Fleet 调用弹权限确认；
- 具有副作用的 API 使用明确名称、显式参数、typed result 和可取消合同，而不是隐藏副作用；
- 参数、返回值、错误和退出状态的精确定义；
- worker 崩溃隔离；
- Ctrl+C、parent exit 和 timeout 的取消传播；
- 流式输入输出与背压；
- 不损坏 workspace、PTY 和 GUI 状态；
- 技术性资源护栏，防止单个 worker 拖垮整个产品；
- 输出工具 schema、函数签名、副作用类别和 typed result，供上层使用。

这些属于运行时正确性和稳健性，不是 agent 权限系统。

#### `agenterm-agent.exe` 负责

- 某个 agent 可以看见哪些工具；
- 某个工具是否需要用户批准；
- 参数、路径、域名、进程和 Fleet 目标的约束；
- credential 注入与 secret redaction；
- 每个 agent、会话和任务的预算；
- 风险分级、审批记录和 agentic audit；
- 把自然语言意图映射成结构化工具调用；
- 必要时选择 `pure`、`observe` 或其他受限入口。

#### `pure` 与 `observe` 的新定位

- 继续保留，因为它们已经是有价值的确定性和只读执行方式。
- 它们是运行时提供给上层选择的工具，不代表整个 Rhai 平台的能力上限。
- 未来 `agenterm-agent.exe` 可以把它们包装成低风险 agentic tools。
- 普通用户明确运行本地脚本时，不应被强制降到这两个模式。

### 协作接口

为了让未来 agentic 层不必猜测脚本能力，`agenterm-rhai.exe` 应提供机器可读工具描述：

```text
script api --json
  +-- 函数和命令 ID
  +-- 参数与返回类型
  +-- 错误类型
  +-- 是否访问文件 / 进程 / 网络 / Fleet
  +-- 是否修改状态
  +-- 是否可能长时间运行
  +-- 是否支持取消
  +-- 是否支持 dry-run 或 inspect
  +-- 版本与兼容范围
```

这里的副作用分类是事实描述，不是权限裁决。未来 `agenterm-agent.exe` 根据这些事实和自己的策略决定是否暴露、批准或约束调用。

### v0.1.8 对标切片

v0.1.8 不可能一次完成 Node.js / Bun 的全部生态，但必须建立正确骨架，并交付一个真正能用的纵向切片。

#### 第一层：运行与诊断

- 保持 `run`、`eval`、`check`。
- `run` 支持脚本路径、stdin、参数和明确工作目录。
- 错误包含文件、行列、调用栈、错误类别和稳定退出码。
- `script api --json` 公开全部标准库、AgenTerm API、模式和版本。
- 评估增加交互式 REPL；如果本轮不做，接口设计不能阻塞以后增加。

#### 第二层：本地标准库

- `fs`：
  读写文件、列目录、元数据、创建目录、复制、移动、删除和原子替换。
- `path`：
  Windows 路径规范化、连接、拆分、相对路径和临时目录。
- `env`：
  读取、设置、删除并构造子进程环境。
- `process`：
  executable + argv、cwd、env、stdin、stdout、stderr、timeout、exit code、cancel。
- `net`：
  HTTP 请求、header、body、timeout、状态码和有界流式响应。
- `time`：
  wall clock、monotonic deadline、timer 和 sleep。
- `json/text/bytes`：
  自动化所需的基础数据处理。

#### 第三层：模块和任务

- 支持本地模块 import。
- 模块解析有明确根目录、相对路径和循环错误。
- 支持项目任务清单，把命名任务映射到脚本、入口函数、参数和运行模式。
- 用户级命名命令注册表供 CLI、GUI 命令面板和未来 agent 共同发现。
- 无效或不兼容命令继续可见，并显示 typed degraded reason。

#### 第四层：AgenTerm 原生能力

- 不只开放少数装饰性操作，而是从 typed operation catalog 系统映射 Fleet API。
- 覆盖 tab 创建、选择、命名、注释、父子关系和关闭。
- 覆盖 Composer 读取、设置和提交。
- 覆盖 terminal 输入、capture、scroll、wait 和进程生命周期。
- 覆盖 workspace、UI、事件和 server 观察。
- 破坏性调用必须显式、可审计、可重放判断，并保留 remain-on-exit、显式关闭和树安全语义。

### 常驻、异步和事件循环

要接近 Node.js / Bun 的用途，运行时不能永远停留在“调用一个函数后立即退出”的模型。

v0.1.8 至少需要确定：

- timer 和异步子进程如何保持 worker 存活；
- HTTP、进程输出和 Fleet event 如何进入同一个任务调度模型；
- 没有未完成任务时如何自然退出；
- Ctrl+C、parent exit、server restart 和 timeout 如何取消所有任务；
- 背压和并发上限如何避免拖垮 GUI；
- 长任务是否继续使用独立 worker，而不是把 Rhai engine 放进 GUI。

推荐保留 sidecar 隔离，但把 sidecar 从“受限安全盒”升级为“通用脚本进程”：

```text
agenterm.exe / agenterm-cli.exe
  -> 启动 agenterm-rhai.exe worker
      -> worker 拥有 Rhai engine + 标准库 + 事件循环
      -> 通过 typed broker 使用 Fleet API
      -> 崩溃、退出或取消不影响 GUI / PTY
```

### 扩散方案与初步收敛

| 问题 | 候选 | 当前倾向 |
|---|---|---|
| 默认模式 | local、observe | `agenterm-rhai.exe` 的普通本地入口使用 local；未来 agent 层自行选择工具入口 |
| 模块格式 | 单文件 import、包目录 | 先支持本地文件和目录模块 |
| 任务清单 | JSON、TOML、Rhai | 优先 TOML 或现有 manifest 风格，避免执行配置 |
| 网络 API | 低层 socket、HTTP/fetch | 先做 HTTP/fetch，socket 延后 |
| 异步模型 | promise、task handle、回调 | 先确定 task handle + wait/stream，避免假装 JavaScript |
| 包管理 | 本轮做、延后 | 公共 registry 延后，本地模块本轮做 |
| Fleet API | 少量白名单、catalog 映射 | 系统映射 typed catalog，不把运行时做成人为残缺版本 |
| 破坏性操作 | 永久禁止、显式调用 | 运行时支持显式调用并保留产品数据一致性；agent 权限由 agent 层决定 |
| 命名命令入口 | CLI、命令面板、状态条 | CLI + 命令面板；状态条不做命令仓库 |

### 与 Node.js / Bun 的边界

本轮不声称：

- JavaScript 语法兼容；
- Node 内建模块兼容；
- npm 包兼容；
- Bun API 或运行性能兼容；
- 可以直接执行现有 Node 项目。

本轮要证明的是：

- 同样可以承担日常本地自动化；
- 有实用的文件、环境、进程、网络、模块和任务能力；
- 启动与分发更轻；
- 与 AgenTerm Fleet 的结合比外部 Node/Bun 更直接；
- 未来 agentic 层仍可以选择 pure、observe 或其他受限工具入口。

### 验收证据

- `script api --json` 与实际运行时 API 完全一致。
- 黑盒测试覆盖文件、目录、环境、子进程、HTTP loopback、timer、JSON、bytes、模块、任务和 Fleet 控制。
- 网络测试使用独立 loopback fixture，不依赖公网。
- 进程测试覆盖 argv、空格、Unicode、cwd、env、stdin、stdout、stderr、非零退出、timeout、cancel 和父进程退出。
- 文件测试使用独立临时根，并覆盖原子写入、Unicode 路径、长路径、失败和清理。
- local 模式证明通用能力；pure / observe 继续证明各自既有的确定性和只读语义。
- Fleet 修改返回 typed receipt，并通过公共状态和事件验证事后结果。
- close、kill、send、server restart 等路径不会假成功或重复执行。
- worker crash、脚本错误和未处理任务不会影响 GUI、PTY 或 workspace。
- 诊断和审计不得意外记录文件内容、环境秘密、HTTP 凭据或终端内容。

## 五、最高优先级分支丙：完整公共自反馈旅程

### 用户成果

发布候选可以演示一条真实的人机共同日用流程；失败时自动说明自己在什么状态失败，并且不抢前台、不遗留资源。

### 旅程

1. 使用独立路径和 no-activate 启动 release GUI。
2. 观察第一窗口异步出现和终端就绪。
3. 创建根节点和子节点，保留稳定 ID。
4. 重命名、写注释、折叠、展开，并用键盘跨区域导航。
5. 编辑并且只提交一次 Composer 内容。
6. 在真实 PowerShell/cmd tab 上依次观察 proxy Prepared、Submitted 和
   经过 shell/child 验证的 Applied，再证明 Failed、脱敏和新 tab 不继承。
7. 选择、复制终端文本并使用回滚区。
8. resize、最小化、恢复，并验证 PTY 网格真实状态。
9. 调用一个命名 Rhai 任务，完成本地文件、子进程和 Fleet API 流程，并验证结果、收据和事后状态。
10. 保留一个已经退出的标签，再显式关闭它。
11. detach、reattach，并验证 PID、epoch、标签、PTY 连续性。
12. 停止独立 server，证明测试拥有的资源全部消失。

### 证据策略

- 每步记录机器可读 manifest：命令、时长、退出或结果类别、解析后的 server/tab/build 身份、前后位置和证据引用。
- 只在有意义的状态转换点保存 `ui-snapshot`、相关 pane/workspace/settings 状态和 PNG；不把截图当装饰。
- 默认隐藏包含内容的参数和秘密。
- 失败只保留一个有界诊断包；成功运行删除瞬态目录。
- 只有 CI 确实需要时才增加 JUnit 兼容摘要。

### 验收证据

- 每个步骤验证事后状态，不只看退出码。
- 证据只来自 release 制品的公共接口。
- 禁止固定 sleep 和私有状态钩子。
- 所有 GUI 路径继承 no-activate。
- 清理证明没有测试拥有的 PID、HWND、PTY worker、script worker、注册记录或临时秘密残留。

## 六、最高优先级分支丁：正式资格测试与发布

### 用户成果

发布过程可预测：错误尽早出现并带可执行诊断；公开下载的文件就是通过资格测试的文件。

### 候选流水线

```text
干净提交
  -> 只读预检
  -> 只构建一次候选
  -> 单测、公共行为、压力和日用旅程资格测试
  -> 生成绑定 hash 的资格收据
  -> 不重建，只打包合格字节
  -> 验证 package manifest 和 ZIP hash
  -> 创建 v0.1.8 tag
  -> GitHub workflow 验证同一 commit、receipt 和制品
  -> 发布 GitHub Release
```

### 计划范围

- 把 v0.1.7 的内部 dry-run 打包边界推广为公开候选模式。
- 一个协调器拥有构建与资格测试；package 不允许调用 Cargo。
- 记录排队、缓存、阶段、候选到合格、tag 到 Release 的时间。
- 给 Cargo registry、Git 和兼容构建输出增加有界且键正确的 CI 缓存；缓存缺失或损坏只能影响速度，不能影响正确性。
- GitHub Actions 继续固定 revision，并保持最小权限。
- CI 失败上传受限制的首错诊断包。
- 保留 GUI 4 MiB、各 sidecar 预算、一秒第一窗口和 no-activate 门。
- 创建 `v0.1.8` 前，先用非公开测试引用或显式 dry-run 演练。

### 验收证据

- 一个干净候选 SHA 只有一份完整且包含压力测试的 receipt。
- required gate 与实际发出的 evidence ID 完全匹配版本化 manifest。
- package 自测拒绝 HEAD 过期、dirty source、EXE/SBOM/lock/manifest 被改、缺门、跳过压力测试和 ZIP 篡改。
- Release workflow 拒绝 tag、version、receipt、commit 不一致。
- GitHub Release 资产 hash 与本地合格 package manifest 相同。
- 重复执行 package 不会重新构建，并且输入字节完全相同。

## 七、第一优先级分支戊：工作区视觉和操作层级

本分支等待三个最高优先级产品分支接口稳定后再进入实现，不把 v0.1.8 扩大成一次全面视觉重做。

### 重点区域

```text
左侧标签区底部
  Tabs 恢复或隐藏入口
  New
  Settings
  未来命名命令入口

终端主区底部
  Composer 输入框
  Send
  草稿、提交中、错误和焦点状态

全窗最底部
  CWD
  Proxy 与可见性
  未来受控 status segment
  降级和 last-known-good 状态
```

### 设计目标

- 明确区分“标签树操作”“当前终端输入”“全局或当前标签状态”三种层级。
- 不让左侧按钮区、Composer 和状态条争夺同一视觉主层级。
- 按钮、文本基线、边界和间距遵守一致整数网格。
- 在隐藏标签区、窄窗口、长 CWD、Proxy 隐藏、中文和高缩放下仍有确定退化策略。
- 命令发现优先使用键盘入口，不为了功能数量无限增加永久按钮。
- 选中标签名称和注释在默认 250 px 宽度下不被三个操作按钮挤成不可读。
- 标签名称和注释在目标树行原地编辑，Composer 始终只服务当前
  terminal draft，绝不兼任标签属性编辑器。
- Settings 只整理信息架构，不在本轮承诺任意主题导入导出。

### P0 bug：tab-scoped proxy 真实状态与应用证明

本项是 v0.1.8 公开候选的 P0 correctness bug，不是未来计划，也不只是
状态条换文案。proxy 继续是 stable-tab-scoped、ephemeral 的便利能力：
不得持久化，不得从一个 live shell 的临时修改推导另一个 tab 的环境。

#### 状态机

```text
Off
  Prepare
    -> Prepared
       只有敏感 Composer draft；不是 On，不写 PTY

Prepared
  Send / Send Now
    -> Submitted
       只证明一次提交已经开始；字节写入不是生效证明

Submitted
  non-secret marker
  + shell environment verification
  + real child inheritance verification
    -> Applied

Prepared / Submitted / Applied
  reject / timeout / mismatch / cancel / terminal or process exit
    -> Failed(reason=redacted typed category)
```

- `Prepare` 只为目标 tab 生成或替换一个 sensitive Composer draft，状态必须
  是 `Prepared`；不修改 shell，不创建 child，不显示 `On`。
- `Send` 与 proxy editor 的 `Send Now` 走同一个 Composer
  exactly-once submission，进入 `Submitted`；不能以 `WriteFile`/PTY byte
  write 成功冒充应用成功。
- `Applied` 同时要求非 secret shell marker、实际 shell environment
  post-state 和真实 child environment 继承验证。marker 只携带
  request/operation correlation，不含 URL、credential 或环境值。
- 拒绝、marker 错误、环境/child 不一致、timeout、cancel、terminal
  exit 或 process exit 都进入 `Failed`，并保留可操作但不泄密的原因。

#### GUI 与隐私

- proxy editor 必须提供鼠标可达且有 accessible name/tooltip 的
  `Reveal`/`Re-mask`、`Prepare`、`Send Now`；不能要求用户知道隐藏快捷键。
- Reveal 只改变当前 editor 的展示，不能改变 persistence、draft、
  application state、event、receipt 或日志内容。
- `ui-snapshot` 暴露 stable target、`revealed`、真实
  `Off|Prepared|Submitted|Applied|Failed`、validation/error category 和
  editor/action bounds，但不包含 proxy URL、credential、prepared command、
  secret environment value 或 Composer text。
- workspace/settings/event/receipt/diagnostic/log 永不保存 proxy secret；
  receipt 只允许 redacted fingerprint 和非 secret result category。

#### Shell 与创建语义

- `cmd.exe` 与 PowerShell 必须用真实 shell 和真实 child 验证环境继承、
  marker 顺序、退出/失败与 exactly-once。
- Bash-compatible 命令同时设置 `HTTP_PROXY`、`HTTPS_PROXY`、
  `http_proxy`、`https_proxy`，四者值一致并接受同一隐私门。
- direct TUI 正拥有输入，或 tab 以 non-interactive command 启动时，
  runtime injection 必须显式拒绝；诊断引导用户新建带 `--proxy` 的 tab，
  不能把 secret setup 强行写入当前输入流。
- 新 tab 不继承 active shell 内部临时 `set`/`$env:`/`export` 修改；
  只有显式 create-time `--proxy` 或同等 tab environment 参数可以注入。

#### Typed 证据与公共黑盒

- 每次状态转换发出 typed event 和 receipt，绑定 request ID、stable
  server/tab identity、epoch/sequence baseline、redacted fingerprint、
  result category 与 verified post-state。
- 黑盒依次证明 Prepared-not-On、Submitted-not-Applied、cmd/PowerShell
  shell+child Applied、Bash 四变量、exit/failure、direct-TUI 与
  non-interactive rejection、新 tab 无意外继承、隐私脱敏和 exactly-once。
- 每个路径证明没有 test-owned shell、child、worker、native editor、
  window、server 或注册记录残留；失败后下一次普通 Composer 和 proxy
  操作仍健康。
- 远程 proxy 分发、fleet-wide/global 默认 proxy、持久 profile、secret
  同步和 revocation 属于另行规划，需要身份、存储、策略与撤销门。

### Tabs UI 纵切：行内编辑与紧凑树

本纵切先固定产品合同和纯几何，再由拥有 `src/lib.rs` 的宿主分支接入
HWND、paint、hit-test、snapshot 与黑盒测试。不能只移动绘制坐标，也不能
在宿主里继续复制 `sidebar_width - 72/-48/-24` 或 `node_x + 24`。

#### 用户状态机

```text
Normal(@id)
  Edit / add-child
    -> Editing(@id, original, draft, validation=clean)

Editing
  Save / Ctrl+Enter + valid name
    -> atomic persist(name,note)
    -> Normal(@id)

Editing
  Save / Ctrl+Enter + blank name
    -> Editing(@id, draft retained, validation=blank-name)

Editing
  Cancel / Esc / target-invalidating transition
    -> discard draft
    -> Normal or target transition
```

- `Editing` 只由稳定 tab ID 识别，不能由当前 index 或可变显示名识别。
- 行内使用两个单行 native edit overlay，分别覆盖该行的 name 和 note
  显示矩形；编辑时 `+`/`Edit`/`Close` 切换为 `Save`/`Cancel`。
- 只有显式 Save 和 `Ctrl+Enter` 提交；`Tab`/`Shift+Tab` 在两字段与
  Save/Cancel 间移动，`Esc` 取消。
- 空白 name 的保存是可恢复验证错误：不保存 note、不退出编辑、不清空
  draft。
- 切换 tab、隐藏 Tabs、从其他入口关闭目标、workspace reload、窗口
  detach/stop/close 都先取消 draft，再执行原转换；不允许隐式保存。
- 普通窗口失焦不提交也不取消；同一行内部的焦点移动也不取消。
- add-child 先以正常初始值建立真实 child，再立即编辑 child 行；取消只
  恢复初始值，不删除 child。
- 任一时刻至多一个 editing row；开始编辑另一行时先取消旧 draft。

#### 纯几何合同

```text
TreeRowGeometry
├─ mode: normal | editing
├─ row / selection
├─ node anchor / expander / disclosure hit / status lamp
├─ text
│  ├─ name
│  └─ note
├─ editors?: name + note
└─ actions
   ├─ density: full | compact
   ├─ normal: add-child + Edit + Close
   └─ editing: Save + Cancel
```

- 根锚点从 17 左移到 12，标准逐层 indent 从 16 收紧到 12。
- 响应式连接线锚点和 row 节点使用同一个函数；180 px 时统一压缩每层
  indent，而不是让深层节点随机重叠或只移动文字。
- 180 px 使用仍可点击、具有 accessible name/tooltip 的紧凑图标动作；
  220 px 及以上可使用完整文字动作。
- 最深受支持层级仍保留至少一个 CJK glyph 加 ellipsis 的文本预算。
- normal/editing 动作、text、两个 editor 均有界且互不重叠；退化宽度
  只允许安全折叠，不允许反向矩形或越过 sidebar。

#### 当前纵切状态与依赖

- [x] `PRD_02_06_human_workspace.md` 拥有编辑、取消、键盘、snapshot、
  黑盒与非目标合同
- [x] `src/ui_geometry.rs` 提供 normal/editing、name/note/editor/action
  矩形、full/compact 动作密度、响应式 connector anchor 与定向单测
- [ ] `src/lib.rs` 同一提交接入编辑状态、两个 native edit HWND、paint、
  hit-test、connector 与 action label；不允许分批接入坐标
- [ ] `ui-snapshot` 暴露 stable target、mode、validation、dirty facts、
  density 和完整几何，不默认泄露未保存 draft
- [ ] 公共 CLI 黑盒证明 Save/Cancel/blank-name/切 tab/隐藏 Tabs/目标
  close/窗口 close 的事后状态，并证明 Composer draft 不变
- [ ] 截图覆盖 180/250 px、深层 CJK、normal/editing 与 validation error

#### 推荐宿主接入顺序

1. 引入单一 `TabInlineEditState`，只保存 stable ID、original、draft 和
   validation；先实现纯状态转换及 atomic Save。
2. 将 connector paint、row paint、selection 和 hit-test 一次性切换到
   `tree_row_geometry_for_mode` / `tree_connector_x`，同时删除旧魔数。
3. 以 name/note editor rect 创建、移动和销毁两个 native edit HWND；
   Composer 路径不参与。
4. 接入 Save/Cancel、`Ctrl+Enter`、`Esc`、Tab focus chain 和
   blank-name inline error。
5. 在 tab switch、Tabs hide、target close、workspace reload 和窗口
   生命周期入口集中调用同一个 cancel-before-transition helper。
6. 扩展 snapshot 后再写公共黑盒和截图；最后检查无 orphan HWND、无
   隐式保存且 Composer draft 字节不变。

### 当前问题树

- 标签区底部的 `Tabs`、`Settings`、`New` 被塞进侧栏最后一条区域，但 geometry 没有把它建模为独立操作栏。树的绘制、滚动和命中范围也没有明确截止边界。
- 按钮宽度由剩余像素临时计算。侧栏接近 180 px 时，`Settings` 最先被挤压；主操作 `New` 反而在最右且视觉最弱。
- 侧栏隐藏后三个按钮同时消失，目前只有状态条中的 `Tabs` 恢复入口。
- 状态条的分段顺序基本合理，但 CWD 同时承载路径、来源和 pending，Proxy 同时承载状态、端点和眼睛按钮，容易截断。
- 自绘状态段看起来像文本，却可以点击；缺少清晰的可操作提示、键盘落点和辅助功能角色。
- provider 区仍有占位文案；标签区可见时左侧的 `Status` 没有提供实际信息。
- Composer 固定 78 px。标题行同时显示目标和长快捷键，真正的输入框只有约 38 px 高，却承担多行输入。
- 单行场景浪费高度，多行草稿又不够用；pending、敏感内容、失败和退出状态没有形成稳定的信息层级。
- 输入区、状态条和侧栏按钮形成连续三层密集边框，视觉噪声偏高。

### 扩散布局方案

#### 方案一：工作台底座，推荐

保留“左侧 Tabs、右侧 Terminal”的主骨架：

- 左侧底部建立独立操作栏；
- 右侧保留位于状态条上方的 Composer；
- 最底部继续使用一条全窗状态条。

优点：

- 标签导航、当前终端输入、全局或标签状态三种作用域最清楚；
- 改动局部，不侵入主终端渲染；
- 可以复用现有 typed actions；
- geometry、snapshot 和黑盒测试容易形成一一对应。

#### 方案二：统一底部命令坞

把侧栏按钮、Composer 和状态合并成一个跨全窗底座。

优点是空间可以统一调度；缺点是全局、标签级和终端级作用域混在一起，响应式、焦点和隐藏逻辑明显更复杂。本轮不选。

#### 方案三：顶部应用栏

把 `New`、`Tabs`、`Settings` 移到窗口顶部，底部只保留 Composer 和状态。

它更接近传统桌面应用，但会消耗终端行、侵入窗口主视觉，并引出一次更大的布局重做。本轮不选。

### 推荐方案：工作台底座

一句话：

> 把现有底部拼装区收敛成有明确作用域、可以响应式退化、可以结构化观察的工作台底座，不改变主窗口骨架。

### ASCII 结构草图

以下是信息结构草图，不是最终像素稿。方框表示作用域和几何所有权，文字长短不代表最终控件宽度。

#### 正常宽度

```text
+--------------------------------------------------------------------------------+
|                                  AgenTerm                                      |
+------------------------+-------------------------------------------------------+
|                        |                                                       |
|  Tabs / 舰队导航       |  Terminal / 当前标签的主工作面                       |
|                        |                                                       |
|  [-] @1 coordinator    |  > cargo test                                         |
|   +-- @2 worker-a      |    ...                                                |
|   +-- @3 worker-b      |                                                       |
|  [+] @4 logs           |                                                       |
|                        |                                                       |
|  标签树只在本区滚动    |                                                       |
|  不进入下方操作栏      |                                                       |
|                        |                                                       |
+------------------------+-------------------------------------------------------+
| [ + New ]   <弹性空白> |  == 拖动调整 Composer 高度；双击恢复 ==              |
|              [Tabs]    +-------------------------------------------------------+
|           [Settings]   |  Composer · @2 worker-a                    [Ready]    |
|                        |  +-----------------------------------+  +----------+  |
|  侧栏操作栏            |  | 输入或编辑当前标签的多行草稿      |  |   Send   |  |
|  不属于标签树滚动区    |  +-----------------------------------+  +----------+  |
+------------------------+-------------------------------------------------------+
| [CWD  D:\dev\agenterm]     [受限 Provider / 无内容时保持空白]    [Proxy: Off] |
+--------------------------------------------------------------------------------+
```

从上图可以直接看出四种作用域：

```text
Tabs 树          负责“去哪里”
侧栏操作栏       负责“管理舰队导航”
Composer         负责“向当前标签做什么”
全窗状态条       负责“我现在处于什么上下文”
```

#### 窄窗口

```text
+--------------------------------------------------------------+
|                         AgenTerm                             |
+--------------+-----------------------------------------------+
| Tabs         | Terminal                                      |
| @1 main      |                                               |
|  + @2 work   |                                               |
|  + @3 logs   |                                               |
|              |                                               |
+--------------+-----------------------------------------------+
| [ + New ]    | == Composer 调整条 ==                          |
| [T]      [S] +-----------------------------------------------+
|              | Composer · @2                     [Sending]    |
| 紧凑按钮仍有 | +--------------------------+  +-------------+ |
| tooltip 和   | | 多行草稿                  |  |  Sending... | |
| accessible   | +--------------------------+  +-------------+ |
| name         |                                               |
+--------------+-----------------------------------------------+
| [CWD  D:\dev\age...]            [Proxy: Applied]             |
+--------------------------------------------------------------+
```

窄窗口的退化顺序：

```text
先删除无内容的 Provider
  -> 再省略 CWD 中间内容
    -> Tabs / Settings 进入紧凑态
      -> Proxy 只保留 On / Off
        -> 必要恢复入口和安全状态始终保留
```

#### Tabs 隐藏

```text
+--------------------------------------------------------------------------------+
|                                  AgenTerm                                      |
+--------------------------------------------------------------------------------+
|                                                                                |
|  Terminal / 获得完整窗口宽度                                                   |
|                                                                                |
|  > long running work                                                           |
|                                                                                |
|                                                                                |
|                                                                                |
+--------------------------------------------------------------------------------+
|  == 拖动调整 Composer 高度；双击恢复 ==                                        |
+--------------------------------------------------------------------------------+
|  Composer · @2 worker-a                                            [Ready]    |
|  +---------------------------------------------------------+  +------------+  |
|  | 输入或编辑当前标签的多行草稿                            |  |    Send    |  |
|  +---------------------------------------------------------+  +------------+  |
+--------------------------------------------------------------------------------+
| [Tabs 恢复] [CWD  D:\dev\agenterm]       [Provider]             [Proxy: Off] |
+--------------------------------------------------------------------------------+
```

隐藏 Tabs 后：

- 左侧导航和它自己的操作栏一起释放宽度；
- Composer 仍然只属于当前活动标签；
- 全窗状态条保留稳定位置；
- `Tabs 恢复` 成为不可被截断的第一优先级入口；
- 不把 `New` 或 `Settings` 临时塞进状态条，避免作用域混乱。

#### 几何所有权树

```text
Window
|
+-- Body
|   |
|   +-- Sidebar
|   |   |
|   |   +-- TabTree
|   |   |   +-- rows
|   |   |   +-- tree scroll / hit-test
|   |   |
|   |   +-- SidebarToolbar
|   |       +-- New
|   |       +-- flexible space
|   |       +-- Tabs
|   |       +-- Settings
|   |
|   +-- ActiveTabWorkspace
|       |
|       +-- TerminalViewport
|       |
|       +-- Composer
|           +-- resize grip
|           +-- header / submission state
|           +-- native edit
|           +-- Send
|
+-- GlobalStatusBar
    +-- Tabs recovery
    +-- CWD
    +-- bounded Provider region
    +-- Proxy state / reveal action
```

关键几何不变量：

```text
TabTree.bottom          == SidebarToolbar.top
TerminalViewport.bottom == Composer.top
Body.bottom             == GlobalStatusBar.top
SidebarToolbar.bottom   == GlobalStatusBar.top

任何树行不得进入 SidebarToolbar
任何 Composer 子矩形不得进入 GlobalStatusBar
任何状态段不得覆盖 Tabs 恢复或 Proxy 安全状态
```

#### 信息层级

```text
Terminal
  主工作面

Composer
  当前活动标签的主动作面

Tabs
  舰队导航

状态条
  只陈述当前上下文和少量受限动作

Settings
  全局次级动作
```

#### 左侧操作栏

- 独立建模为侧栏操作栏，不再作为树剩余空间的一部分。
- 建议高度使用 44 至 46 个 DPI 逻辑单位。
- 上方一条 1 单位分隔线，左右内边距 8，按钮间距 6。
- 按钮高度 32 至 34。
- 建议顺序：

```text
[＋ New]  [弹性空白]  [Tabs] [Settings]
```

- `New` 使用主按钮层级；`Tabs`、`Settings` 使用次级层级。
- 保持 `Tabs` 位于 `Settings` 左边，符合当前产品关系。
- 侧栏接近 180 逻辑单位时，先把 `Tabs`、`Settings` 变为紧凑按钮，并保留 tooltip 与 accessible name；绝不把按钮压缩成零宽。
- 再窄时沿用现有效侧栏归零策略，依靠状态条的 `Tabs` 恢复入口。
- 标签树可绘制、滚动和命中的范围必须截止于 `toolbar.top`。

#### Composer

- 继续放在右侧终端区、状态条上方，不跨越侧栏。
- 推荐默认高度 76 至 80 逻辑单位，最小 64，最大 160 或客户区高度的 40%，取较小者。
- 顶部提供 4 单位拖拽条，双击恢复默认高度。
- 标题行高度 20 至 22；编辑框和 `Send` 底线严格对齐。
- 标题只显示：

```text
Composer · @ID 名称
```

- 右侧只保留一个短状态：
  `Ready`、`Sending`、`Exited` 或 `Sensitive`。
- 长快捷键提示移入 tooltip 或帮助，不常驻挤压标题。
- `Send` 建议宽 80 至 88、高 34；pending 时禁用并显示 `Sending`。
- 输入框至少显示两行。
- Composer 高度作为用户偏好持久化；窄或矮窗口只调整 effective height，不破坏 configured height。
- 如果本轮不做可调高度，至少把固定高度、header、input、send 和 resize grip 的矩形正式纳入纯 geometry。
- Composer 继续优先保留原生编辑快捷键，区域导航不能劫持文本内部的 Ctrl+方向键。

#### 全窗状态条

- 建议高度 28 个 DPI 逻辑单位，顶部一条 1 单位分隔线。
- 保持以下语义顺序：

```text
[Tabs 恢复，仅隐藏时] [CWD] [受限 provider 弹性区] [Proxy]
```

- 删除没有信息价值的 `Status` 和 provider 占位文案；没有 provider 时保持安静空白。
- CWD 以路径为主，来源与 pending 使用短徽标或 tooltip。
- Proxy 显示真实 `Off`、`Prepared`、`Submitted`、`Applied` 或 `Failed`；
  仅在用户明确 Reveal 时在 editor 内显示端点，任何时候都不显示凭据。
- 截断顺序：
  provider 先消失，随后 CWD 省略，最后 Proxy 保留真实状态的紧凑表示。
- `Tabs` 恢复和 Proxy 安全状态始终保底。
- 状态条 segment 必须有明确的可点击表现；不能继续只有鼠标命中却没有视觉或语义可供性。

### 响应式规则

所有规格使用 DPI 逻辑单位，不把现有 26、78 等裸像素直接当作跨 DPI 视觉规格。

| 客户区宽度 | 行为 |
|---|---|
| 不小于 720 | 完整按钮标签、CWD 和状态 |
| 480 至 719 | 隐藏长快捷提示，收缩 provider |
| 320 至 479 | `Tabs`、`Settings`、Proxy 使用紧凑态，CWD 省略 |
| 小于 320 | 只保留必要恢复和安全状态，所有矩形仍不得重叠 |

侧栏配置范围继续为 180 至 480，Terminal 保底 320。

### 键盘与辅助功能

- 保留 Ctrl+↑/↓/←/→ 的区域跳转。
- 候选增加 F6 与 Shift+F6，按顺序和逆序遍历：

```text
Tabs -> Terminal -> Composer -> 可操作状态段
```

- `Esc` 返回 Terminal；是否在所有模式中启用要结合原生编辑和 modal 仲裁。
- `New`、`Tabs`、`Settings`、`Send` 都需要稳定 accessible name、role、enabled/pressed 状态和可见的 2 单位焦点环。
- 紧凑态图标不能成为唯一语义，必须保留 tooltip 和 accessible name。
- 自绘的 `Tabs` 恢复、CWD、Proxy 应提供 UIA 语义或等价的可聚焦原生承载。
- 本轮可以暂缓完整读屏认证，但不能继续只有鼠标命中。
- pending、敏感草稿和退出状态不能只靠颜色表达。

### `ui-snapshot` 语义

建议增加或规范以下结构：

```text
layout.sidebar.toolbar
  bounds
  actions[]
    id
    label
    role
    bounds
    visible
    enabled
    compact
    focused
    action

layout.composer
  bounds
  header
  input
  send
  resize_grip
  mode
  draft_dirty
  line_count
  sensitive
  submission_state
  configured_height
  effective_height

layout.status_bar.segments[]
  id
  owner
  priority
  bounds
  visible
  enabled
  focusable
  action
  display_state
  truncated

proxy
  stable_tab_id
  state: Off | Prepared | Submitted | Applied | Failed
  revealed
  validation
  error_category
  editor_bounds
  reveal_action
  remask_action
  prepare_action
  send_now_action
  secret_fingerprint_redacted

window
  dpi
  scale
```

snapshot 绝不能回显草稿秘密或 Proxy 凭据。

### 几何与公共验收

- 纯几何测试证明 toolbar 不与树行、侧栏 resize grip、Composer 或状态条重叠。
- Composer 的 header、input、send、resize grip 全部有界且基线一致。
- 覆盖 100%、150%、200% DPI，1180×760、640×480 和极窄窗口，以及 Tabs 180、250、480、隐藏状态。
- configured Composer 高度与 effective 高度分开测试。
- 公共黑盒测试真实点击每个动作中心。
- 测试 F6、Shift+F6 和 Ctrl+方向键焦点旅程。
- 如果交付 Composer resize，真实拖动并双击复位。
- 覆盖长中文标签、长 CWD、Proxy 脱敏、pending 草稿和敏感态。
- 每一步断言事后状态，不只看退出码。
- Dark 与 Light 至少覆盖正常宽度、窄窗口、Tabs 隐藏、长中文/CWD、Composer pending 或敏感态。
- PNG 与同一 `ui-snapshot` generation 绑定，断言无重叠、无裁切、输入框与 `Send` 底线一致；不采用脆弱的整图逐像素 golden。

### 本轮明确不做

- 不重写自定义 GUI 框架。
- 不增加顶部应用栏或多栏 Dock。
- 不做任意拖拽重排、toolbar 个性化、动画或毛玻璃。
- 不把状态条扩成多行 Dashboard。
- 不开放 provider 联网或无限常驻。
- 不在状态条显示代理凭据。
- 不做移动端或触屏布局。
- 不借布局调整重写 Settings 或 CWD 的业务逻辑；Proxy 只修复本节 P0
  状态、鼠标操作和验证合同，不扩成全局/远程配置系统。
- 不把完整 UIA 或读屏认证冒充成已经完成；本轮只交付所需语义基础。

## 八、第一优先级分支己：因果上可以互相比较的证据

### 计划范围

- 增加 model sequence、render generation 和 last-painted sequence。
- 给 capture、screenshot 和机器可读输出增加：
  server PID/address/version、epoch、稳定 tab ID、采样事件位置、output position、viewport、render generation 和 truncation。
- 统一所有 wait 命令：
  稳定目标解析、起始位置、server restart、target close、elapsed、deadline、最后有界观察和恢复提示。
- workspace 使用同卷原子替换保存，并公开 revision、hash、path、commit position 和失败。

### 验收证据

- snapshot、capture、cell dump 和 PNG 可以指向同一因果点；不能时必须明确说明原因。
- 注入 restart、target close、journal gap、workspace 写入失败和渲染延迟时，不会返回假成功。
- 注入保存中断后，上一个可读 workspace 仍然存在。

## 九、依赖图

```text
Rhai worker + supervisor + typed protocol
  └─> 通用标准库与任务调度
       ├─> 本地模块和任务清单
       └─> Node.js / Bun 用途对标的脚本流程

v0.1.7 typed operations + receipt
  └─> 系统化 Fleet API 映射
       └─> 命名命令
            └─> 日用旅程中的脚本流程

终端单元与选择纯逻辑
  └─> 自动滚动 + 选词/选行
       └─> 终端资格矩阵
            └─> 完整日用旅程

事件位置 + 稳定身份
  ├─> wait 语义统一
  ├─> 截图因果元数据
  └─> 脚本事后状态验证

共享 TestHarness + qualification receipt
  └─> 完整 dogfood gate
       └─> 公开 package 模式
            └─> release 演练
                 └─> v0.1.8 tag / Release
```

关键路径：

```text
范围与 PRD 冻结
  -> 通用脚本运行时、Fleet API 和终端交互语义稳定
  -> 公共黑盒证据
  -> 集成日用旅程
  -> 干净资格收据
  -> 完全相同字节的 package
  -> release 演练
  -> 经批准后公开 tag
```

## 十、并行实施模型

### 所有权规则

任何时刻不得有两个 agent 同时编辑同一个热点文件。`src/lib.rs`、`Cargo.toml`、`check.ps1`、`PRD.md`、alignment manifest 和 workflow 必须串行单一所有。

并行优先发生在已经提取的纯逻辑模块和独立黑盒测试中，最后在 typed boundary 处汇合。

### 第零波：收敛产品合同，串行

- 冻结最高优先级范围和非目标。
- 把接受的计划叶子写入对应 PRD。
- 定义 capability ID、operation/API 变化、event/evidence ID 和验收命令。
- 画出文件所有权和接口边界。

退出条件：没有任何实现分支依赖尚未决定的权限或 wire contract。

### 第一波：独立基础，并行

| 分支 | 独占文件范围 | 交付物 |
|---|---|---|
| 终端语义 | selection、geometry 纯模块与测试 | 手势、词、行、自动滚动合同 |
| 脚本运行时 | script protocol、标准库、模块、任务调度、supervisor 与 worker 测试 | 通用本地运行时 + 专用 pure/observe 入口 + Fleet API |
| 测试旅程 | TestHarness 和 dogfood 测试文件 | step、evidence、cleanup 合同 |
| 交付 | qualification、package、preflight 脚本 | 公开候选模式和 fail-closed 自测 |
| 下方 UI/UX | 只读产品审查，随后独立 geometry/fixture | 推荐布局和响应式验收合同 |

任何分支都不得自行完成最终 `src/lib.rs` 集成，也不得启动互相竞争的 Cargo release build。

### 第二波：host 集成，以串行为主

- 把终端选择状态接入 Win32 owner。
- 把通用标准库、任务调度和 Fleet broker 接入 script worker。
- 接入命名命令发现与调用。
- 保持 renderer、IPC、worker 和 workspace 权限边界。
- 每个小而完整的增量之后运行对应单测和公共黑盒测试。

### 第三波：证据和 UX，可隔离并行

- 终端真实输入、中文和长输出矩阵。
- 脚本通用能力、专用执行方式、模块、进程、网络、崩溃和恢复矩阵。
- 完整日用旅程。
- 标签树和窗口下方区域可读性 fixture。
- 如果提升为本轮范围，再做因果证据元数据和 wait 统一。

每个测试使用独立 IPC、workspace、settings、registry 和 evidence 路径，并继承 no-activate。

### 第四波：收敛，串行

1. 审查所有并行交付，删除重复实现。
2. 跑格式、Clippy、单测、对齐检查和快速公共行为切片。
3. 提交一个干净候选。
4. 只跑一次完整 release、stress、dogfood 资格门。
5. package 只消费该 receipt。
6. 检查 hash、诊断包、target 大小和孤儿证明。
7. 演练 tag workflow。
8. 只有得到明确发布批准后，才创建并推送 `v0.1.8`。

## 十一、工作分解树

```text
甲、产品合同
  甲一、PRD 所有权与范围冻结
  甲二、operation、API、event、evidence catalog
  甲三、script 执行能力与未来 agentic 治理边界

乙、专业终端
  乙一、纯选择状态机
  乙二、有界自动滚动
  乙三、选词和可视行选择
  乙四、取消与捕获丢失集成
  乙五、中文、ANSI、resize、长输出矩阵

丙、通用脚本运行时
  丙一、通用 local 与既有 pure / observe 执行方式
  丙二、文件、路径、环境、进程、网络和时间标准库
  丙三、本地模块、任务清单和命名命令
  丙四、任务调度、流式输入输出、取消和自然退出
  丙五、typed Fleet API 系统映射与 receipt
  丙六、运行时能力、专用模式和故障隔离黑盒门

丁、自反馈
  丁一、step manifest schema
  丁二、完整日用旅程
  丁三、有界状态和 PNG 证据
  丁四、cleanup、orphan 与注入失败证明

戊、正式交付
  戊一、公开 qualification receipt 模式
  戊二、完全相同字节的公开 package 模式
  戊三、CI cache、计时和诊断上传
  戊四、workflow 演练与远端资产核验

己、工作区 P0 proxy
  己一、tab-scoped ephemeral 状态机和 sensitive Composer draft
  己二、Reveal/Re-mask、Prepare、Send Now GUI 与 snapshot
  己三、cmd/PowerShell marker、environment、child post-state 验证
  己四、Bash upper/lower 四变量和 create-time --proxy
  己五、direct TUI/non-interactive 拒绝与无隐式继承
  己六、typed event/receipt、隐私、exactly-once 与 orphan 黑盒门

庚、可选体验
  庚一、下方按钮、状态条和输入区布局
  庚二、选中标签行可读性
  庚三、键盘命令面板
  庚四、因果 render/capture 身份
  庚五、workspace 原子保存和 revision
```

## 十二、版本资格门

### 门零：范围

- 接受的最高优先级叶子已经进入规范 PRD。
- 每个叶子都有所有者、公共证据和明确非目标。
- `agenterm-rhai.exe` 与未来 `agenterm-agent.exe` 的职责边界已经明确，不把 agent 权限系统塞进运行时。

### 门一：终端

- 选择行为通过纯逻辑测试和真实物理输入测试。
- 中文、换行、宽字符复制证据精确。
- 长输出和 resize 预算有记录并通过。
- 取消后没有捕获、输入或 worker 残留。

### 门一-A：工作区 proxy

- proxy 只属于目标 stable tab，ephemeral 且不进入 workspace/settings。
- Prepare 只产生 sensitive Composer draft 和 `Prepared`；Send 只进入
  `Submitted`，两者都不显示 `On` 或 `Applied`。
- `Applied` 有非 secret marker、真实 cmd/PowerShell shell environment
  和 child inheritance post-state 三重证据；Bash 命令覆盖 upper/lower
  四变量。
- exit、failure、timeout、cancel、marker/environment/child mismatch 都
  得到 `Failed` typed receipt，不把 PTY byte write 当成功。
- Reveal/Re-mask、Prepare、Send Now 可由鼠标到达；snapshot 暴露
  `revealed` 和真实状态但不泄露 endpoint、credential、command 或 draft。
- direct TUI/non-interactive 注入被拒绝并引导 create-time `--proxy`；
  新 tab 不继承 active shell 临时修改。
- 公共黑盒证明 event/receipt/post-state、隐私、exactly-once 和无
  shell/child/worker/editor/window/server orphan。

### 门二：脚本

- 离线 catalog、标准库、模块系统与实际运行时完全一致。
- 文件、路径、环境、子进程、HTTP loopback、timer、JSON、bytes 和 stream 形成可用闭环。
- 本地模块、项目任务和用户命名命令能够发现、运行和报告错误。
- 现有 typed operation catalog 被系统映射为 Fleet API；修改操作返回 receipt 并验证事后状态。
- pure / observe 保持既有语义，但不限制 local 通用入口。
- timeout、crash、cancel、parent exit、server restart 后 GUI、PTY、workspace 与下一次脚本运行正常。
- 工具 schema 足以让未来 `agenterm-agent.exe` 在不解析帮助文本的情况下建立工具层。

### 门三：自反馈

- 使用 release 制品的完整日用旅程通过，并且没有固定 sleep。
- 每个转换点都有足够而受限的失败证据。
- 没有测试拥有的残留资源。

### 门四：交付

- required gates 和 evidence 100% 完整。
- 一个干净 receipt 绑定 commit、lock、toolchain、manifest、SBOM 与 EXE hash。
- package 不调用 Cargo，只消费合格候选。
- Release 演练验证远端 workflow 和资产 hash。

### 门五：公开发布决定

- 没有未解决的最高或第一优先级发现。
- README、PRD 与实际行为一致。
- 大小、启动和性能预算通过。
- 用户明确批准创建 `v0.1.8` 标签。

## 十三、范围控制

符合以下任一情况时，候选能力应拒绝或延后：

- 不能明显提升专业终端、通用脚本运行时、自反馈或同字节交付。
- 为了强调安全而把正常本地脚本能力人为阉割。
- 把 agent 权限、审批或自然语言决策塞进 `agenterm-rhai.exe`。
- 没有实测需求却增加系统级常驻 daemon；脚本自身按任务存活不属于此项。
- 没有先设计命令发现和键盘路径，就增加永久 UI。
- 削弱 remain-on-exit、显式关闭、树循环安全、隐私、no-activate、大小或第一窗口不变量。
- 声称未通过矩阵证明的兼容性。
- 测试和诊断成本明显超过本轮用户价值。

本轮变化预算：

- 新通用运行时 API：允许，但必须组成可用纵向闭环并有公共证据。
- 新可执行文件：零；本轮定义与未来 `agenterm-agent.exe` 的边界，但不偷跑实现。
- 文件、环境、进程和网络：属于 local 运行时正常能力，不按 agent 权限模型逐项审批。
- 新默认系统级后台服务：零。
- GUI 大小预算：不提高。
- 第一窗口预算：不提高。

## 十四、主要风险

| 风险 | 早期信号 | 应对 |
|---|---|---|
| v0.1.8 再次变成只有内部整理 | 没有可演示的用户旅程 | 终端选择和命名命令演示必须在关键路径 |
| 把“对标”误解为一次复制整个 Node 生态 | 任务开始追逐 npm 兼容和所有内建模块 | 先交付本地自动化纵向闭环，再扩展生态 |
| 运行时再次被安全模型阉割 | 每个正常 fs/proc/net 调用都需要白名单 | 权限治理留给未来 agent 层，script 层验证能力完整性 |
| script 与 agent 层混在一起 | runtime 开始处理审批、角色和自然语言策略 | 用工具 schema 协作，保持两个可执行层次独立 |
| 通用标准库变成一个巨大热点模块 | fs/proc/net/event loop 全塞入同一文件 | 按标准库域拆模块，统一 typed result 与取消合同 |
| `src/lib.rs` 成为并行合并瓶颈 | 多个 agent 同时要求编辑同一状态机 | 先提取纯合同，只有一个串行集成所有者 |
| 真实 GUI 测试变得不稳定 | 出现 retry 或固定 sleep | 使用公共 wait、稳定 ID、事件基线和首错证据 |
| 选择功能破坏 TUI 鼠标输入 | 出现不成对 mouse transition | 暂缓 raw mouse，或先建立明确仲裁和 fixture |
| 发布仍然很慢 | 同一候选反复构建 | 构建一次、资格一次、同字节打包，并计时 |
| cache 隐藏正确性问题 | 只有 cache hit 才通过 | clean 和损坏 cache 路径继续属于资格场景 |
| UI 微调不断扩张 | 截图变化却没有验收差异 | 视觉分支必须服务信息层级与日用旅程 |

## 十五、第一次评审需要决定的问题

1. v0.1.8 的通用运行时第一刀是否锁定为：
   文件/路径/环境、子进程、HTTP、timer、JSON/bytes、模块和任务？
2. 异步模型采用 task handle、future/promise 还是事件回调；怎样既适合 Rhai，又不假装 JavaScript？
3. 本地模块和任务清单使用什么格式与解析根？
4. 是否要求 v0.1.8 系统映射全部现有 typed operations，只有明确技术阻塞的操作可以暂缓？
5. `script api --json` 的副作用和工具 schema 要做到什么粒度，才能直接服务未来 `agenterm-agent.exe`？
6. 命名命令是否同时进入 CLI 和 GUI 命令面板？
7. raw application mouse 仲裁是否必须本轮完成，还是自动滚动、选词和选行可以先独立交付？
8. 标签树可读性、工作台底座和命令面板是 release blocker，还是通过最高优先级后再做的增强？
9. v0.1.8 计划成为公开 GitHub Release，还是只做到 public-ready，再由单独决定触发发布？

## 十六、建议的第一刀

```text
核心
  选择自动滚动 + 选词/选行
  Rhai local 通用运行时：fs/path/env/process/http/time/json/bytes
  本地模块、任务清单、命名命令和 CLI 调用
  现有 typed operation catalog 系统映射为 Fleet API
  pure / observe 作为专用执行方式继续兼容
  面向未来 agenterm-agent.exe 的机器可读工具 schema
  完整日用 dogfood
  同字节公开 qualification、package 和 release 演练

增强
  REPL 和 watch
  流式任务、事件订阅和 status provider
  下方按钮区、状态条区、输入区的专业化微调
  选中标签行可读性
  GUI 命令面板
  截图因果元数据
  workspace 原子保存

明确延后与未来计划
  agenterm-agent.exe 的具体实现、审批 UI 和 agent 权限系统
  agenterm-net.exe、libp2p/IPFS 节点、密钥系统和去中心化应用实现
  agenterm-mux.exe 原生 mux server、完整 pane 多路复用和多后端发布
  agenterm.exe 与 agenterm-cli.exe 的单文件合并
  通用 Rhai 稳定后按辅助脚本双跑 -> 测试编排 -> 资格/打包/发布顺序自托管
  PowerShell 实现保留到 Rhai 自举、Windows 语义和紧急回退证据通过
  npm 兼容、公共包仓库和第三方生态
  低层 socket 与不受治理的系统服务
  raw mouse 仲裁，除非真实 fixture 阻塞
  矩形选择
```

这一刀能够让 v0.1.8 同时做到：用户明显感觉终端更专业，`agenterm-rhai.exe` 第一次成为真正的通用自动化运行时，并为未来 `agenterm-agent.exe` 提供强大的工具基础；发布过程仍然复用同一批合格字节。
