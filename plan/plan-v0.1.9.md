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

这一轮主要增强 CLI/runtime，不给普通 GUI 堆新面板。AgenTerm Rhai
对象/接口树是稳定规范和首要产品合同；Rust std 是命名、模块和对象心智
的研究参照，但不复制 Rust 语言/类型系统，也不能驱动 Rhai API 改名；
Node.js/Bun
用于检查“本地自动化用途和组合能力”覆盖，不追求 JavaScript 语法、
Node API、npm 或 Bun 二进制兼容。

横向比较只用于发现问题域和能力缺口，不继承历史接口形状。AgenTerm
没有必要复制 callback/Promise 双轨、sync/async 重复族、旧别名、平台遗留
语义或包生态兼容层；每项能力只选择最符合 Rhai、typed contracts、
Windows-first、自反馈、取消与有界资源原则的 AgenTerm-native 设计。

## 〇.一、API 对象树（用户视角）

先看用户最终如何接触运行时，再看版本如何实施。下面是 v0.1.9 目标
surface，不是内部 Rust 模块树，也不是 Node/Bun 兼容表；交付状态、完整
函数和 deferred 节点在后文 Catalog 能力树展开。

```text
agenterm-script
│
├─ Rhai language                    语言本身，不伪装 Rust
│  ├─ args / print(value)
│  ├─ string / array / map / range
│  ├─ function / closure
│  └─ import "relative/module" as m
│
├─ std::                            Rust-shaped 精选小集
│  ├─ fs::
│  │  ├─ read / read_to_string / write
│  │  ├─ metadata / read_dir
│  │  ├─ create_dir_all / copy / rename
│  │  └─ remove_file / remove_dir
│  ├─ path::
│  │  ├─ Path / PathBuf
│  │  └─ join / parent / file_name / extension / canonical facts
│  ├─ env::
│  │  ├─ var / vars / current_dir
│  │  └─ worker-local mutation（记录与 Rust 的语义差异）
│  ├─ process::
│  │  ├─ command(...)
│  │  ├─ .arg() / .args() / .current_dir() / .env()
│  │  ├─ .status() / .output()
│  │  └─ .start() -> Child（Rust `spawn` 对照；Rhai 保留该词）
│  └─ time::
│     ├─ Duration
│     ├─ Instant
│     └─ SystemTime
│
├─ rhai::                           Rhai-native 高级扩展
│  ├─ task::
│  │  ├─ sleep / after
│  │  ├─ wait_all / race / cancel_all
│  │  └─ bounded Task/Stream composition
│  ├─ http::
│  │  ├─ request(...) -> HttpResponse
│  │  └─ start(...) -> Task
│  ├─ json::
│  │  └─ parse / stringify
│  ├─ bytes::
│  │  └─ from_text / concat / decode
│  ├─ runtime::
│  │  └─ version / profile / limits / cancellation
│  └─ future
│     ├─ package::
│     ├─ test::
│     └─ additional independently gated modules
│
├─ typed objects                    有 identity/lifecycle，使用 .
│  ├─ Task
│  │  └─ .id / .state / .wait() / .cancel()
│  ├─ Stream
│  │  └─ .id / .kind / .state / .buffered_bytes
│  │     / .read() / .collect() / .close() / .truncated / .complete
│  ├─ Bytes
│  │  └─ .len / .slice() / .to_text()
│  ├─ ProcessResult
│  │  └─ .success / .exit_code / .stdout / .stderr
│  ├─ HttpResponse
│  │  └─ .status / .headers / .body
│  ├─ Command / Child / Output
│  ├─ Duration / Instant / SystemTime
│  └─ Fleet values
│     ├─ Workspace / Tab / Terminal
│     ├─ Receipt / Event / PostState
│     └─ typed error / degraded reason
│
├─ fleet                            AgenTerm-bound，不冒充 std/rhai module
│  ├─ .workspace
│  ├─ .tabs
│  │  ├─ .list()
│  │  └─ .active()
│  ├─ .terminal(tab_id)
│  │  ├─ .capture(max_bytes)
│  │  ├─ .send(...)
│  │  └─ viewport/lifecycle
│  └─ .events
│     ├─ .read(...)
│     └─ .wait(...) / .start_wait(...)
│
├─ project automation               使用项目机制，不伪装 namespace
│  ├─ agenterm.tasks.json
│  └─ script task list / show / run
│
└─ discovery & evidence             使用 CLI/catalog，不塞入普通脚本
   ├─ script api [MODULE]
   ├─ script api --json
   ├─ script api --compare rust|node|bun|all
   ├─ script check
   └─ limits / audit / typed result / coverage
```

最短使用路径：

```text
std::fs::read_to_string(...)               Rust-shaped 基础能力
std::process::command(...).start()          Rust-shaped 资源对象
rhai::http::start(...) -> Task.wait()       Rhai-native 高级并发
fleet.tabs.active()                         AgenTerm-bound 领域对象
```

这张对象树是手册首页与 API discovery 的首屏；后文的产品分类树可以扩散，
但任何新增能力都必须先找到一个简洁的用户落点，不能把分类层级直接变成
调用前缀。

可以把整体理解为能力叠加，而不是语言兼容：

