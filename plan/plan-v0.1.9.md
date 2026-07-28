# AgenTerm v0.1.9 公开计划

状态：讨论稿  
工作主题：**通用 Script Runtime 成型**  
版本定位：把已经可靠但能力有限的 `agenterm-script.exe` 从 pure/observe
脚本 sidecar，推进为真正适合日常本地自动化、Fleet 编程和未来 Agent
工具层的通用 Rhai 运行时。

本文是公开的版本执行计划与决策记录，不是实现依赖。接受的产品能力、
边界和验收条件必须同步进入对应 PRD 模块；版本结束后保留本文作为交付
历史。

## 〇、核心判断

v0.1.9 先完善 `agenterm-script.exe`，再在 v0.1.10 交付只读 MCP。

原因不是 MCP 不重要，而是 Script Runtime 是更底层、更高复用的能力：

```text
agenterm-script.exe
  本地自动化标准库
  模块与命名任务
  task / stream / cancellation
  typed Fleet API
  机器可读工具 schema
       |
       ├─ 仓库自托管辅助脚本
       ├─ 动态状态与用户命令
       ├─ agenterm-mcp.exe 的工具适配
       ├─ agenterm-agent.exe 的执行工具层
       ├─ agenterm-{script,bash,...}.exe 可选组件族
       ├─ 未来 agenterm-softmgr.exe 与软件分发市场
       ├─ 未来 agenterm-desktop.exe companion 应用
       └─ 未来 brain / flow 的可组合节点
```

如果先做 MCP，只能得到一个稳定但较薄的只读适配器；如果先把 Script
Runtime 做实，MCP 与 Agent 层可以复用已经经过文件、进程、网络、任务、
取消、错误、审计和 Fleet post-state 验证的工具合同。

产品原则不变：

> 界面简单实用，软件稳定可靠，编程接口丰富，并为扩展保留足够空间。

这一轮主要增强 CLI/runtime，不给普通 GUI 堆新面板。运行时对标
Node.js/Bun 的是“本地自动化用途和组合能力”，不是 JavaScript 语法、
Node API、npm 或 Bun 二进制兼容。

## 一、版本目录树

