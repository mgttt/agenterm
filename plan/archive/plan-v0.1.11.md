# AgenTerm v0.1.11 公开计划

> ## ⚠️ 已归档（2026-08-05）
>
> **本文是 v0.1.11 时期的历史执行记录，保留仅为追溯，不要作为执行依据。**
> 其里程碑证据仍被 `prd/PRD_02_18_roadmap.md` 引用，故整档保留原文未改。
>
> - 上一已发布版本复盘：`plan/archive/plan-v0.1.14.md`；在制：`plan/plan-v0.1.15.md`
> - 在制版本：`plan/plan-v0.1.15.md`
> - 发布链要求（版本无关权威处）：
>   `prd/PRD_02_17_delivery_quality.md` §Release-chain operating requirements
> - 结构 SSOT：`plan/ARCHITECTURE.md`


状态：实现收敛与发布候选验证中（不授权 tag / GitHub Release）
工作主题：**Control Center 基础、本地 IPC 原生化、安静工作台微调与
去中心化基建首个独立闭环**
版本定位：在不扩大日常终端主界面、不把新 UI 或网络职责塞入稳定 server
的前提下，为下一阶段的工作流、插件、应用、信息聚合和去中心化能力建立
可独立演进、可观察、可失败隔离的产品边界。

本文是版本执行计划与决策记录，不是产品事实。评审接受的范围、状态和
长期边界必须同步进对应的 `PRD.md` / `prd/PRD_*.md` owning node；实施完成后，
本文保留为交付历史。

## 一、树式精华

```text
v0.1.11  Control Center 基础与下一阶段平台入口
│
├─ P0：保持主工作台安静、紧凑、易操作
│  ├─ [x] 保留右侧语言按钮的用户改名结果：En|Zh
│  ├─ [x] 普通标签行移除 Edit 按钮，回收密集树宽度
│  ├─ [x] 单击正文选择；双击正文进入原地编辑
│  ├─ [x] disclosure、树线、状态灯和动作区双击不误触编辑
│  ├─ [x] 编辑态只显示 Save / Cancel；保存或取消后恢复普通态
│  └─ [x] F2 与稳定 ui-action 提供键盘和自动化等价入口
│
├─ P0：Control Center 独立客户端基础
│  ├─ [x] 终端上方工具栏中央增加 [Control Center]
│  ├─ [x] 用户入口与机器入口共享一个稳定 action identity
│  ├─ [x] 首选独立进程 agenterm-cc，而不是扩大 agenterm.exe
│  ├─ [x] 只通过公开 typed control plane 连接同一 server
│  ├─ [x] 关闭/崩溃/升级 Control Center 不影响 GUI、PTY 或 workspace
│  ├─ [x] 本轮交付壳层、导航、只读事实和明确空状态
│  └─ [x] 不在 Control Center 内复制第二份 Fleet authority
│
├─ P0：本地 IPC 原生 transport 与逻辑实例
│  ├─ [x] 先冻结 endpoint/instance/discovery/security 合同，再迁移默认值
│  ├─ [x] Windows 默认 named pipe；Linux/macOS 默认 Unix domain socket
│  ├─ [x] 用户可见逻辑实例为 {username}_main 与 {username}_dev
│  ├─ [x] 底层 pipe/path 使用 OS user scope + app namespace + 安全派生键
│  ├─ [x] CLI 支持 --instance main|dev；显式 endpoint 优先且冲突报错
│  ├─ [~] main/dev 可同时存在、严格隔离且各自保持单例
│  ├─ [x] server-list 同时发现新 transport 与旧 TCP registration
│  ├─ [~] stale socket、ACL/权限、路径长度、字符和 owner recovery 可验证
│  └─ [x] 显式 TCP host:port 保留为兼容、诊断及未来远程 transport
│
├─ P0：Control Center 产品树耦合推演
│  ├─ [x] Cockpit：同一 Fleet 的只读驾驶舱
│  ├─ [x] Workflow / Pipeline：真实空状态与未来工作流入口
│  ├─ [x] PluginHub：真实空状态与未来组件目录入口
│  ├─ [x] AppHub：真实空状态与未来应用目录入口
│  └─ [x] InfoHub：真实空状态与未来信息路由入口
│
├─ P1：libp2p / IPFS 独立基础实验
│  ├─ [~] 依赖、许可、体积、内存、线程与跨平台可行性门
│  ├─ [x] agenterm-net 独立进程原型，不链接 GUI/server
│  ├─ [x] 两个本地测试节点完成身份、握手、ping 与有界退出
│  ├─ [x] CID 生成/校验与临时块存储 put/get 闭环
│  ├─ [x] typed JSON 结果、预算、诊断和 orphan 清理
│  └─ [x] 成熟前不接入 server 常驻服务、不冒充稳定发布能力
│
├─ P1：系统 WebView host 方向
│  ├─ [x] 借鉴 Tauri 的“Rust host + 系统 WebView”思想，不承诺兼容 Tauri
│  ├─ [x] 比较原生 API、轻量封装和现有 Rust WebView 方案
│  ├─ [x] 冻结本地资源、消息桥、窗口生命周期和 capability 合同
│  ├─ [x] Windows / macOS / Linux 真实可用性与失败状态可发现
│  └─ [x] 本轮不强制 Control Center 改用 WebView
│
├─ P0：自反馈与交付
│  ├─ [x] snapshot/action 覆盖工具栏、标签手势和 Control Center 生命周期
│  ├─ [~] PNG 与结构化状态一致
│  ├─ [~] 独立进程 crash、missing、incompatible、restart 故障矩阵
│  ├─ [~] Windows/Linux/macOS × x86_64/ARM64 构建与能力事实
│  └─ [x] release gate 继续消费同一批 byte-qualified artifacts
│
└─ 明确延后与未来计划
   ├─ 完整可视化工作流图编辑器、调度器和跨机恢复
   ├─ PluginHub/AppHub 公共远程市场、交易、静默安装或自动更新
   ├─ InfoHub 自动执行外部信号、交易或承诺
   ├─ libp2p DHT、pubsub、relay/NAT、远程 Fleet attach 和常驻 full node
   ├─ 完整 IPFS DAG/pin/gateway/集群和大规模持久缓存
   ├─ 将 libp2p/IPFS 链接进 agenterm.exe 或 agenterm-server
   ├─ 用 WebView 重写主终端窗口或强制 Control Center 本轮采用 WebView
   ├─ 未经认证/威胁模型即把显式 TCP 开放为公网远程控制
   ├─ 在跨版本门未通过前移除旧 TCP registration/连接能力
   └─ Agent 权限、审批、凭据和策略；这些属于未来 Agent harness
```

当前收敛说明（2026-07-31）：

- `[x]` 表示实现与 owning evidence 已进入 `main`，不是发布授权；
- `[~]` 主要剩余项是原生 Unix CI 的最终运行结果、Control Center 扩展
  故障矩阵、跨平台 PNG 边界和 exact-HEAD 候选 qualification；
- `agenterm-net` 与 WebView 仍保持研究/host-spike 身份，完成本轮合同不等于
  晋升为稳定常驻服务或生产 renderer；
- 只有六目标 CI、完整 qualification receipt 和 release rehearsal 对同一
  clean commit 全部通过后，才可称为 release candidate；仍须用户另行批准
  才能创建 tag 或 GitHub Release。

## 二、用户问题与版本 outcome