```text
AgenTerm Rhai Environment
  = Rhai language
  + Rust-shaped std subset
  + rhai-native extension set
  + AgenTerm-bound Fleet domain
```

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
├─ 最高优先级：Rust-shaped std 小集 + Rhai-native 扩展
│  ├─ std::{fs,path,env,process,time}
│  ├─ rhai::{task,http,json,bytes,runtime}
│  ├─ Rust path/name/object 心智优先，签名按 Rhai 适配
│  ├─ catalog 逐项记录 rust_path 与 semantic differences
│  ├─ Fleet 保持 invocation-bound object，不塞进 std/rhai
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
├─ 横切架构：稳定 server + 可替换 UI client
│  ├─ agenterm-server.exe 独占 workspace / PTY / scrollback / event truth
│  ├─ agenterm.exe 只拥有 HWND / layout / render / focus / clipboard
│  ├─ hello -> bounded bootstrap snapshot -> ordered delta -> reconnect
│  ├─ UI 更新或崩溃不改变 server PID、tab ID、PTY PID 与输出连续性
│  └─ 协议不兼容时保留 server，明确提示升级/重启，不自动杀会话
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
agenterm-script.exe api [MODULE] [--status STATE]
agenterm-script.exe api --json
agenterm-script.exe api --compare rust|node|bun|all
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

## 六、Rust-shaped 小集与 Rhai-native 扩展

### 命名所有权

API 先判断属于谁，再决定名字：

| Root | 可以进入的能力 | 不能进入的能力 |
|---|---|---|
| Rhai language/global | 原生 string/array/map/function/import，`args`、`print` | 大量 host convenience |
| `std::` | 与 Rust std 有明确、稳定概念对应的精选能力 | HTTP client、executor、Fleet、假 trait/泛型 |
| `rhai::` | 本运行时自有的高级能力和组合原语 | 冒充 Rust std 或 AgenTerm Fleet |
| `fleet` | 与当前 AgenTerm server/profile/broker 绑定的领域对象 | 通用文件/网络/进程函数 |

`std::` 不是营销标签。每个 entry 必须有 `rust_path`、mapping level 和
semantic differences；无法诚实说明对应关系就放入 `rhai::` 或 Fleet。

### `std::fs`

首版候选采用 Rust 熟悉名称：

```text
read / read_to_string
write
metadata / read_dir
create_dir / create_dir_all
copy / rename
remove_file / remove_dir
```

AgenTerm-specific atomic replacement 与 owned temp 若没有 Rust std 的直接
表面对应，不伪装成 `std::fs` 原生函数；它们可以返回 typed helper，或在
证据确认后进入 `rhai::runtime`/独立 extension。

要求：

- `read_to_string` 明确 UTF-8，`read` 返回 `Bytes`；
- 单次与累计 bytes 有界；
- Windows long path、Unicode、只读、占用、拒绝访问分型；
- atomic replace 不把失败报告为成功；
- remove 只作用于明确路径，不接受空/root/未解析 broad target；
- owned temp helper 记录所有权并在取消/崩溃路径清理。

### `std::path`

- 优先提供 `Path`/`PathBuf` 的 Rust-shaped value/method 心智；
- join、parent、file_name、extension、relative、normalize 只选择 Rhai
  中真正有用的 subset，不复制 borrow/OsStr/trait 层；
- project root 与 cwd 分开；
- Windows drive、UNC、separator、Unicode、long path 语义明确；
- canonicalization/reparse point 不静默改变报告的目标；
- 返回 typed path value 或规范 string，不能依赖显示文本解析。

### `std::env`

- `var`、`vars`、`current_dir` 优先对应 Rust 名称；
- worker-local mutation 与 Rust process-global environment 的语义差异必须
  显式记录；异步任务开始后的环境 snapshot/竞争规则先冻结再暴露；
- child environment 主要通过 `std::process::Command` 的 `env`、
  `env_remove`、`env_clear` 构造；
- 任何修改不影响 parent AgenTerm 进程；
- audit/diagnostics 记录 name/count，不记录 value；
- secret value 不进入 error、schema 或 retained bundle。

### `std::process`

用户心智采用 Rust 的 `Command -> Child/Output`：

```rhai
let command = std::process::command("git");
command.args(["status", "--short"]);
command.current_dir(repo);
command.env("NAME", "value");

let output = command.output();  // 本行取得终态
let child = command.start();    // `spawn` 是 Rhai 保留字
```

候选对象：

- `Command`：program、argv、cwd、env overlay/clear、stdin/stdout/stderr、
  timeout/output ceiling 和有限 Windows creation policy；
- `Child`：id/state、wait、kill/cancel、bounded stdout/stderr；
- `Output`：success、exit code、stdout、stderr、complete/truncated facts。

不复制 Rust ownership、`Stdio` trait plumbing 或平台内部 handle。禁止一个
command string 隐式调用 shell；需要 shell 时显式启动 `cmd.exe`、
PowerShell 或未来 `agenterm-bash.exe` 并提供 argv。

### `std::time`

只承接 Rust-shaped 时间值：

- `Duration`；
- `Instant`，用于 monotonic elapsed/deadline；
- `SystemTime`，用于 wall-clock；
- constructor/method 名尽量对应 Rust，不能混淆 wall clock 与 monotonic。

Rust std 没有高级 executor/timer runtime，因此 `sleep`、cancellable timer、
race 和 wait composition 不放进假的 `std::task`，归 `rhai::task`。

### `rhai::task`

- `sleep(Duration)` 与 `after(Duration)`；
- invocation-local Task/Stream identity；
- wait、wait_all、race、cancel/cancel_all；
- terminal state、迟到完成、失败传播和自然 worker exit；
- bounded queue/backpressure，不能把 truncation 报成完整成功。

### `rhai::http`

```text
request(method, url, options) -> HttpResponse
start(method, url, options) -> Task
```