```text
v0.1.9  通用 Script Runtime 成型
│
├─ 最高优先级：local 通用执行入口
│  ├─ run / eval 默认 local profile
│  ├─ check 离线验证脚本、模块、任务和 API
│  ├─ local 具有普通本地程序应有的文件/进程/网络能力
│  ├─ pure / observe 保持兼容并继续是显式专用 profile
│  └─ typed result、exit class、timeout、cancel、crash 和 recovery
│
├─ 最高优先级：可用的本地标准库
│  ├─ fs / path
│  ├─ env
│  ├─ process
│  ├─ time / task
│  ├─ json / text / bytes
│  ├─ http
│  └─ temp / cleanup / atomic replacement
│
├─ 最高优先级：task、stream 与取消模型
│  ├─ 异步 API 返回 typed task handle
│  ├─ wait / wait_all / cancel / bounded stream
│  ├─ stdout/stderr/HTTP body 不要求一次性装入内存
│  ├─ backpressure、timeout、parent exit 传播
│  └─ sidecar 在无 foreground task 后自然退出
│
├─ 最高优先级：模块、任务清单与命名命令
│  ├─ 本地相对模块和明确 project root
│  ├─ versioned agenterm.tasks.json
│  ├─ task list / show / run
│  ├─ 无效任务保持可发现并给出 degraded reason
│  └─ CLI 与未来 GUI command palette 共用一个 catalog
│
├─ 第一优先级：面向组件生态但不提前做市场
│  ├─ runtime/module/task 具有稳定 identity、version 与 entry point
│  ├─ requirements、capability facts 与 provenance hooks 可机器读取
│  ├─ 未来 agenterm-{script,bash,...}.exe 共用发现语言
│  ├─ 不远程解析、不下载安装、不决定签名信任
│  └─ task manifest 与未来 package manifest 保持不同职责
│
├─ 最高优先级：完整 typed Fleet API
│  ├─ 从 operation catalog 系统映射，不手写第二套 API
│  ├─ observe / control / destructive facts 可发现
│  ├─ local mutation 带 request identity、receipt 和 post-state
│  ├─ 不可安全映射的操作显式 degraded
│  └─ 输出未来 MCP / agenterm-agent.exe 可直接消费的工具 schema
│
├─ 第一优先级：公共自反馈与日用 dogfood
│  ├─ Unicode/长路径文件旅程
│  ├─ argv/cwd/env/stdin/stdout/stderr 子进程旅程
│  ├─ 本机 loopback HTTP 旅程
│  ├─ task/module/manifest/stream/cancel 旅程
│  ├─ Fleet mutation + receipt + event + post-state 旅程
│  └─ timeout/crash/parent exit 后无 worker/child/temp orphan
│
├─ 第一优先级：自托管第一步
│  ├─ 选择一个低风险只读 PowerShell helper
│  ├─ PowerShell 与 Rhai 双跑并比较结构化结果
│  ├─ Rhai 失败时保留 PowerShell last-known-good
│  └─ 不触碰 build/check/package/release 关键路径
│
├─ 第一优先级：运行时架构整理
│  ├─ 从 bin 文件提取 runtime/stdlib/task/module/catalog
│  ├─ 每个标准库域独立 Rust 模块和测试
│  ├─ GUI 启动不构造 Rhai engine、不扫描任务目录
│  ├─ 一个 invocation-owned sidecar，不做系统 daemon
│  └─ 保持 GUI、CLI、script binary 大小和启动预算
│
└─ 明确延后与未来计划
   ├─ v0.1.10 agenterm-mcp.exe 只读 MCP bridge
   ├─ agenterm-agent.exe、审批 UI、agent 权限和自然语言策略
   ├─ npm 兼容、远程任意模块和第三方包生命周期
   ├─ agenterm-softmgr.exe、签名包/应用市场与联网软件分发
   ├─ agenterm-desktop.exe companion 与更远期可选 Shell Replacement
   ├─ persistent script daemon、跨 invocation mutable state
   ├─ REPL、watch mode、durable scheduler 和开机自启任务
   ├─ low-level sockets、监听公网端口和通用网络 sidecar
   ├─ 把 agenterm-bash.exe 设置为默认 shell
   ├─ 用 Rhai 替换资格/打包/发布关键 PowerShell 脚本
   ├─ agenterm-net.exe、libp2p/IPFS 与去中心化应用
   ├─ agenterm-mux.exe 原生 mux server、完整 pane 与多后端
   ├─ agenterm.exe 与 agenterm-cli.exe 单文件合并
   └─ 安装器、自动更新和未单独批准的公开发布
```

## 二、什么叫“完善”

“完善”不能理解为一次实现整个 Node/Bun 生态。v0.1.9 的完成标准是：

> 用户可以只依赖 `agenterm-script.exe` 和一个本地项目目录，编写、检查、
> 发现并运行可组合任务；任务能可靠处理文件、环境、子进程、HTTP、时间、
> JSON/文本/字节和 AgenTerm Fleet，并在成功、失败、取消、超时和崩溃后
> 给出可验证结果且不留下残余资源。

本版不追求：

- JavaScript/TypeScript；
- Node/Bun API compatibility；
- npm install；
- 任意远程 import；
- 浏览器 DOM；
- 系统级常驻 runtime；
- 用“安全沙箱”名义阉割正常 local 自动化能力；
- 把 agent 权限、审批和自然语言策略塞入 script runtime。

## 三、北极星演示

一个全新的示例项目必须完成：

```text
agenterm-script.exe task list
  -> 发现 task "daily-check"
     -> agenterm-script.exe task show daily-check --json
        -> 显示入口、参数、profile、cwd、API、limits
           -> agenterm-script.exe task run daily-check -- target
              -> 读取 Unicode 配置文件
              -> 创建 owned temp directory
              -> 并行启动两个 argv-safe child
              -> 请求本机 loopback HTTP fixture
              -> 汇总 JSON
              -> 调用 typed Fleet API 修改测试 tab note
              -> 等待 receipt/event 并验证 post-state
              -> 原子写出结果文件
              -> 清理 child、stream、temp 和 task
```