### 2.1 用户问题

1. 主工作台已经形成稳定的 Tabs + Terminal + Composer 结构，但标签行的
   常驻 `Edit` 动作占用宝贵宽度；密集 Fleet 中，编辑应该更直接而不是让
   每一行常驻一个低频按钮。
2. Workflow、PluginHub、AppHub、InfoHub 等能力如果继续堆进终端主窗口，
   会破坏“界面简单实用”的产品特色；它们需要一个明确但不打扰日常工作的
   二级入口。
3. 原 v0.2.0 `Fleet Hub` 设想偏向一个 Settings 级 overlay，但未来内容会
   包含复杂图形、目录、详情、安装流程和信息视图，生命周期和技术栈都需要
   比主终端 UI 更独立。
4. libp2p/IPFS 不能永远停留在灵感文档中，但也不能未经独立成熟就进入
   server 权威和 PTY 热路径。
5. 未来高密度可视化不能全部依靠手写原生控件；需要评估系统 WebView，
   同时避免直接继承 Tauri 的全部依赖、运行时和产品假设。
6. 当前本地 IPC 以 TCP 地址和端口表达实例；开发/测试容易产生随机端口、
   stale registration 和用户难以理解的实例关系。操作系统已有更适合本机
   单用户 IPC 的 named pipe / Unix domain socket，但切换不能破坏
   `server-list`、显式 TCP、旧 GUI/CLI 或已运行 server。

### 2.2 一个具体 outcome

> v0.1.11 发布时，用户可以在保持安静、紧凑的主终端工作台中，通过工具栏
> 中央的 `Control Center` 入口打开或聚焦一个崩溃隔离的二级客户端；该客户端
> 从同一 server 读取真实 Fleet 状态并展示可扩展导航，而标签编辑已改成
> Warp 式双击原地编辑。本机默认实例通过 Windows named pipe 或 Unix
> domain socket 连接，`main` 与 `dev` 可预测且隔离，TCP 继续作为显式兼容
> transport。同时，仓库拥有一个不连接稳定 server 的
> libp2p/IPFS 独立实验闭环，以及一份有证据的系统 WebView host 技术选择。

### 2.3 版本收敛原则

- **先壳后内容**：本版让 Control Center 成为可靠入口与扩展容器，不承诺
  一次实现四个大型产品。
- **一份权威，多种投射**：server 继续只拥有 Fleet、PTY、workspace、
  journal 和 receipts；Control Center 是可替换客户端。
- **独立进程优先**：复杂 UI、WebView、目录解析和未来网络消费不会增加
  `agenterm.exe` 首窗口延迟，也不会随主 GUI 崩溃或升级。
- **网络先独立成熟**：libp2p/IPFS 先在 `agenterm-net` 原型中证明协议、
  资源和清理，再讨论向 server 提供稳定服务。
- **本地连接先原生、迁移先兼容**：native local transport 成为最终默认，
  但必须经过 endpoint schema、双栈发现、跨版本和安全门，不能用一次
  flag-day 改动让旧 client 与新 server 互相失联。
- **能力不伪装**：壳、占位、原型和稳定能力必须在 catalog/snapshot 中
  使用不同状态，不用“按钮已经出现”冒充内容已实现。

## 三、产品信息架构

### 3.1 Control Center 作为二级控制面

暂定公开显示名为 **Control Center**，暂定可执行文件为
`agenterm-cc.exe` / Unix `agenterm-cc`。实施波次零要完成命名评审：

| 层次 | 暂定名称 | 说明 |
|---|---|---|
| 工具栏 | `Control Center` | 用户可见入口；窄宽度可显示 `CC`，语义名不变 |
| 进程 | `agenterm-cc` | 独立、按需启动、可替换的控制中心客户端 |
| 产品节点 | Control Center | Cockpit、Workflow/Pipeline、PluginHub、AppHub、InfoHub |
| 旧称 | Fleet Hub | 作为 v0.2.0 历史规划名；接受新名称后不并存两套产品 |

推荐选择独立进程，而不是继续采用原 v0.2.0 的 overlay-first 决策：

```text
agenterm.exe
  └─ toolbar action: open-control-center
       └─ 启动或聚焦 agenterm-cc --instance <当前 logical instance>
            └─ public typed IPC / event journal / waits
                 └─ agenterm-server（唯一 Fleet 权威）
```

独立进程的直接收益：

- Control Center 可以独立升级、崩溃、重启和切换原生/WebView 渲染器；
- 主 GUI 不链接 package、feed、libp2p/IPFS 或 WebView 依赖；
- 未来可从命令行直接启动，也可成为远程/多窗口投射；
- 多平台可以真实报告 native/WebView capability，而不阻塞终端主窗口。

### 3.2 主工具栏布局

```text
┌──────────────────────────────────────────────────────────────────────┐
│ [<Tabs] [New]            [Control Center]           [Settings][En|Zh][z|Z] │
└──────────────────────────────────────────────────────────────────────┘
```

布局合同：

- 左组保持 Tabs、New；右组保持 Settings、语言、字号；
- Control Center 位于两组之间的可用区域视觉中心，不通过硬编码窗口绝对中心
  与左右控件重叠；
- 宽度不足时先把显示文本压缩为 `CC`，语义 action、tooltip、snapshot 名称
  和键盘可达性不变；不换行、不挤压 Terminal 为负宽度；
- `En|Zh` 是当前确认的语言按钮显示，不把用户的手工 `Zh` 改回 `繁`；
- 显式鼠标/键盘打开时允许正常激活 Control Center；自动化或继承
  `--no-activate` 时必须保持前台窗口不变。

### 3.3 Control Center 首版导航

```text
Control Center
├─ Cockpit
│  ├─ server / epoch / sequence
│  ├─ tabs running / dead / detached 摘要
│  └─ inspect / select 等已存在 typed action
├─ Workflow / Pipeline
│  ├─ 本地 Rhai task catalog
│  ├─ workflow definition / run 的未来 schema 空状态
│  └─ 不把 task 列表谎称为 durable workflow
├─ PluginHub
│  ├─ runtime / tool / sidecar / plugin manifest
│  ├─ installed / available / incompatible 状态
│  └─ 安装事务未来只交给 softmgr
├─ AppHub
│  ├─ 由脚本、插件、模板和 UI 组合的应用 manifest
│  ├─ 本地/仓库/第三方 source identity
│  └─ 不与 PluginHub 共用一个含混的“所有东西市场”
└─ InfoHub
   ├─ source / subscription / item / route 模型
   ├─ 信息进入通知或 Composer 草稿
   └─ libp2p/IPFS 是未来 source/backend，不是本版常驻节点
```

### 3.4 PluginHub 与 AppHub 的边界

| 维度 | PluginHub | AppHub |
|---|---|---|
| 用户问题 | 给系统增加能力 | 给用户提供可直接使用的组合体验 |
| 典型内容 | runtime、CLI、sidecar、provider、connector | dashboard、workflow pack、workspace app、可视化工具 |
| 执行单位 | 独立组件或扩展模块 | 一个或多个组件 + 脚本 + 资源 + UI |
| 安装权威 | 未来 `agenterm-softmgr` | 仍复用 softmgr，不另建安装器 |
| 本版 | manifest/schema 与只读本地目录 | manifest/schema 与明确空状态 |
| 非目标 | 公共交易市场 | 第二套插件格式或私有 server |