首版包含 HTTP(S)、headers、text/bytes body、status、bounded body stream、
timeout/cancel，以及 proxy/TLS/connection 的无 secret 诊断。不包含 raw
socket、listener/server、WebSocket、任意 scheme 或自动远程 module 下载。
资格测试只使用本机 loopback fixture，不依赖公网。

### `rhai::json`、`rhai::bytes` 与 Rhai 原生 text

- `rhai::json` 提供 bounded parse/stringify；
- Rhai 原生 string 承担常见 UTF-8 text 操作，不重新包装一个平行 String；
- `Bytes` 是 typed object，`rhai::bytes` 只放 construction/conversion helper；
- explicit text/bytes conversion；
- hex/base64 是否进入首版由实际 HTTP/process 旅程决定；
- 深层 JSON、巨大 collection、无效 UTF-8 返回 typed limit/data error。

### `rhai::runtime`

只提供脚本确实需要读取的当前 invocation facts，例如 runtime/API version、
profile、有效 limits、source/project identity 与 cancellation state；不把
内部 supervisor、thread、Win32 handle 或 private broker 暴露成脚本 API。

### Catalog 能力全景树（不是脚本 namespace）

API 不使用一张不断变长的平面函数表，而使用稳定的
`domain -> capability group -> callable/type` 三层树。状态图例：

```text
[x] 已交付    [~] 已有基础但需扩展    [ ] v0.1.9
[>] 明确延后  [-] 有意不兼容/不属于 Script Runtime

agenterm-script
├─ runtime
│  ├─ entry
│  │  ├─ [x] run / eval / check
│  │  ├─ [x] api --json baseline
│  │  ├─ [x] api tree + module/status filters + Node.js/Bun comparison metadata
│  │  └─ [x] named task list / show / run
│  ├─ profile
│  │  ├─ [x] pure
│  │  ├─ [x] observe
│  │  ├─ [x] local foundation（显式选择、base Rhai、无需 server）
│  │  └─ [x] local ordinary default（首个可用 std slice 已交付）
│  └─ output
│     ├─ [x] print / bounded stdout
│     └─ [>] typed result / stderr / exit class（稳定结果信封、失败 code、
│        child/cancelled/fleet 分类及 CLI 退出码已交付；Rhai catch 内的
│        丰富 typed error object 已由 require_success 打通第一条纵切，
│        其余公共 API 仍待迁移）
│
├─ data
│  ├─ json
│  │  ├─ [x] parse
│  │  └─ [x] stringify / stringify_pretty
│  ├─ text
│  │  ├─ [ ] UTF-8 length / slice / search / replace
│  │  └─ [ ] encode / decode
│  └─ bytes
│     ├─ [>] text conversion / length 已交付；slice / concat 待补
│     ├─ [ ] hex / base64（由真实旅程决定）
│     └─ [>] hash / crypto（独立需求和供应链边界）
│
├─ system
│  ├─ fs
│  │  ├─ [x] read text / bytes
│  │  ├─ [x] write text / bytes + same-volume atomic promotion
│  │  ├─ [x] typed read_dir / DirEntry / metadata
│  │  ├─ [x] create / copy / rename
│  │  └─ [x] remove explicit target + broad-target rejection
│  ├─ path
│  │  ├─ [>] PathBuf / join / display / name / extension 已交付；parent 待补
│  │  ├─ [>] absolute 已交付；relative / canonical facts 待补
│  │  └─ [ ] Windows drive / UNC / long path
│  ├─ temp
│  │  ├─ [x] invocation-owned temp directory
│  │  └─ [x] success/failure/crash cleanup + atomic promotion
│  ├─ env
│  │  ├─ [x] var / has / names / current_dir
│  │  ├─ [>] process-global set/remove 明确延后
│  │  └─ [x] Command child env overlay / remove / clear
│  └─ process
│     ├─ [x] Command.output executable + argv
│     ├─ [x] Command.start -> invocation-owned Child
│     ├─ [x] bounded stdin / captured + live-stream stdout / stderr
│     └─ [>] exit / timeout / kill / Job cleanup 已交付；cancel token 待补
│
├─ concurrency
│  ├─ time
│  │  ├─ [>] Duration / wall clock 已交付；Instant 待补
│  │  └─ [x] sleep / cancellable timer
│  ├─ task
│  │  ├─ [x] timer wait / wait_all / indexed race
│  │  └─ [>] cancel / terminal state / HTTP typed payload 已交付；
│  │      Fleet payload 与 prompt transport abort 待补
│  └─ stream
│     ├─ [x] child stdout/stderr read / bounded collect
│     └─ [x] child/HTTP 64 KiB queue backpressure / truncation / close
│
├─ network
│  ├─ http client
│  │  ├─ [x] request / response / start -> Task
│  │  ├─ [x] duplicate headers / text / bytes / bounded stream
│  │  └─ [x] proxy / native TLS / timeout / logical cancellation diagnostics
│  └─ server and low-level
│     ├─ [>] HTTP listener / WebSocket / TCP / UDP
│     └─ [>] agenterm-net / libp2p / IPFS
│
├─ code-and-automation
│  ├─ module
│  │  ├─ [x] local relative import / explicit project root
│  │  └─ [>] root escape / missing / cycle 已分型；跨 invocation cache 延后
│  ├─ task-manifest
│  │  ├─ [x] agenterm.tasks.json schema v2
│  │  ├─ [x] project identity/version + entry/args/cwd/env names/profile
│  │  └─ [x] required Script API range + stable capability IDs
│  ├─ package
│  │  ├─ [-] npm / Node module compatibility
│  │  └─ [>] softmgr / signed package and application market
│  └─ development
│     ├─ [>] REPL / watch / test runner
│     └─ [>] bundler / transpiler / FFI / worker threads
│
├─ fleet
│  ├─ observe
│  │  ├─ [x] workspace / tabs / active tab / UI snapshot
│  │  ├─ [x] bounded capture
│  │  └─ [x] events read / bounded wait
│  ├─ control
│  │  ├─ [~] tab note metadata 已交付；完整 tree/Composer 待补
│  │  ├─ [ ] terminal input / viewport / workspace
│  │  └─ [ ] lifecycle / destructive explicit calls
│  └─ evidence
│     ├─ [~] 当前 mutation 的 request / receipt / event 已交付
│     └─ [~] 当前 mutation 的 post-state / replay / degraded reason 已交付
│
└─ observability
   ├─ [x] budgets / hard ceilings baseline
   ├─ [x] typed error / audit / crash isolation baseline
   ├─ [x] catalog hierarchy / availability / Rust/Node.js/Bun metadata 与
   │  人类可读 tree/filter/comparison CLI
   └─ [~] 英文 spec 与部分 catalog/runtime conformance 已交付；
      自动 manual/index 生成待补
```