演示同时证明：

- `check` 在不执行脚本时发现未知 module/API/signature；
- argv 不经过隐式 shell 拼接；
- HTTP、stdout/stderr 和文件错误不泄露 credential/body；
- Fleet mutation 不以脚本返回值作为唯一成功证据；
- Ctrl+C、timeout、server restart 或强杀 worker 后 GUI/PTY 健康；
- 下一次 task invocation 正常；
- 没有 child、worker、pipe、temp、registration 或 secret orphan。

## 四、Profile 模型

### `local`

普通 `run`、`eval` 和 named task 默认使用 `local`：

- 权限相当于用户主动启动的普通本地程序；
- 可使用本版完整标准库；
- 不要求每个文件、进程或 HTTP 调用再传一层 capability flag；
- 仍受数据完整性、typed error、资源上限、取消、审计隐私和产品不变量
  约束；
- Fleet mutation 必须通过公共 typed operation/receipt，而不是直接改
  GUI/PTY 私有状态。

### `pure`

继续适合确定性计算：

- 无 ambient fs/env/process/network/clock/Fleet；
- JSON-compatible 输入输出；
- 固定预算、稳定失败；
- 现有行为不回退。

### `observe`

继续适合只读 Fleet 工具：

- typed workspace/tab/snapshot/capture/event read/wait；
- 无文件、进程、网络和 Fleet mutation；
- restart、gap、timeout、truncation 分型；
- 现有行为不回退。

`local|pure|observe` 是 runtime execution profile，不是未来 Agent 的角色、
审批或权限系统。未来 agent 层可以过滤工具 schema，但不能迫使 runtime
重新实现一套标准库。

## 五、CLI 合同

主入口：

```text
agenterm-script.exe run [OPTIONS] FILE.rhai|- [--] [ARGS...]
agenterm-script.exe eval [OPTIONS] EXPRESSION [--] [ARGS...]
agenterm-script.exe check [OPTIONS] FILE.rhai|-
agenterm-script.exe api --json
agenterm-script.exe task list [--manifest PATH] [--json]
agenterm-script.exe task show TASK [--manifest PATH] [--json]
agenterm-script.exe task run TASK [--manifest PATH] [--] [ARGS...]
```

如保留 `agenterm-cli.exe script ...`，它只能是同一 catalog/runtime 的薄
入口，不能形成第二套选项、默认值或错误合同。

共同 options：

```text
--profile local|pure|observe
--cwd PATH
--timeout-ms N
--max-output-bytes N
--max-tasks N
--max-stream-bytes N
--json
```

所有默认值和 hard ceiling 由 `api --json` 公开。CLI override 只能收紧或
在允许范围内调整，不能超过编译时 hard ceiling。

退出分类：

| class | 含义 |
|---|---|
| success | 脚本与所有 required foreground task 完成 |
| script | Rhai parse/runtime/user error |
| configuration | 参数、manifest、profile、API 不可用 |
| child | child 正常启动但返回非零且调用要求传播 |
| limit | 时间、内存近似预算、输出、task、stream 上限 |
| cancelled | Ctrl+C、parent、显式 task cancellation |
| host | worker protocol、spawn、crash、internal invariant |
| fleet | server unavailable/restart/gap/receipt/post-state failure |

文本 message 可改善，但自动化只依赖稳定 class/code/JSON。

## 六、标准库第一版

### `fs`

```text
read_text / read_bytes
write_text / write_bytes
atomic_write
list
metadata
create_dir / create_dir_all
copy / move
remove_file / remove_dir
```

要求：

- 显式 UTF-8/bytes，不猜编码；
- 单次与累计 bytes 有界；
- Windows long path、Unicode、只读、占用、拒绝访问分型；
- atomic replace 不把失败报告为成功；
- remove 只作用于明确路径，不接受空/root/未解析 broad target；
- owned temp helper 记录所有权并在取消/崩溃路径清理。