### 3.5 InfoHub 的产品边界

InfoHub 不是新闻阅读器，也不是自动交易工具。它负责：

```text
source
  -> fetch / receive
    -> normalize
      -> verify provenance
        -> filter / predicate
          -> notify / Control Center card / Composer draft
```

本版只冻结 source、item、provenance、route 的最小对象关系和空状态；不让
外部信息自动执行 destructive Fleet action。未来 libp2p/IPFS 可以成为
source/transport，但 InfoHub 不拥有网络节点生命周期。

## 四、本地 IPC 原生 transport 与逻辑实例

### 4.1 产品模型：实例名不是 endpoint

用户选择的是逻辑实例，操作系统连接的是 endpoint。两者必须分开：

```text
用户/产品语义
├─ --instance main  -> 显示名 {username}_main
└─ --instance dev   -> 显示名 {username}_dev
          |
          v
Canonical InstanceIdentity
  app = agenterm
  scope = current OS user identity
  role = main | dev
  key = versioned safe derivation(scope, role)
          |
          +-- Windows -> named pipe endpoint
          +-- Linux   -> Unix domain socket endpoint
          +-- macOS   -> Unix domain socket endpoint
          └-- explicit TCP -> tcp://host:port
```

`{username}_main` / `{username}_dev` 是用户可读名称，不是安全标识，也不
直接成为 pipe/path。底层派生必须使用可信 OS identity：

- Windows 使用当前用户 SID 或等价稳定 security principal，经版本化哈希/
  编码形成短 key；
- Unix 使用有效 UID、可信 runtime directory ownership 和短 key；
- username 只用于经过长度/控制字符处理的 display label；
- canonical role 首版只接受 ASCII `main|dev`；未来自定义实例另开 schema，
  不接受任意字符串直接成为原生 endpoint；
- endpoint schema 携带 transport、namespace version、instance role、
  user-scope fingerprint 和安全显示值，但 diagnostics 不泄露完整 SID、
  home path 或凭据。

### 4.2 建议 endpoint

具体字面路径在平台 spike 后冻结，下面是语义而非必须照抄的字符串：

```text
Windows
  \\.\pipe\AgenTerm\<namespace-version>\<user-scope-key>\<instance-key>

Linux
  $XDG_RUNTIME_DIR/agenterm/<user-scope-key>/<instance-key>.sock
  fallback: 受当前 UID 独占且 mode 0700 的短 runtime directory

macOS
  当前用户私有 runtime/temp base/agenterm/<user-scope-key>/<instance-key>.sock
  不把长 home path 或原始 username 直接拼入 sun_path
```

安全合同：

- Windows pipe 创建显式 DACL，只允许当前 user SID 和必要的 LocalSystem；
  禁止继承宽松 ACL，首版拒绝 remote named-pipe client；
- Unix 父目录必须由当前有效 UID 拥有且 mode `0700`，socket mode `0600`；
  bind 前使用不跟随 symlink 的检查，connect 后在平台支持时校验 peer
  credentials；
- 任何 fallback base 必须先验证 owner/type/permissions，不能因为环境变量
  存在就信任；
- endpoint 长度在 bind 前验证；过长时使用固定长度派生 key，不截断到可能
  冲突的裸前缀；
- display username 中的斜杠、反斜杠、冒号、空白、控制字符、Unicode
  normalization 和超长输入只影响安全显示，不改变可信 OS scope；
- endpoint collision、ACL/permission 错误、unsupported transport 和
  namespace version mismatch 使用稳定 typed error。

### 4.3 CLI 与 endpoint 选择优先级

建议新增通用 endpoint 表达：

```text
--instance main|dev
--endpoint pipe:<opaque-local-name>
--endpoint unix:<absolute-socket-path>
--endpoint tcp:<host>:<port>
--address <host>:<port>          # 现有 TCP 兼容 spelling
```

解析优先级：

```text
1. 显式 --endpoint
2. 显式兼容 --address（等价显式 tcp endpoint）
3. 显式 AGENTERM_IPC_ENDPOINT
4. 兼容 AGENTERM_IPC_ADDRESS（等价显式 tcp endpoint）
5. 显式 --instance main|dev -> 该用户/平台派生的 local endpoint
6. 无参数 -> instance main -> 该用户/平台派生的 local endpoint
7. 仅对旧版本迁移期 -> 已登记的 legacy TCP alias / 固定默认端口探测
```

冲突不静默覆盖：

- 同时给出 `--endpoint` 与 `--address`：typed conflicting-selector；
- 显式 endpoint 与显式 `--instance` 同时出现：默认拒绝；若未来允许，
  必须把 instance 只作为预期身份断言并验证一致，不能忽略其中之一；
- CLI 参数优先于环境变量，但 diagnostics 必须说明环境值被显式参数覆盖，
  不输出可能敏感的完整 endpoint；
- `--instance dev` 绝不回退连接 main；
- Control Center、GUI、CLI、Script、MCP、mux 必须复用一个 resolver，不各写
  一套优先级。

显式 TCP 继续存在：

- loopback TCP 用于旧版本兼容、诊断、测试 fixture 和显式多 server；
- non-loopback TCP 是未来远程 transport，不因本次 selector 语法自动成为
  安全可用能力；认证、加密、peer identity 和 threat model 未完成时必须
  truthful Unsupported 或保持现有 loopback 限制；
- 不再为普通 main/dev 启动随机 TCP port；测试需要额外实例时优先使用
  isolated instance/runtime directory，确需 TCP 才显式请求端口。

### 4.4 main/dev 生命周期和单例

```text
current OS user scope
├─ main -> 最多一个 authoritative server
└─ dev  -> 最多一个 authoritative server
```

- main 与 dev 使用不同 endpoint、registration、workspace/settings 默认值、
  server epoch 和 lock identity；
- 一个 role 已有 live compatible authority 时，新启动复用它；
- 同一 role 有 live incompatible server 时 fail closed 并给出升级/显式
  endpoint 选择，不竞争 bind 或结束对方；
- main 和 dev 可以同时运行，任何无参数生产 GUI 不误连 dev；
- dev 的窗口标题、`server-list`、snapshot 和 diagnostics 必须明显显示 role；
- stable tab ID 只在各自 server epoch/authority 中成立，不跨实例混用。

单例不能只依赖“文件存在”：

- Windows 以 pipe creation ownership + versioned instance lock/lease 判定；
- Unix 以原子 bind + owner metadata/lock/lease 判定；
- PID 必须配合 process start identity/lease nonce，避免 PID reuse；
- 已连接的 server identity 必须与 registration 的 instance/transport/
  endpoint/epoch 相符；
- 并发启动的唯一赢家成为 authority，失败者有界连接赢家或返回 typed
  conflict，不各自转去随机 TCP port。

### 4.5 stale Unix socket 与恢复

只有以下条件全部满足，启动者才可删除 socket path：

1. path 通过 no-follow 检查且确实是 socket，不是 regular file、directory、
   symlink 或其他类型；
2. path 和父目录由当前有效 UID 拥有，权限符合合同；
3. bounded connect 得到确定的 dead/refused 结果，而非 timeout/permission；
4. registration/lock 的 PID + start identity/lease 已确认不存活或一致地过期；
5. recovery 在同一 instance lock 内完成，随后原子重新 bind；
6. 删除和 bind 之间的竞争被再次 identity 校验捕获。