这棵 catalog 树回答“产品覆盖了哪些问题域”，不直接规定用户必须写成
`system::fs::read_text()`。分类层级可以深，脚本调用表面必须浅。

### 用户脚本表面：Rust-shaped + Rhai-native + Fleet-bound

用户只需判断能力属于哪一层：

```text
Rust std 有稳定 analogue？
  ├─ yes -> std::，保留熟悉 path/name/object，记录语义适配
  └─ no
      ├─ 通用 Rhai runtime extension？ -> rhai::
      ├─ 有 identity/lifecycle？        -> typed object method
      └─ 绑定 AgenTerm server？         -> fleet object
```

选择规则：

- namespace 使用 Rhai 原生 `::`；
- `std::` 只放可诚实映射的 Rust std 小集，不追求完整 Rust；
- `rhai::` 放 HTTP、Task executor、JSON convenience 等自有扩展；
- 有资源 identity 或生命周期的值使用方法，例如 `child.wait()`、
  `response.body.read()`；
- 不暴露 `runtime::data::...`、`system::...`、`network::...` 等 catalog 壳；
- Rhai 原生 string/array/map 不包装成伪 `std::string/vec/collections`；
- 不实现假的 Rust trait、borrow、generic、`Result/?` 或 Future/Poll 表面；
- named task 属于 manifest/CLI，不与并发 Task 混成同一注册表；
- Fleet 与当前 server/profile/broker 绑定，使用 `fleet` object；
- 当前 v1 的 `agent` 会与未来 `agenterm-agent.exe` 概念冲突。v0.1.9
  建议升 Script API v2，以 `fleet` 为唯一 canonical 名称；`check` 针对
  旧 `agent.*` 给出明确迁移诊断，不长期保留第二别名。

普通脚本应当让 Rust 用户直接猜中主要结构：

```rhai
let config = rhai::json::parse(
    std::fs::read_to_string("agenterm.local.json")
);

let command = std::process::command("git");
command.args(["status", "--short"]);
command.current_dir(config.repo);

let output = command.output();
if !output.success {
    throw output.error;
}

print(output.stdout);
```

Fleet 操作从绑定对象出发，不暴露内部 operation ID：

```rhai
let active = fleet.tabs.active();
let screen = fleet.terminal(active.id).capture(4096);
print(screen.text);
```

operation ID、receipt、event 和 post-state 仍存在于 typed result/catalog，
只是普通路径不要求用户手工拼接它们。

每项 API 同时记录四个坐标：

```text
catalog_path         system / filesystem / read-text
surface_path         std::fs::read_to_string
rust_path            std::fs::read_to_string
semantic_difference  typed Rhai exception instead of Result<T, io::Error>
```

Rust 是首要的命名与对象心智参照，但仍不是兼容目标：

| AgenTerm surface | Rust analogue | mapping | 必须公开的差异 |
|---|---|---|---|
| `std::fs::read_to_string` | 同路径 | adapted | typed Rhai error，不返回 Rust `Result` |
| `std::process::Command` | 同对象心智 | adapted | 无 ownership/trait/OS handle surface |
| `std::time::Duration` | 同类型心智 | adapted | Rhai number 与 hard ceiling |
| `rhai::task` | 无高层 std analogue | native | executor-neutral Task/Stream |
| `rhai::http` | 无 Rust std HTTP client | native | 有界 client，不影射某个 crate |
| `fleet` | 无对应物 | AgenTerm-specific | 绑定 server/profile/broker |

Node.js/Bun 继续作为问题域覆盖参照，不表示 API 兼容。横向比较采用
“用途相似”而不是“函数同名”：

```text
Rust std + Node.js / Bun
  提供问题域、成熟用例和遗漏检查
          |
          v
  不复制 API 形状或历史兼容层
          |
          v
AgenTerm-native contract
  Rhai-native + typed + Windows-first
  bounded + cancellable + observable
  one preferred path, few compatibility aliases
```