### `path`

- join、parent、file name、extension、relative、normalize；
- project root 与 cwd 分开；
- Windows drive、UNC、separator、Unicode、long path 语义明确；
- canonicalization/reparse point 不静默改变报告的目标；
- 返回 typed path value 或规范 string，不能依赖显示文本解析。

### `env`

- get、has、set、remove、names、construct child env；
- Windows name 大小写语义正确；
- 读取和修改仅影响 worker/child，不修改 parent AgenTerm 进程；
- audit/diagnostics 记录 name/count，不记录 value；
- secret value 不进入 error、schema 或 retained bundle。

### `process`

```text
run(program, argv, options)
spawn(program, argv, options) -> TaskHandle
```

options：

- cwd；
- explicit env overlay/replace；
- stdin text/bytes/stream；
- separate stdout/stderr；
- timeout；
- output/stream limits；
- expected exit policy；
- Windows creation flags 的有限 typed 选择。

禁止用一个 command string 隐式调用 shell。需要 shell 时用户必须显式启动
`cmd.exe`、PowerShell 或未来 `agenterm-bash.exe` 并提供 argv。

### `http`

```text
request(method, url, options) -> TaskHandle|Response
```

首版包含：

- HTTP(S)；
- headers；
- text/bytes body；
- status、response headers；
- bounded body stream；
- timeout/cancel；
- proxy/TLS/connection error 的无 secret 诊断。

不包含：

- raw socket；
- listener/server；
- WebSocket；
- 任意 scheme；
- 自动远程 module 下载。

资格测试只使用本机 loopback fixture，不依赖公网。

### `time` 与 `task`

- wall-clock 与 monotonic deadline 分开；
- sleep/timer 可取消；
- task handle 具有 invocation-local stable ID；
- wait、wait_all、race 的顺序和失败传播明确；
- task cancel 有终态且迟到完成不能覆盖 cancelled；
- sidecar 无 reachable foreground task 后自然退出。

### `json`、`text`、`bytes`

- bounded parse/stringify；
- UTF-8-safe slice/length；
- explicit text/bytes conversion；
- hex/base64 是否进入首版由实际 HTTP/process 旅程决定；
- 深层 JSON、巨大 collection、无效 UTF-8 返回 typed limit/data error。

## 七、Task 与 Stream 模型

Rhai 不需要伪装成 JavaScript Promise。首版使用显式 typed handle：

```text
TaskHandle
  id
  kind
  state = pending|running|completed|failed|cancelled

StreamHandle
  id
  kind = bytes|text|json-lines
  state
  buffered_bytes
  truncated
```

候选 API：

```text
task.wait(handle, timeout_ms?)
task.wait_all(handles, timeout_ms?)
task.cancel(handle)
stream.read(handle, max_bytes)
stream.close(handle)
```

最终命名在 catalog 冻结时确定，但必须满足：

- handle 不能跨 invocation 使用；
- duplicate/unknown/completed handle 分型；
- wait 不阻塞 cancellation frame；
- queue item/bytes/concurrency 有 hard ceiling；
- stdout/stderr/HTTP body 有 backpressure；
- truncation 不能伪装完整；
- worker crash/parent exit 由 Job Object 与 supervisor 清理 child；
- task error 包含 stable code，不把任意 body/argv/env 写入 message。

## 八、模块系统

首版只支持本地模块：

- entry script 或 manifest 所在目录是明确 project root；
- relative module resolution 不能逃出 root，除非用户显式声明额外 root；
- module identity 使用规范路径和 runtime/schema version；
- cycle、missing、duplicate identity、parse failure 分型；
- module source 不进入 audit；
- 不扫描用户 home、PATH 或网络；
- 不实现 npm-style package resolution。

缓存：

- invocation 内可缓存 parsed/compiled module；
- 可选 bounded AST cache 必须以 source fingerprint、runtime/API version、
  profile 为 key；
- v0.1.9 不要求跨 invocation mutable cache；
- cache miss 与 cache corruption 不能改变脚本结果。