若任一证据不足，返回 `ipc_endpoint_occupied` / `ipc_owner_unknown`，不删除。
named pipe 没有同形 stale filesystem node，但仍需处理 live pipe、创建者崩溃、
旧 registration 和 PID reuse。

### 4.6 registration 与 server-list 兼容

实例登记升级为 versioned endpoint union：

```text
registration
├─ schema_version
├─ logical_instance: main | dev | explicit
├─ display_name: <safe username>_main
├─ transport: named_pipe | unix | tcp
├─ endpoint: typed transport payload
├─ compatibility_endpoints: [typed endpoint...]
├─ pid + process_start_identity
├─ server build/protocol/epoch/session/workspace identity
└─ health / last_observed / stale reason
```

`server-list` 迁移要求：

- 保留现有 PID、ADDRESS、VERSION、STATUS、WINDOW、TABS、ACTIVE、SESSION、
  WORKSPACE 语义，新增 INSTANCE、TRANSPORT、ENDPOINT 或等价机器字段；
- 人类表格可以把 ADDRESS 泛化为 ENDPOINT，但 `--json` 必须保留旧字段的
  compatibility 读取期，TCP record 仍精确 round-trip；
- 同一 server 的 primary local endpoint 与 compatibility TCP endpoint
  聚合为一条 authority record，不能显示为两个 server；
- 新 client 能读旧 TCP registration；旧 client 仍能通过迁移期 TCP alias
  连接新 server；
- prune 只删除有确定 dead owner 的 record；socket 暂不可达但 owner live
  仍保留诊断，行为与现有 discovery 不变量一致；
- `server-kill` / `kill-server` 按 authority identity 工作，不按显示行或
  endpoint 字符串误杀另一个 instance。

### 4.7 分阶段迁移

```text
I0 研讨与冻结
   endpoint union / InstanceIdentity / error / resolver / ACL / permissions
   registration schema / server-list compatibility / cross-version matrix
        |
I1 双栈能力（默认仍不切）
   server 可显式启动 named-pipe/UDS listener
   clients 可显式 --instance/--endpoint
   TCP 现有行为保持
        |
I2 双栈发现
   registration 同时表达 primary local + compatibility TCP
   server-list 聚合同一 authority
   new-client↔old-server / old-client↔new-server 通过
        |
I3 默认切换
   no-arg main 使用 native local transport
   --instance dev 使用独立 native local transport
   migration window 保留固定/显式 TCP compatibility endpoint
   普通启动不依赖随机 TCP port
        |
I4 收窄旧隐式 TCP（后续版本）
   以真实使用/版本遥测和发布说明决定何时移除隐式 alias
   显式 tcp endpoint 永久作为兼容/诊断/未来远程 transport
```

不得跳过 I0/I1 直接切默认。若跨版本、安全或 `server-list` 聚合门未通过，
v0.1.11 可以交付合同与显式 opt-in，但不得把半成品改成默认；计划和
capability 必须如实标明迁移阶段。

### 4.8 IPC 验收矩阵

| 维度 | 必须证明 |
|---|---|
| 默认 | Windows 无参数连接 named pipe；Unix 连接 UDS；不分配随机 TCP port |
| 实例 | main/dev 同时运行、不同 PID/epoch/workspace/endpoint，零串线 |
| 单例 | 同一 role 32 路并发启动只有一个 authority，其他复用或 typed conflict |
| selector | endpoint/address/env/instance/default 全优先级与冲突表驱动覆盖 |
| ACL/权限 | 不同 OS user 无法接入；宽松/错误 owner/path fail closed |
| 字符/长度 | Unicode/特殊/超长 username 不进入裸 path，派生无冲突且可诊断 |
| stale | dead socket 可恢复；live/unknown/symlink/regular file 绝不删除 |
| discovery | server-list 聚合同一 authority，旧/new registration 都可读 |
| 跨版本 | new client↔old server、old client↔new server、升级/回滚 |
| 生命周期 | GUI detach/restart、server stop/kill、Control Center/MCP/mux 语义不变 |
| TCP | 显式 loopback TCP 仍通过；non-loopback 未授权时不被误报 shipped |
| 清理 | 测试无 pipe/socket/lock/registration/process/workspace orphan |

## 五、标签树：双击原地编辑

### 5.1 行为合同

普通态：

```text
[disclosure][tree/status][name + note................][+][Close]
```

编辑态：

```text
[disclosure][tree/status][name editor / note editor][Save][Cancel]
```

要求：

- 普通态完全移除 `Edit` 图标/按钮，不留不可见占位宽度；
- 单击 name/note 正文只选择该 stable tab；
- 同一 stable tab、同一正文命中区内的主键双击进入原地编辑；
- 第一次点击可以先完成选择，第二次点击再进入编辑，不产生双重 mutation；
- disclosure 双击最多按既有 disclosure 语义处理，不进入编辑；
- 树连接线、indent 空白、状态灯、滚动条、`+`、`Close` 区双击均不进入编辑；
- 编辑开始后维持现有两个 native editor、Save/Cancel、draft cancellation、
  stable ID 和 Composer 独立不变量；
- Save 成功回普通态，Cancel 不 mutation 并回普通态；
- F2 对当前 Tabs 焦点/选中行进入编辑；
- 现有稳定 `ui-action edit-tab -t @ID` 保留为自动化等价入口；除非 catalog
  证明名称不稳定，否则不新增同义 action。

### 5.2 手势判定

共享产品层冻结 `TabRowGesture` 语义，平台 adapter 提供原生 click count、
双击时间/距离或规范化事件：

```text
primary click
├─ content rect + count 1 -> select
├─ content rect + count 2 + same stable @id -> begin edit
├─ disclosure rect -> toggle tree only
├─ action rect -> invoke that action only
├─ scrollbar rect -> scroll ownership
└─ connector/status/blank rect -> no edit
```

滚动、Tabs resize、collapse、tab close、窗口失焦或 stable target 改变会清除
未完成的双击候选，防止在移动后的另一行误触编辑。

### 5.3 可观察证据

- snapshot 普通态不再广告/占用 `edit` action bounds；
- snapshot 编辑态仍给出 editor、Save、Cancel 的稳定语义和几何；
- 同一宽度下 name/note 可用宽度比 v0.1.10 增加；
- 180/250/480 px Tabs、深层树、CJK name/note、滚动前后命中一致；
- native 双击、F2 和 `ui-action edit-tab` 三条入口产生相同编辑状态；
- Save、Cancel、Esc、tab switch、Tabs hide、target close、scroll、resize
  均无 orphan editor 或隐藏提交。

## 六、Control Center 进程与合同

### 6.1 进程职责

`agenterm-cc` 拥有：

- 自己的窗口、导航、渲染、焦点和临时 UI draft；
- 通过公共 IPC 获取的只读/typed Fleet projection；
- 本地 catalog/manifest 的读取与显示；
- 未来 WebView host 的进程内生命周期；
- 自己的日志、预算和崩溃边界。

它不拥有：

- PTY、tab tree、workspace、journal、receipt 或 server 生命周期；
- Plugin/App 的安装事务；
- libp2p/IPFS 节点的长期生命周期；
- Agent 权限、审批、凭据或策略；
- 主 GUI 的 Settings、Tabs 可见性或 Composer draft。

### 6.2 首版命令与发现

建议的最小公开面：