| AgenTerm 领域 | Node.js 主要参照 | Bun 主要参照 | v0.1.9 策略 |
|---|---|---|---|
| fs/path | `node:fs`, `node:path` | `Bun.file`, `Bun.write`, Node-compatible fs | Windows-first typed subset |
| env/process | `process`, `node:child_process` | `Bun.env`, `Bun.spawn` | executable + argv，无隐式 shell |
| http | global `fetch`, HTTP/HTTPS modules | `fetch` | 只做有界 client |
| task/stream/time | Promise/timers/streams | Promise/Web Streams/`Bun.sleep` | Rhai typed handles，不模仿 Promise |
| modules/tasks | ESM/CJS/package scripts | module resolver/package scripts | 本地 Rhai module + AgenTerm task |
| data/text/bytes | JSON/string/Buffer | Web/Bun binary APIs | 小而显式、有界转换 |
| Fleet | 无对应物 | 无对应物 | AgenTerm 的核心差异能力 |
| package/tooling | npm/npx | package manager/build/test | 延后给 AgenTerm 组件生态 |

对照基线记录来源和复核日期，不能写成兼容性承诺。初始参照为
[Rust std](https://doc.rust-lang.org/std/)、
[Node.js API index](https://nodejs.org/api/) 与
[Bun API index](https://bun.sh/docs/runtime/bun-apis)（2026-07-28 复核）。

## 七、Task 与 Stream 模型

### 用户心智：默认顺序执行，需要并行时才显式 start

Rhai 不需要伪装成 JavaScript Promise，也不新增 `async`/`await` 语法。
普通 I/O 使用阻塞脚本调用：

```rhai
let command = std::process::command("git");
command.arg("status");
let output = command.output();

let response = rhai::http::request("GET", url, #{});
```

这里“阻塞”的只是本次 `agenterm-script.exe` 的 Rhai evaluation thread，
不会阻塞 AgenTerm GUI、PTY、server 或其他 invocation。需要并行时，只有
可能长时间运行的 API 提供语义不同的 `start`/`spawn`：

```rhai
let command = std::process::command("git");
command.args(["status", "--short"]);
command.current_dir(repo);

let web = rhai::http::start("GET", release_url, #{});
let child = command.start();
let timeout = std::time::Duration::from_secs(15);

// 两项已经并行运行；wait 的书写顺序不等于执行顺序。
let response = web.wait(timeout);
let output = child.wait_with_output(timeout);
```

这不是为每个函数复制 `fooSync/foo/fooAsync` 三套 API。规则是：

- 快速、本地、有界操作只提供直接调用；
- 外部 I/O/进程/Fleet wait 提供直接终态调用和显式 start/spawn；
- `Command.output`、`http::request`、`wait` 表示本行取得终态；
- `Command.start` 返回 `Child`，`http::start` 返回 `Task`；catalog 中保留
  Rust `Command::spawn` 对照，脚本不使用 Rhai 保留字 `spawn`；
- `Child`、`Task`、`Stream` 共享一致的 state/cancel/deadline 心智，但不
  为统一而抹掉有价值的 typed object；
- `rhai::task` 提供跨 waitable 的组合面；
- v0.1.9 不并行执行任意 Rhai closure，不引入 worker-thread 共享脚本状态。

首版使用显式 typed handle：

```text
TaskHandle
  id
  kind
  state = pending|running|completed|failed|cancelled

StreamHandle
  id
  kind = bytes（text/json-lines 仅保留未来扩展空间）
  state = pending|readable|closed|failed|cancelled
  buffered_bytes
  truncated
  complete
```

已冻结的 child Stream API：

```text
handle.wait(Duration?)
handle.cancel()
stream.read(max_bytes, Duration?)
stream.collect(max_bytes, Duration?)
stream.close()
rhai::task::wait_all(waitables, Duration?)
rhai::task::race(waitables, Duration?)
rhai::task::cancel_all(waitables)
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

### Rust host 如何实现

Rhai evaluation 本身保持单线程、同步。异步能力属于 Rust host：

```text
agenterm-cli supervisor
  ├─ deadline / Ctrl+C / Job Object / forced cleanup
  └─ framed protocol
       |
agenterm-script process
  ├─ frame loop
  │    └─ 即使 Rhai 正在 wait，仍能接收 cancel frame
  ├─ Rhai evaluation thread
  │    ├─ 执行普通表达式
  │    ├─ start() 只登记任务并立即返回 Task ID
  │    └─ wait() 等待 registry condition，不占用 GUI/server
  └─ invocation-owned task runtime
       ├─ child process + stdout/stderr pumps
       ├─ HTTP request/body pump
       ├─ timer
       └─ Fleet broker wait
            |
            └─ completion/result/stream queue/cancel token
```

Rust 的 `async fn` 会产生 `Future`，Future 必须由 executor poll 才推进；
Tokio 是一种 executor/runtime，但不是唯一实现。v0.1.9 不应因为“异步”
二字就立即加入 Tokio。波次 0 比较两种实现：

1. 小型 fixed worker/thread + channel/condition variable；
2. 小型 async executor，在 HTTP streaming、timer 与 cancellation 明显更
   简单且二进制预算可接受时才采用。

用户看到的 `Task` 合同与底层选择无关。后台任务保存 Rust typed payload、
bytes 和状态；只有 Rhai thread 在 `wait/read` 时把结果转换成 `Dynamic`。
这样不需要把 `Engine`、`Scope` 或任意 Rhai value 跨线程共享，也不需要
仅为 I/O 并发开启 Rhai `sync` feature。

取消有三层：

1. frame loop 收到 cancel，设置 invocation cancellation token；
2. task runtime 取消 HTTP/Fleet wait，终止 child 并唤醒 `Task.wait()`；
3. Rhai CPU loop 由 `Engine::on_progress` 观察 token；超出 grace 后由
   supervisor/Job Object 强制清理。

因此“脚本看起来同步”与“系统能够并发、取消、不冻结 GUI”并不矛盾。

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

已交付 schema v2：

```json
{
  "schema_version": 2,
  "project": {
    "id": "daily-tools",
    "version": "1.0.0",
    "requires": {
      "script_api": {"minimum": 2, "maximum": 2},
      "capabilities": [
        "runtime.project.named-task",
        "std.process.command"
      ]
    }
  },
  "tasks": [
    {
      "id": "daily-check",
      "description": "Run the local daily check",
      "entry": "tasks/daily-check.rhai",
      "profile": "local",
      "cwd": ".",
      "args": [],
      "env": ["REQUIRED_ENV_NAME"]
    }
  ]
}
```

约束：

- project `id/version` 与 task `id` 是稳定、可发现的 identity，
  description 只是显示；
- tasks 保留 manifest 顺序；
- invalid task 不消失，显示 `status:degraded` 和 degraded reason；
- duplicate、unknown field、bad version、root escape、missing script 分型；
- `env` 只保存 required name，不保存 secret env values；
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

稳定性优先级固定为：

```text
Rhai surface_path / object semantics
  > Script API major rules
  > current typed catalog
  > Rust / Node.js / Bun comparison metadata
  > internal Rust implementation names
```

每个 API entry 至少包含：

- stable ID；
- `catalog_path`（domain/group/name）与稳定排序键；
- canonical `surface_path`、module/type/function/signature；
- optional `rust_path`；
- Rust mapping level：`direct|adapted|inspired|none`；
- machine-readable `semantic_differences`，至少覆盖 error、type、blocking、
  cancellation、platform 和 limit 差异；
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
- `comparison` metadata：`rust`/`node`/`bun` analogue、关系
  `similar|agenterm-specific|deferred|not-applicable`、reference/version 与
  last-reviewed；它只用于差距分析和手册，不参与运行时语义。

人类默认看到树，机器看到同一棵树的 JSON：

```text
agenterm-script.exe api
agenterm-script.exe api std::fs
agenterm-script.exe api rhai::task
agenterm-script.exe api --status planned
agenterm-script.exe api --compare rust|node|bun|all
agenterm-script.exe api --json
```

树、横向比较表、参考手册索引和实现覆盖率都从 catalog 生成；仓库不维护
第二份手写函数清单。未知、degraded 和 deferred 节点也保留在树中，防止
“没实现”被误读成“忘记记录”。

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
- `agenterm-script.exe` 可以提供文件、HTTP、进程等通用自动化能力，
  hashing/crypto 是否加入须另立需求，
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

## 十四、自托管与渐进置换

这不是一次性重写 `.ps1`，而是从 v0.1.9 开始平行建设 `.rhai` 脚本集，
把仓库自身作为运行时的长期真实负载。迁移和归档以单项能力为单位闭环，
不设置“统一等到某个版本再整理”的总开关。

```text
parallel
  PowerShell last-known-good + Rhai candidate
        |
        | same inputs / structured outputs / exit class / cleanup
        v
parity-proven
  clean-machine、路径、编码、取消与恢复持续等价
        |
        v
default-rhai
  默认入口切到 Rhai，PowerShell 仍可显式回退
        |
        v
PowerShell archived
  单项默认入口切换后立即归档对应旧脚本
  archive 不再被正常 build/check/release 路径引用
```

第一项已落地：

```text
scripts/rhai/verify-script-contract.rhai
```

- 读取英文 runtime spec 与机器 API catalog；
- 校验稳定性、设计日期、版本和对象树合同；
- 由公开 `agenterm-cli script run --profile local` 在黑盒套件中执行；
- PowerShell 目前只负责生成测试 fixture 和作 qualification fallback。

首个既有脚本闭环已经选择 Cargo target inventory：Rhai 版本推动了 typed
read_dir、metadata、absolute path 和 SystemTime，正常 build/check 调用点切换
后，旧 `scripts/target-report.ps1` 进入 PowerShell archive。

第二个闭环是 internal-only version policy：Rhai 版本通过 `Command.output`
调用 argv-safe Git，校验 typed exit/capture，并读取 release/workflow 合同；
正常 check 调用点切换后，旧 PowerShell 实现进入 archive。

后续每项双跑要求：

- 同一输入生成结构化结果；
- 忽略明确的时间性字段后逐字段相等；
- 对比退出分类、duration、错误与 orphan/临时文件清理；
- Rhai 失败不遮蔽 PowerShell 结果；
- 未满足 clean machine、取消、路径、编码和 recovery 前不切默认入口。
- 切换默认调用方后，把对应 `.ps1` 移入明确的 PowerShell archive，
  并在 PRD 记录旧路径、新路径、切换提交和验证证据；
- archive 只承担限时回退与历史对照，不得继续被常规入口调用；确认无需
  回退后可删除工作树副本，完整历史仍由 Git 保留。

v0.1.9 不切换以下关键流程的默认实现，但允许提前编写 Rhai 候选并双跑：

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
- `agenterm-script.exe` 原 2 MiB 建议值经 HTTP/native-TLS spike 复核：
  clean release 为 2,359,808 bytes；采用系统 TLS/root verifier、关闭 ureq
  默认 rustls/gzip feature，并保留仓库已经审核的 3 MiB artifact gate，
  本轮不再抬高；
- 第一窗口 1 秒门不提高；
- GUI 无 script startup work；
- local invocation startup、cache hit/miss、peak output/task 数进入报告；
- build/check/package 仍由现有 PowerShell last-known-good 驱动；
- clean candidate 仍只构建一次，package 消费同一批字节。

README 增加一个简短 script task 示例；稳定运行时合同由
[`docs/agenterm-script-runtime.md`](../docs/agenterm-script-runtime.md)
承载，机器事实由 `agenterm-script.exe api --json` 承载，PRD 拥有产品
状态，避免 README 或计划变成第二份手写手册。

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
  A. std::fs/path + Path/Bytes/temp
  B. std::env/process/time + Command/Child/Duration
  C. rhai::json/runtime
  D. rhai::task/stream
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
| Rust-shaped std | `script_std/fs.rs`, `path.rs`, `process.rs`, `env.rs`, `time.rs` |
| Rhai extensions | `script_rhai/task.rs`, `json.rs`, `bytes.rs`, `runtime.rs` |
| HTTP | `script_rhai/http.rs`, loopback fixture |
| Modules/tasks | `script_module.rs`, `script_manifest.rs` |
| Fleet | `script_fleet.rs` |
| Tests | new script runtime black-box suites |

`Cargo.toml`、`src/bin/agenterm-script.rs`、worker protocol、catalog alignment 和
最终 qualification 是热点，只允许一个串行 owner 收口。

## 十七、验收门

### 门一：通用 local runtime

- `std::{fs,path,env,process,time}` 与
  `rhai::{task,http,json,bytes,runtime}` 可形成真实纵向任务；
- catalog/surface/rust paths、mapping 和 semantic differences 可发现；
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

- `lint.ps1` 在昂贵测试前统一执行 Rust、PowerShell、JSON 和生产 Rhai
  的 fail-fast 检查，并能输出机器可读结果；
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
| Rust-shaped 被误写成 Rust-compatible | 开始复制 trait/borrow/Result/Future | 每项 mapping + semantic differences |
| LLM 因近似名称过度推断 | 生成未支持的 Rust std API | 完整树、unknown节点、check迁移建议 |
| 自有能力冒充 std | 出现 `std::http`/高层`std::task::spawn` | 无真实 std analogue 就归 `rhai::` |
| local 又被安全模型阉割 | 每次 fs/process 都要 capability | agent policy 留给未来 agent 层 |
| 标准库变成一个大文件 | bin/runtime 同时塞 fs/http/task | 按域拆模块，先冻结 typed contracts |
| Rhai 异步模型难用 | API 假装 Promise 或靠 callback 地狱 | 显式 TaskHandle/StreamHandle |
| catalog 分类泄漏进用户代码 | 出现 `system::network::http` | catalog_path 与 shallow surface_path 分离 |
| 为“异步”直接引入大 runtime | 未做 spike 就加入 Tokio | Task 合同与 executor 解耦，按证据选型 |
| Rhai value 跨线程共享 | worker 持有 Dynamic/Scope/Engine | 后台只存 Rust payload，Rhai 线程转换 |
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
4. 首版能力锁定 `std::{fs,path,env,process,time}` 与
   `rhai::{task,http,json,bytes,runtime}`；Rhai 原生 string/array/map 直接
   复用。
5. 异步模型使用显式 TaskHandle/StreamHandle。
6. manifest 使用 `agenterm.tasks.json` schema v2，显式声明兼容的 Script
   API 范围和必需 stable capability IDs。
7. 模块只支持本地 project-root-relative。
8. process API 只接受 executable + argv。
9. HTTP 只做 client，不做 listener/socket/WebSocket。
10. Fleet API 从 operation catalog 系统映射。
11. GUI command palette 不是 blocker。
12. 自托管只做一个低风险 PowerShell helper 双跑。
13. v0.1.9 只交付 package-ready identity/provenance hooks，不实现包管理。
14. `agenterm.tasks.json` 与未来 package manifest 永久保持职责分离。
15. catalog taxonomy 与脚本 surface 分离；每项记录 catalog/surface/rust
    path、mapping level 与 semantic differences。
16. `std::` 是 Rust-shaped 精选小集，`rhai::` 是 runtime-native 扩展；
    static namespace 使用 `::`，有 identity 的 typed object 使用点号方法。
17. canonical Fleet facade 使用 `fleet`；Script API v2 不长期保留 `agent`
    别名，只提供明确迁移诊断。
18. Rhai 不新增 async/await；直接调用服务顺序脚本，`start` + `Task` 服务
    显式并发。
19. Task/Stream 合同不绑定 Tokio；executor 由波次 0 的体积、取消和
    streaming spike 决定。

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
  [x] typed script catalog schema v3（Script API 仍为稳定 v2；v3 增加
      Node.js/Bun 研究比较合同）
  [x] explicit local profile foundation
  [>] typed result/error/exit expansion（稳定结果信封与退出分类已交付；
      catchable typed error object 已交付首个 process 纵切）
  [>] api --json + check alignment（v1 agent method 已由 catalog 驱动；
      nested namespace/signature alignment 随首个 std slice 完成）

提交 2
  [x] std::fs/path + PathBuf/Bytes + rhai::json 首个可用切片
  [x] ordinary default 切换到 local
  [>] Unicode 基线和 bounded privacy error 已覆盖；
      long-path/atomic/cleanup 继续扩展

提交 3
  [x] std::env/process + Command/Child/Output + Duration
  [x] argv/cwd/env/stdin/stdout/stderr/timeout/kill/Job cleanup 已交付
  [>] Instant 与跨资源 cancel token 继续进入后续波次

提交 4
  [x] rhai::task + timer Task + child-process Stream
  [x] bounded read/collect、64 KiB queue backpressure、truthful truncation
  [>] HTTP/Fleet typed Task payload 与跨资源 cancellation 继续扩展

提交 5
  [x] local modules + agenterm.tasks.json
  [x] task list/show/check/run
  [x] schema v2 API/capability requirements + fail-closed compatibility

提交 6
  [x] rhai::http + independent loopback HTTP
  [x] bounded body/timeout/cancel/privacy + typed HTTP Task payload
  [x] clean release 2,359,808 bytes；保留既有 3 MiB 门，不再抬高

提交 7
  [x] 从全部 15 个现有 typed operations 系统生成 Fleet API
  [x] observe/local 权限、receipt/event/post-state 与迁移诊断

提交 8
  [x] north-star dogfood（直接 agenterm-script task list/show/check/run）
  [x] self-host dual-run
  [x] release rehearsal：四个 release 二进制均低于既有预算
  [x] qualification：候选 `5b6c310` 的 clean release + stress 全绿，
      receipt 绑定四个精确二进制哈希；target 清理后仅余约 7 KiB 证据
```

这一刀完成后，AgenTerm 不只是“内置 Rhai 的终端”，而是拥有一个能被人、
仓库自动化、MCP 和未来 Agent 共同复用的本地编程运行时。

## 二十一、稳定 Server 与可替换 UI

用户目标不是“把旧窗口重新画一次”，而是：

```text
agenterm-server.exe（稳定）
  workspace / tree / active tab
  ConPTY / child PID / parser / scrollback
  composer draft / cwd / proxy facts
  operation receipt / event journal
              │
              │ versioned loopback protocol
              ▼
agenterm.exe（可替换）
  HWND / theme / layout / renderer
  focus / selection gesture / clipboard
  settings surface / close confirmation
```

采用独立内部 `agenterm-server.exe`，而不是让 `agenterm.exe --server`
长期驻留。原因是 Windows 会锁住正在运行的 executable image；若 server
仍映射 `agenterm.exe`，构建后的新 GUI 仍不能稳定替换同一路径，违背这一
需求本身。

协议最小闭环：

```text
GUI ui.hello(build, protocol range, capabilities)
  -> server compatibility decision + epoch
  -> ui.bootstrap(workspace, tabs, active, terminal screen, event position)
  -> ui.subscribe(after position)
  -> ordered snapshot/delta rendering
  -> typed commands / receipts / correlated events
  -> ui.disconnect or reconnect
```

所有权规则：

- server 是 tab/tree/PTY/scrollback 的唯一事实源；
- GUI 不复制可变 session truth，只持有带 epoch/sequence 的 projection；
- HWND、像素布局、主题、焦点、菜单和剪贴板永远不进入 server；
- tree selection、terminal viewport size 和输入通过单一 interactive lease
  写回 server，避免两个 GUI 同时改变 PTY 尺寸；
- server 与 GUI 任一侧发现 epoch restart、journal gap 或协议不兼容时均
  fail closed，不用“看起来还能画”掩盖状态缺口；
- UI close 默认只断开 client；关闭 server 仍是独立、明确、可审计动作。

迁移波次：

```text
S0  [x] ownership inventory + typed discovery/negotiation 已交付；
        renderer-neutral hello/bootstrap/screen/delta DTO、schema 与 hard limits
        已交付，且 capability flags 按当前事实独立发布
S1  [x] current combined server 已能从同一
        ControlHost/TerminalTab/EventJournal truth 生成 bootstrap 和受影响标签
        的完整 delta post-state；独立 state owner 仍属于 S3
S2  [~] `ui-hello`、`ui-bootstrap`、有界 `ui-deltas` polling 已通过公共
        loopback IPC 与黑盒 snapshot-follow/restart/gap 证据；专用
        subscription transport 与 GUI consumer/reconnect 进入 S4
S3  [~] internal `agenterm-server.exe` 已能无 HWND 地持有 workspace、
        tab/tree、真实 ConPTY、parser/scrollback、event journal 和共享
        operation replay/receipt authority，并通过公共 hello/bootstrap/
        delta/PTY/receipt 黑盒；完整 server 命令面与默认 authority 切换待补
S4  [~] server-owned 单活 interactive UI lease 已交付 attach/idempotent
        renewal/live-owner conflict/heartbeat/detach 与 dead/expired 回收；
        `ui-interact` 已对 stable-ID select、有界 binary input、PTY resize
        强制 exact-live-lease；opt-in `agenterm.exe --ui-client` 已能启动/
        连接独立 server、渲染 DTO、确认 applied sequence、关闭后保留 PTY，
        且 replacement GUI 恢复同一 server/tab/terminal marker；普通启动
        默认切换、旧工作台完整 parity、in-place reconnect 与 rollback 待补
S5  same-server GUI upgrade + rollback black-box
S6  remove combined-process handoff only after parity gates
```

验收必须同时证明：

- server PID、epoch、tab IDs、PTY child PIDs 不变；
- 长命令在 UI 退出、更新、重开期间继续输出；
- 新 GUI 的 HWND 和 build identity 改变，截图出现新布局；
- scrollback、active tab、draft、cwd/proxy facts 无损；
- 新 GUI 启动失败或不兼容时，原 server 与 PTY 仍健康；
- 可回滚到上一兼容 GUI；
- server 未启动时仍保持一条清晰、无多 server 竞态的 bootstrap 路径。