## 九、命名任务

首版清单固定为：

```text
agenterm.tasks.json
```

选择 JSON 的理由：

- 仓库已有稳定 `serde_json`；
- 不增加 TOML/YAML parser 与发布依赖；
- schema、error location、machine editing 和工具消费直接；
- 与 `api --json`、receipt、diagnostic manifest 语言一致。

候选 schema：

```json
{
  "schema_version": 1,
  "tasks": {
    "daily-check": {
      "description": "Run the local daily check",
      "script": "tasks/daily-check.rhai",
      "profile": "local",
      "cwd": ".",
      "args": [],
      "env_names": [],
      "timeout_ms": 30000
    }
  }
}
```

约束：

- task key 是 stable ID，description 只是显示；
- list 按 stable ID 排序；
- invalid task 不消失，显示 `available:false` 和 degraded reason；
- duplicate、unknown field、bad version、root escape、missing script 分型；
- manifest 不保存 secret env values；
- project manifest 与用户级 named command 暂不合并搜索路径，除非先定义
  优先级和冲突语义；
- GUI command palette 以后只消费这个 catalog，不创建第二注册表。

## 十、Typed Fleet API

Script Fleet API 必须从公共 operation catalog 系统派生：

```text
operation spec
  stable ID
  observe|control|destructive
  params/result/error schema
  stable target rules
  request identity/deadline
  receipt/wait contract
  event/post-state
  availability/degraded reason
       |
       ├─ agenterm-cli
       ├─ agenterm-script local/observe
       ├─ v0.1.10 agenterm-mcp
       └─ future agenterm-agent
```

首版要求：

- 每一个 public operation 都出现在 script schema；
- `observe` profile 只暴露 observation subset；
- `local` 可以调用明确 control/destructive operation；
- destructive 不被重命名成模糊 helper；
- close/kill/shutdown 继续遵守原生确认或明确非交互合同；
- mutation 自动生成 request ID 或接受用户提供的稳定 ID；
- 返回 receipt、resolved target、event position 和 post-state result；
- retry 不重复 side effect；
- 无法安全映射的 operation 显示 degraded reason，而不是静默遗漏；
- runtime 不读取 `AppState`、HWND、renderer 或 PTY 私有字段。

这里的 classification 是工具事实，不是 Agent authorization。未来
`agenterm-agent.exe` 可以基于 schema 过滤/审批，但 script local 仍是用户
主动启动的正常程序。

## 十一、`api --json` 工具 schema

catalog 是实现、check、文档、MCP/Agent 消费者的同一事实源。

每个 API entry 至少包含：

- stable ID；
- module/function/signature；
- profile availability；
- input/result/error schema；
- fs/process/network/Fleet access facts；
- mutation/destructive facts；
- sync/task/stream；
- cancellation/timeout；
- defaults、soft limit、hard ceiling；
- runtime/API version；
- degraded/unavailable reason；
- secret-bearing input/output facts。

`check` 使用同一 catalog 验证：

- imports；
- task entry；
- API name；
- profile；
- arity/signature；
- unavailable/degraded call；
- manifest/runtime version；
- 能静态确认的 hard limit；
- 不执行用户代码，不连接 GUI，不访问网络。

### 面向未来组件/软件分发的最小接口

v0.1.9 不实现包管理器，却要避免把未来堵死。runtime、module 和 named
task 的机器可读描述需要包含稳定 identity、schema/runtime version、entry
point、required API/capabilities，以及可选 origin/provenance hooks。这样
未来 `agenterm-softmgr.exe` 可以在不执行脚本的情况下完成 inventory 和
compatibility planning，`agenterm-mcp.exe`/`agenterm-agent.exe` 也能解释
“已安装、缺失、不兼容或不可用”。

边界必须清楚：

- `agenterm.tasks.json` 描述如何运行本地任务，不承担下载、签名或安装；
- future package manifest 描述分发单元、文件、hash、签名、依赖和入口；
- `agenterm-script.exe` 可以提供 hash、文件、HTTP、进程等通用自动化能力，
  但不能自行成为信任根或绕过 `agenterm-softmgr.exe` 的事务边界；