```text
agenterm-cc --help
agenterm-cc --version
agenterm-cc capabilities --json
agenterm-cc [--instance main|dev | --endpoint ENDPOINT | --address HOST:PORT]
            [--no-activate]
```

主 GUI 使用稳定 `open-control-center` action；CLI/Rhai 可通过同一 catalog
发现和触发。实现前必须决定是否增加独立
`control-center status|open|close` 命令，还是只保留 executable +
`ui-action`，避免平行命令面。

server 选择沿用 explicit address / environment / exactly-one healthy instance
的 fail-closed 规则，不随机连接多个 server，不为 Control Center 自动创建
第二个 workspace authority。

### 6.3 单实例与生命周期

默认建议：**每个用户会话、每个 server address 最多一个可交互
Control Center 窗口**。

- 再次点击按钮聚焦现有兼容窗口；
- `--no-activate` 只确保存在，不抢前台；
- 连接的 server 重启时显示 `Recovering/Restarted`，从新 epoch snapshot
  恢复，不伪造事件连续性；
- Control Center 关闭只关闭自身；
- GUI 关闭但 server 保留时，Control Center 可继续观察同一 server；
- server 关闭时进入明确 Offline，不展示 stale Fleet 为在线；
- 可执行文件缺失、不兼容或启动失败时，主 GUI 保持可用，并通过非阻塞状态
  与 typed result 报告，不弹阻塞自动化的 MessageBox。

### 6.4 v0.1.11 可交付内容

- 工具栏入口、稳定 action、响应式几何；
- 独立进程 bootstrap、server 选择、窗口生命周期和 capability discovery；
- Cockpit 最小只读摘要；
- Workflow/Pipeline、PluginHub、AppHub、InfoHub 导航与真实 availability/
  empty state；
- 本地 Rhai task catalog 和本地 manifest 可作为只读数据源，但不能据此
  宣称 durable workflow 或安装能力已经完成；
- snapshot/action、PNG、crash isolation、missing/incompatible、
  no-activate 和 orphan 证据。

本版不要求四个内容区达到产品完整态。

## 七、libp2p / IPFS 分阶段引入

### 7.1 分层路线

```text
阶段 N0：选型与边界
  dependency / license / feature / size / memory / thread / platform audit
      |
阶段 N1：独立原型（v0.1.11）
  agenterm-net lab process
  local two-peer identity + handshake + ping
  CID v1 + bounded temporary block put/get
  typed JSON + timeout/cancel/cleanup
      |
阶段 N2：稳定 sidecar 合同（后续）
  public capabilities + versioned protocol + durable identity choice
  discovery/pubsub/relay/cache/pin budgets
      |
阶段 N3：Script / Control Center 消费（后续）
  typed rhai::p2p / rhai::ipfs or provider calls
  InfoHub source/backend
      |
阶段 N4：server 服务（最后评审）
  只有稳定性、身份、安全、资源和升级证据成熟后
  才考虑 server 暴露摘要/路由；仍不链接重依赖进 PTY 热路径
```

### 7.2 v0.1.11 原型完成门

原型可以在源码和 CI 中存在，但在达到稳定 sidecar 门前不得作为 stable
release asset 广告。资格要求：

| 叶子 | 用户/工程问题 | 成功证据 | 安全失败 | 本版排除 |
|---|---|---|---|---|
| 依赖选型 | 避免盲目引入大依赖树 | license、feature、二进制/编译耗时报告 | 选型不达标则不进入 release | 不先抬高 2 MiB sidecar 预算 |
| 两节点握手 | 证明 libp2p 真实运行而非文档占位 | 两个独立进程交换 peer identity、ping receipt、clean exit | timeout/peer exit 分型并清理 | DHT、relay、NAT |
| CID | 证明 IPFS 内容寻址基础 | 相同 bytes 得到相同 CID，篡改校验失败 | invalid CID/size typed error | 完整 Kubo/full node |
| block put/get | 证明最小内容闭环 | invocation-owned store 写入、读取、hash 一致 | 中断不报告成功，temp 清理 | 长期 pin/GC/集群 |
| 隔离 | 不影响稳定产品 | kill/hang 原型后 GUI/server/PTY 继续 | fail closed，无 orphan | server 常驻集成 |

测试使用 loopback 和临时目录只是确定性 fixture，不是产品端点或路径权限
限制。未来 Script Runtime 调用仍遵守“Rhai 无权限层”的现有不变量。

### 7.3 进入稳定服务前的硬门

- 明确持久/临时 peer identity、密钥备份与轮换模型；
- threat model 覆盖恶意 peer、放大、资源耗尽、内容欺骗和 downgrade；
- bounded connection、stream、DHT、cache、pin、disk、memory 和 task budgets；
- Windows、macOS、Linux 原生互操作与升级/回滚；
- protocol、capability、receipt、event 和 diagnostics schema；
- sidecar crash/kill/update 不影响 GUI/server/PTY；
- 软件分发、SBOM、license 和 binary budget 独立审查；
- 用户明确启用，不因安装 AgenTerm 或打开 GUI 静默成为常驻节点。

## 八、系统 WebView host

### 8.1 目标

借鉴 Tauri 的核心思想：Rust 掌握产品状态和 native 生命周期，复杂展示使用
机器已有的系统 WebView；但 AgenTerm 不追求 Tauri API、插件、打包或前端
生态兼容。

```text
Control Center / future GUI product model
        |
        v
typed WebHost bridge（platform-neutral messages）
        |
        +-- Windows: WebView2 availability/host
        +-- macOS: WKWebView availability/host
        +-- Linux: WebKitGTK availability/host
```

### 8.2 v0.1.11 必须回答

1. 原生 API 直连、轻量 Rust wrapper、`wry`/相近方案各自的依赖、维护、
   binary size、跨架构和打包代价是什么？
2. Linux 的 WebKitGTK 动态依赖是否能满足 portable package 合同？缺失时
   如何 truthful Unsupported，而不影响主 GUI？
3. 本地静态资源用何种 version/hash identity 加载，如何禁止路径混淆和
   半更新？
4. Web → Rust / Rust → Web 消息如何 version、bound、cancel、诊断和测试？
5. WebView crash/reload 如何恢复 projection，而不复制 server authority？
6. `--no-activate`、DPI、locale、clipboard、accessibility、screenshot 和
   automation 如何通过现有 `platform/` capability 表达？

### 8.3 本轮交付和非目标

本轮交付：

- 一份进入 owning PRD 的 WebHost capability/消息/资源/生命周期合同；
- 三平台 availability 和包装矩阵；
- 至少一个独立、可删除的技术 spike 或 compile proof；
- 对 Control Center 原生首版与 WebView 首版的量化选择结论。

本轮非目标：

- 用 WebView 重写 terminal renderer、Tabs、Composer 或 Settings；
- 从网络直接加载 Control Center 代码；
- 在 Web 页面中保存 Fleet 权威；
- 为了采用 WebView 放宽第一窗口、包体、离线或平台失败真实性门；
- 强制 Control Center v0.1.11 采用 WebView；证据若不成熟，继续原生壳。

## 九、PRD 对齐与 owning tree

计划评审通过后，第一波必须先整理 canonical ownership，避免 Control Center
把其他模块的需求复制成第二份真相。

建议新增：

```text
PRD.md
└─ Control Center
   └─ prd/PRD_02_21_control_center.md
      ├─ shell / navigation / lifecycle
      ├─ Cockpit projection
      ├─ Workflow/Pipeline projection
      ├─ PluginHub projection
      ├─ AppHub projection
      ├─ InfoHub projection
      └─ native/WebHost renderer boundary

PRD.md
└─ Decentralized network
   └─ prd/PRD_02_22_decentralized_network.md
      ├─ agenterm-net process and protocol
      ├─ libp2p
      ├─ IPFS content addressing/storage
      ├─ resource/stability gates
      └─ future Script/InfoHub/server integration
```

跨模块唯一 ownership：

| 需求 | owning module | Control Center 只拥有 |
|---|---|---|
| 工具栏按钮、Tabs 双击编辑 | Human workspace | 无复制；只链接入口 |
| 独立 executable role/budget | Executable family | 自己的 UI 生命周期 |
| InstanceIdentity、endpoint、resolver、registration | Agent control plane | 只消费解析后的 selected authority |
| named pipe / UDS 平台安全适配 | Native platform + Agent control plane | 不自行拼 pipe/path |
| UI platform/WebView capability | Native platform | 产品级渲染选择 |
| Fleet snapshot/journal/wait | Control plane / Observable Fleet | 只读投射 |
| durable workflow/flow runtime | MCP orchestration | 导航、编辑与状态投射 |
| package/install/update/rollback | Optional components | PluginHub/AppHub 浏览和发起 |
| Rhai task/module/API | Rhai scripting | 目录消费，不复制 runtime |
| libp2p/IPFS node/protocol | 新 Decentralized network module | InfoHub 的 source/backend 投射 |

现有 v0.2.0 `Fleet Hub` 路线需要在波次零明确处理：

- 若接受 Control Center 独立进程方向，v0.2.0 计划改为内容成熟版本，
  不再并行维护 `Fleet Hub overlay` 与 `Control Center process` 两套壳；
- v0.1.11 交付基础壳、目录和原型；v0.2.0 聚焦 Workflow/Pipeline、
  PluginHub/AppHub、InfoHub 的第一批真实内容；
- `Fleet Hub` 可以作为历史设计名保留在决策记录中，不作为第二个产品按钮。

## 十、依赖图

```text
                 ┌─ 标签手势合同 ──► Win/Unix 行为 ──► UX black-box
                 │
PRD ownership ───┼─ Control Center process/lifecycle contract
                 │       ├─► toolbar action + responsive geometry
                 │       ├─► shared endpoint resolver + Cockpit projection
                 │       └─► four content catalogs / empty states
                 │
                 ├─ InstanceIdentity + Endpoint union + registration schema
                 │       ├─► Windows named-pipe adapter + ACL
                 │       ├─► Unix UDS adapter + permissions/stale recovery
                 │       ├─► shared client resolver / server-list aggregation
                 │       └─► dual-stack cross-version gate ──► default switch
                 │
                 ├─ agenterm-net boundary
                 │       ├─► dependency/license/size spike
                 │       ├─► two-peer libp2p proof
                 │       └─► CID + temporary block proof
                 │
                 └─ WebHost boundary
                         ├─► three-platform availability matrix
                         └─► spike result ──► native or WebView decision

全部集成
  -> catalog / snapshot / action / artifact alignment
  -> Windows native GUI journeys + Unix native evidence
  -> six-target CI
  -> stress-inclusive qualification
  -> byte-qualified package / release rehearsal
```

共享前置：

- v0.1.10 的 replaceable GUI/server、platform contract revision、MCP 只读
  snapshot/wait、Rhai task graph 和六平台发布链必须保持全绿；
- Control Center 不得依赖未完成的 workflow engine、softmgr、InfoHub
  connector 或 libp2p server 集成才能启动；
- 本地 IPC 默认值切换不得先于 endpoint schema、双栈 server-list、
  ACL/permission 和新旧 client/server 互通门；
- net/WebView spike 不进入普通 GUI startup dependency graph。

## 十一、并行设计与实施波次

### 波次 0：主代理串行冻结

- 同步远程 `main`，确认用户的 `En|Zh` 修改和其他未提交改动；
- 冻结 v0.1.11 outcome、Control Center 命名、独立进程选择、PRD ownership；
- 冻结标签双击 hit-region 和稳定 action；
- 冻结 InstanceIdentity、typed endpoint union、selector 优先级、registration
  schema、main/dev scope 和跨版本迁移矩阵；
- 冻结 agenterm-net 原型的“源码/CI 有证据、stable package 不广告”边界；
- 标出 `Cargo.toml`、`src/lib.rs`、toolbar geometry、locale、PRD root、
  alignment catalog 和 release manifest 等热点，统一串行 owner。

### 波次 1：产品设计并行

在不并发编辑同一 PRD/plan 文件的前提下，建议三个独立设计分支：

| 分支 | 交付 | 必须回答 |
|---|---|---|
| Workflow/Pipeline | 对象树、JTBD、状态机、恢复与证据草案 | task 与 durable workflow 的边界；run/node/receipt identity |
| PluginHub/AppHub | 两类 manifest、发现/安装边界、用户旅程 | 插件与应用为何不混为一谈；softmgr 唯一事务权威 |
| InfoHub + NET | source/item/route、libp2p/IPFS 接入阶段 | 信息如何变成可验证草稿；网络节点为何不属于 InfoHub |

主代理负责 Control Center 总信息架构、冲突收敛和 canonical PRD 合入。
WebHost 研究可以作为只读技术分支并行，不编辑 GUI 热点。

### 波次 2：可并行基础实现

```text
A. Tabs 双击编辑
   owner: shared row geometry/gesture + one平台主实现

B. Control Center executable shell
   owner: 新 cc 模块/entry + isolated unit fixtures

C. agenterm-net prototype
   owner: 独立 net 模块/entry/fixtures；不编辑 server/GUI

D. WebHost feasibility
   owner: read-only research + isolated spike；不接管 Control Center

E. IPC shared contract
   owner: endpoint/instance/resolver/registration pure modules + unit fixtures

F. Native local IPC adapters（合同冻结后并行）
   owner-win: named pipe + ACL + Windows native fixtures
   owner-unix: UDS + permissions + stale recovery + Unix native fixtures
```

不得并发运行竞争同一 target 的 Cargo build。需要修改 `Cargo.toml`、
artifact manifest 或公共 catalog 时，由主代理在各分支接口稳定后串行集成。

### 波次 3：IPC 双栈集成与 Control Center 内容基础

- server/client/CLI/Script/MCP/mux 统一消费 endpoint resolver；
- server-list 聚合 native primary 与 legacy TCP compatibility endpoint；
- main/dev 并行单例、旧↔新互通和默认切换门；
- toolbar action 与 launcher；
- Cockpit 同源 projection；
- Workflow/Pipeline、PluginHub、AppHub、InfoHub 本地目录与 empty states；
- Win/Linux/macOS adapter 与 capability facts；
- public snapshot/action/CLI discovery。

### 波次 4：并行黑盒

- Tabs gesture/geometry/native editing journey；
- Control Center lifecycle/no-activate/restart/crash/missing/incompatible journey；
- IPC selector/main-dev/singleton/ACL/stale/cross-version/server-list journey；
- catalog/source truth 与 stale/offline journey；
- libp2p two-peer、CID/block、timeout/crash/orphan journey；
- WebHost capability/packaging probe。