- v0.1.9 不扫描全机组件、不访问公共 registry、不安装任何 package；
- 这层合同服务于整个 `agenterm-{script,bash,mcp,agent,desktop,...}.exe`
  家族，不只服务 Rhai module。

## 十二、运行时架构

避免继续把所有逻辑塞进 `src/bin/agenterm-script.rs`：

```text
src/script_runtime.rs
  invocation lifecycle
  profile
  engine assembly
  typed result

src/script_catalog.rs
  API/schema/default/limit facts

src/script_task.rs
  task handle
  scheduler
  cancellation

src/script_stream.rs
  bounded stream and backpressure

src/script_module.rs
  project root and local resolver

src/script_manifest.rs
  agenterm.tasks.json
  task discovery

src/script_std/
  fs.rs
  path.rs
  env.rs
  process.rs
  http.rs
  time.rs
  json.rs
  text.rs
  bytes.rs

src/script_fleet.rs
  operation-catalog adapter
  receipt/post-state

src/bin/agenterm-script.rs
  argument parsing and worker entry
```

不变量：

- normal GUI startup 不构造 Rhai engine、不扫描脚本/manifest；
- 一个 invocation 拥有一个 fresh sidecar；
- sidecar 可因自己的 foreground tasks 延长生命，但不是系统 daemon；
- supervisor 不依赖 Rhai 类型；
- worker frame protocol 与 script stdout 分离；
- filesystem/process/http 模块不能把 GUI 或 Fleet authority 拉进 worker。

## 十三、公共黑盒资格

### 文件与路径

- Unicode、空格、长路径、不同 drive 语义；
- text/bytes；
- metadata/list/copy/move/remove；
- atomic replacement；
- occupied/read-only/access denied；
- root escape/reparse point；
- cancel/crash 后 owned temp cleanup。

### Environment

- case-insensitive Windows name；
- overlay/replace/remove；
- child inheritance；
- parent GUI/server env 不变；
- secret sentinel 不进入 stdout/stderr/audit/bundle。

### Process

- executable + argv 边界；
- cwd、Unicode、spaces；
- stdin；
- separate stdout/stderr；
- nonzero exit；
- output limit；
- timeout/cancel/parent exit；
- process tree Job Object cleanup；
- 下一次 invocation recovery。

### HTTP

- loopback GET/POST；
- headers/body/status；
- text/bytes；
- bounded streaming；
- timeout/cancel；
- malformed response/disconnect；
- proxy/TLS-safe errors；
- 无公网依赖、无 listener 残留。

### Task/stream

- concurrent progress；
- wait/wait_all/cancel；
- race 与迟到完成；
- backpressure；
- truncated/incomplete truth；
- queue/concurrency ceiling；
- natural worker exit。

### Module/manifest

- relative modules、cycles、duplicate/missing；
- manifest version；
- invalid task remains visible；
- stable ordering；
- args/cwd/profile；
- root escape；
- list/show 不执行脚本。

### Fleet

- catalog 每项 mapped 或 degraded；
- observe 与 local profile 边界；
- mutation receipt；
- stable target；
- retry exactly once；
- correlated event/post-state；
- close/send/restart false-success；
- server restart/gap/timeout。

### 故障与隐私

- malformed/oversized/duplicate worker frames；
- script error、panic、worker crash；
- Ctrl+C、parent exit、hard timeout；
- GUI/PTY/workspace 健康；
- source、argv、env、HTTP body/credential、terminal content、stdout 不进入
  retained audit/diagnostic；
- worker/child/task/stream/temp/pipe orphan 为零。

## 十四、自托管第一步

v0.1.9 只选择一个低风险、只读、结果可结构化比较的 helper 做双跑。

推荐候选：

```text
scripts/target-report.ps1
```

原因：

- 只读 target inventory；
- 能验证 fs/path/json/process 基础；
- 不影响 build artifact 正确性；
- PowerShell 与 Rhai 结果容易逐字段比较；
- 失败时可立即回退。

双跑要求：

- 同一输入生成结构化结果；
- 忽略明确的时间性字段后逐字段相等；
- 记录 duration 与错误；
- 默认仍以 PowerShell 为 last-known-good；
- Rhai 未通过 clean machine、取消、路径、编码和 recovery 前不替换原脚本。

明确不迁移：

- `build.bat`；
- `check.ps1`；
- qualification；
- package/release；
- credential/GitHub workflow。

## 十五、交付与预算

v0.1.9 不增加新 executable，集中完善现有：

```text
agenterm-script.exe
```

预算：

- `agenterm.exe` 4 MiB 上限不提高；
- `agenterm-script.exe` 使用独立 2 MiB 建议门；如 HTTP/TLS 使其不现实，
  必须先给出依赖和 clean release 实测，再由产品决定，不能顺手抬高；
- 第一窗口 1 秒门不提高；
- GUI 无 script startup work；
- local invocation startup、cache hit/miss、peak output/task 数进入报告；
- build/check/package 仍由现有 PowerShell last-known-good 驱动；
- clean candidate 仍只构建一次，package 消费同一批字节。

README 增加一个简短 script task 示例；完整 API、manifest 和错误合同由
`agenterm-script.exe api --json` 与 PRD 承担，避免 README 变成手册。

## 十六、依赖与并行波次

```text
波次 0：串行冻结
  profiles
  typed result/error
  task/stream handle
  catalog entry schema
  manifest schema
  stdlib first-delivery list
          |
          v
波次 1：可并行纯模块
  A. fs/path/temp
  B. env/process
  C. json/text/bytes
  D. task/stream/time
  E. manifest/module/catalog
  F. HTTP loopback fixture与 adapter spike
          |
          v
波次 2：串行 runtime 集成
  engine assembly
  local profile
  worker lifecycle
  CLI
          |
          v
波次 3：可并行
  Fleet adapter
  public black-box
  task/module dogfood
  self-host dual-run
  build/size/SBOM
          |
          v
波次 4：串行候选
  full journey
  privacy/orphan
  clean qualification
  package/release rehearsal
```

建议所有权：

| 分支 | 首选文件 |
|---|---|
| Runtime/contracts | `script_runtime.rs`, `script_catalog.rs` |
| Task/stream | `script_task.rs`, `script_stream.rs` |
| Files | `script_std/fs.rs`, `path.rs` |
| Process/env | `script_std/process.rs`, `env.rs` |
| Data | `script_std/json.rs`, `text.rs`, `bytes.rs` |
| HTTP | `script_std/http.rs`, loopback fixture |
| Modules/tasks | `script_module.rs`, `script_manifest.rs` |
| Fleet | `script_fleet.rs` |
| Tests | new script runtime black-box suites |

`Cargo.toml`、`src/bin/agenterm-script.rs`、worker protocol、catalog alignment 和
最终 qualification 是热点，只允许一个串行 owner 收口。

## 十七、验收门

### 门一：通用 local runtime

- fs/path/env/process/http/time/json/text/bytes 可形成真实纵向任务；
- local 默认且不被 agent 权限模型阉割；
- pure/observe 回归全绿；
- result/error/exit class 稳定。

### 门二：可组合执行

- task/stream/cancel/backpressure 有界；
- module/manifest/named task 可发现、可检查、可运行；
- invalid/degraded 不静默；
- sidecar 自然退出且无 orphan。

### 门三：Fleet 工具层

- operation catalog 100% mapped 或 degraded；
- mutation 具有 request/receipt/event/post-state；
- destructive facts 诚实；
- schema 可直接供后续 MCP/Agent 消费。

### 门四：自反馈

- 北极星任务完整通过；
- 首错诊断有界且隐私安全；
- timeout/crash/cancel/parent exit 后下一次 invocation 健康；
- 一个低风险 PowerShell helper 双跑一致。

### 门五：交付

- GUI startup/size 不回退；
- script size/startup/limits 有报告；
- required evidence 100%；
- clean candidate 和同字节 package 通过；
- tag/Release 仍需用户明确批准。