每条 journey 使用独立 IPC endpoint/instance、workspace、settings 和临时目录；
不得接触用户的默认 server。

### 波次 5：串行收口

- lint → format → catalog/PRD alignment → Clippy → unit；
- build + owning smoke；
- Windows/Linux/macOS × x86_64/ARM64 CI；
- full public-interface regression；
- stress-inclusive qualification；
- byte-identical packaging与 non-publishing rehearsal；
- 是否 tag / GitHub Release 仍服从发布授权边界。

## 十二、公共证据与测试树

```text
v0.1.11 evidence
├─ Workspace UX
│  ├─ En|Zh source/snapshot/PNG 一致
│  ├─ normal row 无 Edit action/bounds
│  ├─ single click select / body double-click edit
│  ├─ disclosure/tree/status/action double-click exclusion
│  ├─ F2 / ui-action equivalence
│  └─ save/cancel/focus/scroll/resize/CJK/deep-tree
├─ Control Center
│  ├─ toolbar center + compact geometry
│  ├─ explicit open / re-open focus / no-activate
│  ├─ same server PID/epoch/sequence/stable tab IDs
│  ├─ close/crash does not affect GUI/server/PTY
│  ├─ server restart/offline/stale/incompatible
│  └─ four navigation surfaces truthful availability
├─ Local IPC
│  ├─ native default + explicit TCP compatibility
│  ├─ main/dev isolation + same-role singleton race
│  ├─ selector precedence/conflict table
│  ├─ named-pipe ACL / UDS owner-mode-peer facts
│  ├─ stale socket safe recovery / unsafe target refusal
│  ├─ server-list registration aggregation
│  └─ new↔old client/server upgrade and rollback
├─ NET prototype
│  ├─ dependency/license/size report
│  ├─ two-peer handshake/ping
│  ├─ CID deterministic + tamper reject
│  ├─ bounded temporary block put/get
│  └─ timeout/cancel/kill/orphan cleanup
├─ WebHost
│  ├─ three-platform capability matrix
│  ├─ local resource and message bound tests
│  ├─ missing runtime typed Unsupported
│  └─ native-vs-WebView measured decision
└─ Release
   ├─ no first-window or GUI binary regression
   ├─ all GUI test launches no-activate
   ├─ six platform cells
   ├─ artifact/SBOM/hash/provenance alignment
   └─ exact candidate qualification receipt
```

### 12.1 每个本版 shipped leaf 的最小合同

| 叶子 | 不变量/权威边界 | 可观察成功 | 安全失败 | 黑盒 owner |
|---|---|---|---|---|
| 双击编辑 | stable tab + Composer 独立 | snapshot editing + PNG + persisted Save | 非正文双击不 mutation | workbench/remote UI journey |
| Control Center 按钮 | GUI 只启动客户端 | action receipt + process/window identity | missing/incompatible 不阻塞终端 | Control Center lifecycle journey |
| Cockpit | server 是唯一事实 | PID/epoch/sequence/tab 与 CLI 同源 | offline 清 stale projection | same-source journey |
| Instance resolver | 一个 selector 选一个 authority | table-driven endpoint/instance identity | 歧义/冲突不猜测 | CLI/IPC resolver journey |
| named pipe / UDS | 当前 OS user scope | native connect + peer/owner facts | ACL/permission/type 不符拒绝 | platform IPC journey |
| main/dev singletons | 每 role 一个 authority | 并发启动 + 双实例隔离 | 冲突不转随机端口 | lifecycle/discovery journey |
| registration migration | 一条 record 对应一 authority | server-list 新旧同源聚合 | owner unknown 不 prune | cross-version journey |
| Hub 目录 | availability 不伪装 shipped | catalog + empty/degraded state | unknown manifest 不执行 | catalog fixture |
| net handshake | net process 独立 | peer IDs + ping typed receipt | timeout/kill 清理 | net prototype journey |
| CID/block | content hash 是身份 | put/get bytes/hash 相同 | corrupt/oversize typed error | net storage fixture |
| WebHost 选择 | 不影响主 GUI startup | capability matrix + measured report | 缺运行时 Unsupported | platform probe |

### 12.2 必须保留的回归门

- process exit 不删除 tab；
- GUI close 的 keep/stop/cancel 三选与 server 保留语义；
- GUI 可替换且 server/PTY identity 不变；
- 旧显式 `--address HOST:PORT`、多实例 discovery、server-list、kill-server 和
  upgrade/rollback 行为在 native local transport 引入后不回退；
- Tabs tree collapse、promote-children、scrollbar、inline editor 和 CJK；
- terminal selection、clipboard、wheel、resize、IME、DPI 和 no-activate；
- Script Runtime 无权限层；
- MCP 首发仍只读且只有一个 wait tool，Control Center 不借它新增隐式 mutation；
- 第一窗口、binary budget、locked artifact、orphan 和 qualification receipt。

## 十三、风险与收敛策略

| 风险 | 早期信号 | 收敛 |
|---|---|---|
| v0.1.11 变成四个大产品同时开工 | 开始实现完整 flow/market/feed | 只交付壳、catalog、empty state 和一条 Cockpit 纵切 |
| Fleet Hub 与 Control Center 双轨 | 两个按钮/overlay/process 都存在 | 波次零做唯一命名与壳决策，旧称仅留历史 |
| 独立进程复制 server 状态 | cc 内出现第二份 workspace/journal | 只用 public snapshot/delta/wait，断线清 projection |
| Control Center 反向拖大主 GUI | agenterm.exe 链接 WebView/net/package | launcher/action only；重依赖只在 sidecar |
| AppHub 与 PluginHub 含混 | 同一 manifest 同时表示工具和应用 | 分开用户 job/schema，共用 softmgr 事务层 |
| InfoHub 变媒体/交易应用 | feed item 自动执行动作 | 首版只 route 到 notification/Composer draft |
| libp2p 依赖爆炸 | sidecar 远超预算、编译显著变慢 | feature audit；原型不进 stable package；不随意抬预算 |
| “先实验”永久没有产品门 | 只有 demo，无 typed protocol/cleanup | N1 明确 receipt、budget、failure、orphan 完成门 |
| WebView 平台不对称 | Linux 缺系统库却报告可用 | capability matrix + typed Unsupported + native fallback |
| 双击误触 disclosure/其他行 | scroll/resize 后进入错误 tab | stable ID + content rect + candidate cancellation |
| 移除 Edit 伤害可发现性/键盘 | 用户不知道如何编辑 | tooltip/帮助、F2、context action、ui-action |
| 把 username 当安全 namespace | 特殊字符/同名用户/path traversal | OS SID/UID scope + versioned short derivation，username 只显示 |
| UDS stale recovery 误删用户文件 | bind 前无 type/owner/live 证据 | no-follow + socket/type/owner/lease/connect 全条件 |
| main/dev 串线 | dev GUI 出现在 main workspace | role 进入 identity/registration/hello，禁止 dev→main fallback |
| 双栈被显示成两个 server | server-list 同一 PID 两行 | authority identity 聚合 primary + compatibility endpoints |
| flag-day 切 IPC 破坏旧版本 | old client 看不见新 server | I0–I3 分期，迁移期 dual-stack + cross-version fixture |
| 并发启动绕过单例 | 两个 server 转去不同随机端口 | 原子 native bind/lock，失败者复用或 typed conflict |
| named pipe/UDS 权限过宽 | 另一用户可读写 Fleet | explicit DACL / 0700+0600 / peer credential evidence |
| TCP 兼容被误解为安全远程 | non-loopback 能无认证控制 | remote capability 单独 fail closed，显式语法不等于授权 |
| 多代理编辑热点冲突 | PRD/Cargo/toolbar 同时修改 | 设计分支只交报告；热点由主代理串行合入 |
| 测试窗口抢前台 | Control Center/GUI 闪到用户工作前 | 自动化继承 no-activate；专属 IPC/workspace |