## 十八、主要风险

| 风险 | 早期信号 | 应对 |
|---|---|---|
| “完善”膨胀为复制 Node | 开始追 npm/JS compatibility | 锁定本地自动化纵向闭环 |
| local 又被安全模型阉割 | 每次 fs/process 都要 capability | agent policy 留给未来 agent 层 |
| 标准库变成一个大文件 | bin/runtime 同时塞 fs/http/task | 按域拆模块，先冻结 typed contracts |
| Rhai 异步模型难用 | API 假装 Promise 或靠 callback 地狱 | 显式 TaskHandle/StreamHandle |
| process 存在命令注入 | API 接收一个 shell string | executable + argv，shell 必须显式 |
| HTTP 拉大依赖 | script binary 明显超预算 | 先做 size spike，限制 feature |
| Fleet API 再造一套 | 手写几十个函数和帮助 | 从 operation catalog 生成/适配 |
| 任务清单变第二产品树 | CLI/GUI 各有 registry | 一个 `agenterm.tasks.json` catalog |
| task manifest 偷长成包管理器 | 出现 URL/signature/install hooks | task 与 future package manifest 分责 |
| Script 变成供应链信任根 | Rhai 代码决定签名/安装可信性 | softmgr 独占验证与事务 authority |
| 自托管过早影响发布 | Rhai 失败导致 check/release 不可用 | 只做低风险双跑，PS 保留 |
| 并行修改冲突 | 大家编辑 bin/Cargo/lib | 先拆模块、明确 owner、串行集成 |
| 测试依赖公网 | HTTP 测试偶发失败 | 只用 repo-owned loopback fixture |
| secret 进入诊断 | argv/env/body 出现在 bundle | sentinel 扫描 + metadata-only audit |

## 十九、首轮默认决策

建议接受：

1. v0.1.9 主线是 `agenterm-script.exe`，MCP 顺延 v0.1.10。
2. ordinary run/eval 默认 `local`。
3. pure/observe 保持专用 profile。
4. 首版标准库锁定 fs/path/env/process/http/time/json/text/bytes。
5. 异步模型使用显式 TaskHandle/StreamHandle。
6. manifest 使用 `agenterm.tasks.json` schema v1。
7. 模块只支持本地 project-root-relative。
8. process API 只接受 executable + argv。
9. HTTP 只做 client，不做 listener/socket/WebSocket。
10. Fleet API 从 operation catalog 系统映射。
11. GUI command palette 不是 blocker。
12. 自托管只做一个低风险 PowerShell helper 双跑。
13. v0.1.9 只交付 package-ready identity/provenance hooks，不实现包管理。
14. `agenterm.tasks.json` 与未来 package manifest 永久保持职责分离。

实施波次 0 仍需用 spike 确认：

- HTTP/TLS implementation 与 release size；
- task/stream 最终 API 名字；
- manifest 中 entry function 与 script argv 的模型；
- local profile 的默认 soft budgets；
- Fleet destructive operation 在 local 中的显式调用形式；
- 低风险自托管 helper 的最终选择。

## 二十、建议第一刀

```text
提交 1
  script catalog schema
  local profile
  typed result/error/exit
  api --json + check alignment

提交 2
  fs/path/temp + json/text/bytes
  Unicode/long-path/atomic/cleanup black-box

提交 3
  env/process
  argv/cwd/stdin/stdout/stderr/timeout/Job cleanup

提交 4
  task/stream/time
  cancellation/backpressure/natural exit

提交 5
  local modules + agenterm.tasks.json
  task list/show/run

提交 6
  loopback HTTP
  bounded body/timeout/cancel/privacy

提交 7
  generated Fleet API
  receipt/event/post-state conformance

提交 8
  north-star dogfood
  self-host dual-run
  clean qualification and release rehearsal
```

这一刀完成后，AgenTerm 不只是“内置 Rhai 的终端”，而是拥有一个能被人、
仓库自动化、MCP 和未来 Agent 共同复用的本地编程运行时。