## 十四、明确延后与未来计划

### Control Center 内容

- Workflow/Pipeline 的完整 DAG 编辑、版本、运行、暂停、重试、补偿、
  durable recovery 和跨机调度；
- PluginHub/AppHub 的公共 registry、评分、支付、publisher portal、
  自动依赖求解、在线安装和自更新；
- InfoHub 的大规模 feed store、全文检索、推荐系统、推送和自动 action；
- Control Center 多用户协同或云端 SaaS。

### 去中心化网络

- 默认常驻 libp2p/IPFS 节点；
- DHT、mDNS/公网 discovery、pubsub、relay、NAT traversal 和跨公网 Fleet；
- 完整 IPFS DAG、pin service、gateway、CAR、集群和长期存储；
- CID 签名包市场、去中心化应用分发和可验证计算交易；
- 将 heavy network dependency 链接进 GUI/server。

### WebView

- 全面 Web 技术重写 AgenTerm；
- remote code/content 作为 UI；
- 把 server authority 或凭据放入 JavaScript；
- 为了统一界面隐藏平台 runtime 缺失；
- 未经独立可访问性、输入、IME、DPI、clipboard、screenshot 和 crash 门
  就宣布替代原生 UI。

### IPC 与远程连接

- 在跨版本窗口和发布证据不足时移除 legacy TCP alias；
- 把 named pipe/UDS 暴露为跨机器 transport；
- 没有认证、加密、peer identity、revocation 和 threat model 的
  non-loopback TCP；
- 任意自定义实例字符串直接映射 native path/name；
- 同一用户无限实例、跨用户共享 authority 和系统级 daemon；
- 仅为方便测试恢复普通启动的随机 TCP port 依赖。

### 其他

- Agent harness、权限、审批、credential/endpoint policy；
- 完整 MCP mutation、subscriptions、federation 和 autonomous scheduling；
- 把 Control Center 的新能力反向塞回主工具栏或底部状态条。

## 十五、建议默认决策与未决项

### 建议直接接受

1. 对外工作名使用 **Control Center**，进程名使用 `agenterm-cc`。
2. Control Center 采用独立进程、按需启动、同 server typed client 架构；
   不再并行实现 Fleet Hub overlay。
3. v0.1.11 只交付壳、Cockpit 只读纵切、四个内容目录/空状态；
   v0.2.0 再承担内容成熟。
4. `PluginHub` 管能力组件，`AppHub` 管组合应用；二者共用未来 softmgr，
   不建两个安装器。
5. `agenterm-net` 先做不进 stable package 的独立原型；不连接 server。
6. WebView 先做合同、三平台能力矩阵和 spike；不强制 Control Center 本轮采用。
7. 普通标签行移除 Edit，正文双击、F2、现有 `ui-action edit-tab`
   成为三条等价入口。
8. 语言按钮保持用户已采用的 `En|Zh`。
9. 逻辑实例首版固定为 `main|dev`，显示为
   `{username}_main` / `{username}_dev`；底层 endpoint 不使用原始
   username，也不把显示名当成安全身份。
10. Windows native default 为 named pipe，Linux/macOS 为 UDS；普通实例
    不再依赖随机 TCP port。
11. `--instance main|dev` 走统一 resolver；显式 endpoint/address 优先，
    多个显式 selector 冲突时 fail closed。
12. 显式 loopback TCP 在迁移期保留为兼容/诊断 transport；何时移除旧
    默认行为由兼容证据另行决定。non-loopback 稳定能力继续等待独立的
    认证、加密和威胁模型安全门。
13. 默认切换必须经过双栈 registration、server-list 聚合和新旧版本互通，
    不做 flag-day migration。

### 实施波次零仍需裁决

- 一个用户配置域内复用单个 Control Center 进程并切换多个 server
  context 时，窗口/导航偏好与各 server 临时视图缓存的持久化边界；
- Control Center v0.1.11 使用现有原生渲染壳，还是 WebHost spike 达标后直接
  使用系统 WebView；
- Plugin/App manifest 是否扩展 optional-component manifest，还是使用
  引用它的上层 composition manifest；
- InfoHub 首个真实 source 是本地文件/Rhai task、HTTP，还是只保留 empty state；
- net 原型是否作为默认 workspace member 编译，还是显式 feature/独立 CI lane；
- 原型二进制是否需要临时放入 ignored `dist/` 供评审，但不进入 release manifest；
- 双击时间/距离完全使用 OS 原生阈值，还是共享 recognizer 只消费平台阈值。
- Windows pipe namespace/DACL 的最终字面合同与 LocalSystem 是否保留访问；
- Linux `$XDG_RUNTIME_DIR` 缺失和 macOS runtime base 的准确 fallback；
- legacy TCP alias 保留几个公开版本，以及 main 是否固定保留 48815 兼容；
- registration 的旧 ADDRESS 列如何在人类表格中平滑迁移为 ENDPOINT；
- 显式 custom instance 是否本版完全拒绝，还是只作为 `explicit endpoint`
  存在而不进入 `{username}_{role}` 逻辑命名。

## 十六、完成定义

v0.1.11 public-ready 必须同时满足：

- 用户的 `En|Zh` 改名在单一 locale source、Win/Unix 显示、snapshot 和 PNG
  中一致；
- 标签普通态没有 Edit action 或占位，双击正文/F2/typed action 编辑通过；
- Windows 默认 main/dev 通过 named pipe，Linux/macOS 默认通过 UDS；普通
  启动不依赖随机 TCP port，显式 TCP 兼容仍通过；
- `{username}_main/dev` 只作为安全显示，底层 SID/UID scope、namespace、
  ACL/permissions、长度/字符和 peer/owner 校验通过；
- `--instance`、endpoint/address/environment/default 选择与冲突矩阵通过，
  main/dev 同时运行不串线，同 role 并发启动只有一个 authority；
- stale socket 安全恢复、unsafe path 拒绝、server-list 新旧 registration
  聚合、new↔old client/server 和升级/回滚证据通过；
- Control Center 工具栏入口、独立进程、server 同源、重启/offline、crash/
  missing/incompatible、no-activate 和 orphan 证据通过；
- Cockpit 真实可用，Workflow/Pipeline、PluginHub、AppHub、InfoHub 的
  availability/empty state 不夸大；
- libp2p 两节点和 CID/block 独立原型在 CI 有确定性证据，但未被包装成稳定
  server 或 stable market capability；
- WebView host 有三平台可行性结论和明确采用/延后决策；
- owning PRD、roadmap、executable/artifact/capability catalogs 和公开文档
  对齐；
- lint、Quick、owning smoke、full public-interface、六平台 CI、
  stress-inclusive qualification 和 byte-qualified rehearsal 全绿；
- 任何新二进制都具有独立 size/SBOM/hash/provenance 和失败隔离事实；
- tag 与 GitHub Release 仍只在用户明确批准后创建。
