# AgenTerm Script Runtime 规范

状态：首版讨论稿

目标版本：v0.1.9

规范语言：中文；关键字 `MUST`、`SHOULD`、`MAY` 具有约束意义

产品所有者：[Rust host + Rhai scripting PRD](../prd/PRD_02_10_rhai_scripting.md)

执行计划：[AgenTerm v0.1.9 公开计划](../plan/plan-v0.1.9.md)

本文定义 `agenterm-script.exe` 面向脚本作者、宿主实现者、测试工具和未来
Agent 消费者的稳定运行时合同。它描述目标表面，而不是当前实现清单。
除非能力目录明确标记为 `shipped`，本文中的接口均不得被理解为已经交付。

## 1. 核心模型

AgenTerm Script Runtime 是：

```text
Rhai language
  + Rust-shaped 精选 std 子集
  + Rhai-native 扩展
  + AgenTerm-bound Fleet domain
```

推荐产品表述：

> AgenTerm Script 是以 Rhai 为语言、采用 Rust-shaped 标准能力，并叠加
> Rhai-native 与 AgenTerm-native 扩展的自动化运行时。

它不是 Rust、Node.js、Bun 或其他 Rhai 宿主的兼容层。Rust、Node.js 与
Bun 只用于复用成熟心智、检查问题域覆盖和发现遗漏；AgenTerm 不承诺其
语法、类型、模块、包、异步或 API 兼容。

### 1.1 一页 API 对象树

```text
agenterm-script
│
├─ Rhai language
│  ├─ let / fn / if / for / while / try-catch
│  ├─ array / map / string / number / bool
│  ├─ closures
│  └─ import
│
├─ globals                         极少量 invocation prelude
│  ├─ args                         只读脚本参数
│  └─ print(value)                 有界标准输出
│
├─ std::                           Rust-shaped 精选标准能力
│  ├─ fs::
│  │  ├─ read_to_string / read / write
│  │  ├─ metadata / read_dir
│  │  └─ create_dir_all / copy / rename / remove_*
│  ├─ path::
│  │  └─ Path / PathBuf 与常用路径操作
│  ├─ env::
│  │  └─ var / vars / current_dir / child environment
│  ├─ process::
│  │  └─ Command / Child / Output
│  └─ time::
│     └─ Duration / Instant / SystemTime
│
├─ rhai::                          Runtime 自有扩展
│  ├─ task::                       wait_all / race / cancel_all
│  ├─ http::                       request / start
│  ├─ json::                       parse / stringify
│  ├─ bytes::                      construction / conversion helpers
│  ├─ runtime::                    profile / limits / invocation facts
│  └─ package::                    明确延后
│
├─ typed objects                  identity/lifecycle 使用点号
│  ├─ Path / Metadata
│  ├─ Command / Child / Output
│  ├─ Duration / Instant / SystemTime
│  ├─ Task / Stream / Bytes
│  ├─ HttpResponse
│  └─ Receipt / Event / PostState
│
├─ fleet                          绑定当前 server/profile/broker
│  ├─ .workspace
│  ├─ .tabs.list() / .tabs.active()
│  ├─ .terminal(tab_id).capture(...)
│  └─ .events.read(...) / .events.wait(...)
│
├─ project composition
│  ├─ import "relative/module" as module
│  └─ agenterm.tasks.json
│
└─ discovery
   ├─ api [PATH] / api --json
   ├─ check
   └─ task list / show / run
```

对象树是用户入口，不是内部 Rust 模块图。产品能力分类可以更深，但公开
脚本表面 `surface_path` SHOULD 保持浅、可猜测和唯一。

## 2. 目标与非目标

### 2.1 目标

运行时 MUST：

- 让 Rust 程序员和熟悉 Rust 资料的 LLM 能借助 `std::fs`、
  `std::process::Command` 等心智快速开工；
- 保持 Rhai-native 的脚本体验，不暴露借用、生命周期、泛型、trait、
  `Pin`、`Poll` 或 executor 细节；
- 为文件、路径、环境、进程、时间、HTTP、任务、流和 Fleet 提供
  typed、bounded、cancellable、observable 的合同；
- 让同一个机器能力目录生成发现、检查、手册、覆盖率以及未来
  MCP/Agent schema；
- 在成功、失败、取消、超时和宿主崩溃后给出可验证终态，并清理
  invocation-owned 资源。

### 2.2 非目标

运行时不承诺：

- Rust 语言、Cargo crate、完整 Rust `std` 或 ABI 兼容；
- JavaScript/TypeScript、Node API、Bun API、npm 或包解析兼容；
- 模拟 Rust 的 `Result<T, E>`、`?`、trait、iterator 或 `Future` 类型系统；
- 浏览器 DOM、通用 socket server、远程任意 import 或常驻脚本 daemon；
- 充当 Agent 审批系统、软件包信任根或系统级安全沙箱；
- 为历史兼容保留重复的 sync/async API、旧别名或多种等价调用方式。

## 3. 规范关键字与能力状态

- `MUST`：实现和一致性测试必须满足。
- `SHOULD`：除非有记录在案且可测试的理由，否则必须满足。
- `MAY`：兼容合同允许，但不要求交付。

机器目录中的每个节点 MUST 标记状态：

```text
shipped       已实现且通过公开一致性测试
planned       已接受目标，尚未交付
experimental  可试用但不承诺稳定
deferred      已记录，当前版本不实现
unavailable   当前构建或 profile 不可用，并附原因
```

v0.1.9 目前是目标版本。文档生成器和 CLI MUST 避免把 `planned` 渲染成
`shipped`。

## 4. 命名空间所有权

### 4.1 `std::`

`std::` 由 Rust-shaped 标准能力独占。一个 API 只有同时满足以下条件才可
进入该空间：

1. Rust 标准库存在明确对应路径；
2. 主要用途和用户预期足够对应；
3. Rhai 适配后的差异可以被简短、精确地记录；
4. 名称不会暗示未实现的 Rust 类型系统或平台保证。

因此 `std::fs`、`std::path`、`std::env`、`std::process`、`std::time`
是候选；`std::http` 和高层 `std::task::spawn` MUST NOT 出现，因为 Rust
标准库没有这些对应的高层运行时能力。

### 4.2 `rhai::`

`rhai::` 由 AgenTerm Script Runtime 自有、跨产品领域的扩展独占：

- `rhai::task`：跨 I/O 域的任务组合；
- `rhai::http`：高层 HTTP(S) client；
- `rhai::json`：受预算约束的数据转换；
- `rhai::runtime`：当前 invocation、profile、limits 和版本事实；
- `rhai::package`：仅保留未来命名，不在 v0.1.9 实现。

`rhai::` 不是 Rhai 上游官方 API 的声明，也不表示所有 Rhai 宿主可用。

### 4.3 `fleet`

`fleet` 是当前 AgenTerm server、profile 和 broker 绑定的对象，不是静态
namespace。它 MUST 通过点号暴露带身份和 authority 的资源。

现有 Script API v1 的 `agent` facade 向 v2 `fleet` 迁移，是 v0.1.9 的
已接受方案，不是已交付事实。迁移期 SHOULD 提供明确诊断；除非另行批准，
不得永久保留两个等价 facade。

### 4.4 全局与 Prelude

全局空间 MUST 极小。首版目标只有：

- `args`：只读参数数组；
- `print(value)`：受输出预算限制的显示函数。

普通能力 MUST NOT 为了少写前缀而注入全局。运行时 MAY 提供显式 prelude
导入，但其内容、版本和冲突规则必须可发现，且不得静默覆盖用户符号。

### 4.5 静态能力与有状态对象

- 无 invocation identity 的能力使用 `::`；
- 具有 identity、状态或生命周期的值使用 typed object 与 `.`；
- 同一操作 SHOULD 只有一条规范调用路径；
- 便利函数不得掩盖资源所有权、取消或截断状态。

## 5. API 目录模型

每个公开能力 MUST 来自同一 typed catalog，并至少包含：

```text
stable_id
catalog_path
surface_path
rust_path
rust_mapping
semantic_differences
status
since / deprecated_since / removed_in
profiles
signatures
input / output / error schema
authority and side-effect facts
sync / task / stream behavior
timeout / cancellation behavior
soft limits / hard ceilings
secret-bearing fields
availability / degraded reason
```

四个路径字段不得混淆：

- `catalog_path`：用于产品分类、覆盖率和手册导航，例如
  `system/filesystem/read-text`；
- `surface_path`：用户实际调用路径，例如
  `std::fs::read_to_string`；
- `rust_path`：Rust 参照路径，例如 `std::fs::read_to_string`；无参照时
  为 `null`；
- `semantic_differences`：结构化列出错误、类型、阻塞、编码、平台和预算
  差异。

`rust_mapping` MUST 为以下之一：

```text
direct      名称、用途和主要语义高度对应
adapted     有明确对应，但为 Rhai/Windows/预算语义做了适配
inspired    只复用对象心智，不声称行为对应
none        AgenTerm/Rhai 自有能力
```

示例：

```json
{
  "stable_id": "script.std.fs.read_to_string.v1",
  "catalog_path": "system/filesystem/read-text",
  "surface_path": "std::fs::read_to_string",
  "rust_path": "std::fs::read_to_string",
  "rust_mapping": "adapted",
  "semantic_differences": [
    "成功时直接返回 string，失败时抛出 typed script error",
    "只接受 UTF-8；不猜测本地编码",
    "受单次读取与 invocation 累计字节预算限制"
  ],
  "status": "planned"
}
```

Node/Bun 对照 MAY 作为附加 research metadata，但 MUST NOT 参与运行时
解析或暗示兼容性。

## 6. Rust-shaped 标准能力

### 6.1 文件与路径

`std::fs` SHOULD 采用 Rust 熟悉的动词和返回对象。文本读取 MUST 明确
UTF-8；二进制读取 MUST 返回 `Bytes`。文件删除 MUST 只作用于明确解析的
目标，不得接受空路径、根目录或未解析的广泛目标。

`std::path` SHOULD 提供 `Path`/`PathBuf` 风格的不可变/可构建心智，但
不得模拟 Rust 借用。Windows drive、UNC、长路径、separator、Unicode、
canonicalization 和 reparse point 差异 MUST 有结构化说明和测试。

### 6.2 环境

`std::env` MUST 正确处理 Windows 环境变量名称语义。worker 内修改不得
改变父 AgenTerm 进程。child environment 的继承、overlay、replace 和
remove MUST 显式可区分。诊断不得记录 secret value。

### 6.3 进程

规范入口 SHOULD 是 `std::process::Command`：

```rhai
let command = std::process::command("git");
command.args(["status", "--short"]);
command.current_dir(repo);
let output = command.output();
```

进程启动 MUST 使用 executable + argv，不得将一个 command string
隐式交给 shell。需要 shell 时，用户必须显式启动 shell executable。

`Command`、`Child` 和 `Output` MUST 是 typed objects。`Child` 必须公开
清晰的 wait、cancel/kill 和 output 生命周期；parent 取消或退出后，不得
遗留 invocation-owned process tree。

### 6.4 时间

`std::time::Duration`、`Instant` 和 `SystemTime` SHOULD 复用 Rust 的对象
心智。monotonic deadline 与 wall clock MUST 分离。可取消 timer 属于
runtime task 能力；不得暗示 Rust `std::thread::sleep` 可取消。

## 7. Rhai-native 扩展

### 7.1 JSON

`rhai::json::parse` 与 `stringify` MUST 有深度、节点、输入和输出预算。
无效 UTF-8、超深数据与超限集合 MUST 返回稳定错误代码。

### 7.2 HTTP

`rhai::http` 首版目标仅为 HTTP(S) client：

```rhai
let response = rhai::http::request("GET", url, #{
    timeout: std::time::Duration::from_secs(10)
});
```

响应 MUST 区分 status、headers、bounded body、截断和 transport error。
超时与取消不得泄露 credential、proxy secret 或任意 body。首版不包含
listener、raw socket、WebSocket 或远程模块下载。

### 7.3 Runtime facts

`rhai::runtime` MAY 公开只读的 runtime/API 版本、profile、project root、
invocation ID 和有效 limits。它 MUST NOT 暴露 secret、私有 HWND、
renderer、PTY 内部字段或可绕过 Fleet authority 的句柄。

### 7.4 Bytes

`Bytes` 是带边界的 typed object，常用读取、切片和转换使用对象方法。
`rhai::bytes` 只拥有从 text/hex/base64 等值构造或跨值组合所需的 helper；
最终首版编码集合必须由 HTTP/process 真实旅程决定，不得复制一套与 Rhai
string 平行的文本库。

## 8. 错误语义

公开 API MUST 使用稳定的 typed error，而不是要求脚本解析 message。
错误至少包含：

```text
class
code
operation
safe_message
retryable
target_kind
truncated
cause_class（可选）
```

Rust-shaped API 不复制 `Result<T, E>` 和 `?`。成功时直接返回目标值；失败
时抛出可由 Rhai `try/catch` 捕获的 typed error。错误 message MAY 改善，
但自动化只能依赖稳定字段。

错误类 SHOULD 覆盖：

```text
script / configuration / data / io / child / network
limit / timeout / cancelled / host / fleet
```

源代码、环境值、HTTP credential/body、完整 argv、终端内容和任意大输出
MUST NOT 自动进入错误、audit 或 retained diagnostic。

## 9. Task、Stream 与异步

Rhai 脚本求值保持同步，不新增假装 JavaScript 的 `async/await`。异步 I/O
由 Rust host 推进，脚本通过显式 typed handle 组合。

```text
Rhai evaluation thread
  start() ───────────────> invocation-owned Rust task runtime
  Task.wait() <────────── typed completion / error / stream state
```

顺序调用 SHOULD 最短：

```rhai
let output = command.output();
let response = rhai::http::request("GET", url, #{});
```

显式并发 MUST 可见：

```rhai
let command = std::process::command("git");
command.args(["status", "--short"]);

let child = command.spawn();
let web = rhai::http::start("GET", url, #{});

let output = child.wait_with_output();
let response = web.wait(std::time::Duration::from_secs(15));
```

`Task` MUST 具有 invocation-local stable ID、状态、wait、cancel 和稳定终态。
迟到完成不得覆盖 `cancelled`。`rhai::task::wait_all`、`race` 和
`cancel_all` MUST 定义结果顺序、失败传播和取消传播。
可取消 `sleep(Duration)` 与 `after(Duration)` 归 `rhai::task`，不得伪装成
Rust `std::time` 自带的 executor 或可取消 `std::thread::sleep`。

`Stream` MUST 有界并表达：

- readable / closed / failed 状态；
- 当前读取是否完整；
- `truncated` 事实；
- bytes/text 转换错误；
- backpressure 和累计上限。

截断数据不得伪装成完整结果。丢弃最后一个脚本句柄不得自动取消仍由
invocation 拥有的 foreground task；foreground/background 与退出策略必须
显式定义。

## 10. 线程与宿主边界

后台线程 MUST 只保存 Rust typed payload、bytes、task state 和 cancellation
token。`Engine`、`Scope` 与任意 Rhai `Dynamic` MUST NOT 为 I/O 并发而在
线程间共享。只有 Rhai evaluation thread 在 wait/read 边界将 Rust 结果
转换为脚本值。

底层 MAY 使用 worker threads、channel/condition variable 或 async
executor。公开 Task/Stream 合同 MUST 与 Tokio 或任何 executor 解耦。

取消路径 MUST 覆盖：

1. Ctrl+C、deadline、parent exit 或显式 cancel 设置 invocation token；
2. HTTP、Fleet wait、timer 与 child process 观察 token 并停止；
3. `Task.wait()` 被唤醒并取得稳定取消错误；
4. CPU-bound Rhai 代码由 engine progress hook 中断；
5. grace period 后由 supervisor/Job Object 清理整个 process tree。

GUI、PTY 和 server MUST 不因脚本 wait、panic 或 runtime crash 被阻塞或
终止。

## 11. Execution Profiles

profile 是 runtime execution profile，不是未来 Agent 的角色或审批模型。

### `local`

目标默认 profile。权限等同于用户主动启动的普通本地程序，可使用完整
本地标准能力。它仍受 typed error、budgets、取消、资源所有权、隐私和
Fleet 公共操作合同约束。

### `pure`

用于确定性计算：无 ambient filesystem、environment、process、network、
clock 或 Fleet。输入输出保持 JSON-compatible，并受固定预算约束。

### `observe`

用于只读 Fleet：允许 workspace/tab/snapshot/capture/event read/wait；
不允许本地文件、进程、网络或 Fleet mutation。

同一 catalog entry MUST 声明 profile availability。不可用调用 MUST 在
`check` 阶段尽可能被发现，在运行时返回稳定 `configuration/authority`
错误，不能静默降级。

## 12. Fleet Domain

`fleet` MUST 从公共 typed operation catalog 系统派生，不得手写第二套
状态机或读取 GUI/PTY 私有字段。

每个 public operation 必须：

- 映射为脚本 API，或明确显示 `unavailable/degraded` 原因；
- 声明 observe/control/destructive 事实；
- 使用 stable target ID；
- mutation 使用 request identity、receipt、event position 和 post-state；
- retry 不重复副作用；
- server unavailable、restart、gap、timeout 和 false-success 分型。

示例（目标表面）：

```rhai
let active = fleet.tabs.active();
let screen = fleet.terminal(active.id).capture(4096);
print(screen.text);
```

```rhai
let receipt = fleet.tabs.set_note("@3", "build running");
receipt.wait(std::time::Duration::from_secs(5));
if !receipt.post_state.confirmed {
    throw "Fleet mutation was not confirmed";
}
```

operation classification 是工具事实，不是 Agent authorization。未来
`agenterm-agent.exe` MAY 根据这些事实过滤或审批工具，但不得迫使 Script
Runtime 复制一套标准库。

## 13. 模块、项目与命名任务

首版模块 MUST 是本地、project-root-relative：

```rhai
import "lib/report" as report;
report::run(args)
```

resolution MUST 防止未声明的 root escape，并对 cycle、missing、duplicate
identity 和 parse failure 分型。运行时不得扫描用户 home、PATH 或网络来
猜测模块。

命名任务清单固定为 versioned `agenterm.tasks.json`。它描述“如何运行本地
任务”，不是包清单，也不承担 URL、下载、签名、安装或信任。`task list`
和 `task show` MUST 不执行用户代码；无效任务 MUST 保持可发现并附
degraded reason。

## 14. Discovery 与手册生成

以下消费者 MUST 复用同一个 catalog：

```text
runtime registration
script check
api tree / api --json
reference manual
implementation coverage
Node/Bun/Rust research comparison
future MCP tool adapter
future Agent tool policy
```

目标 CLI：

```text
agenterm-script.exe api
agenterm-script.exe api std::fs
agenterm-script.exe api --status planned
agenterm-script.exe api --compare rust|node|bun|all
agenterm-script.exe api --json
agenterm-script.exe check FILE.rhai
```

`api` 默认 SHOULD 先显示对象树，再展开选定节点。手册页面 MUST 从
catalog 生成签名、状态、profile、错误、limits、Rust mapping 和语义差异，
避免维护第二份手写函数清单。

`check` MUST 不执行用户代码、不访问网络、不连接 GUI。它 SHOULD 验证
import、API 名称、signature、profile、manifest/runtime version，以及可
静态确认的 hard limit。

## 15. 版本与迁移

Runtime version、API schema version、manifest schema version 和单项 API
`stable_id` MUST 可独立发现。

兼容规则：

- 在同一 API major 内，已 shipped 的名称和稳定错误字段不得静默改义；
- 新增 optional 字段 MAY 向后兼容；
- 删除或重命名 MUST 先进入 deprecated 状态，并提供机器可读 replacement；
- `check` MUST 对 deprecated/removed API 给出精确迁移诊断；
- 不得仅为避免迁移而无限保留历史别名；
- `planned` API 在 shipped 前 MAY 调整，但调整必须同步计划、PRD 与
  catalog proposal。

`agent` -> `fleet` 是 Script API v2 的迁移提案/已接受计划。完成实现和
一致性证据前，文档必须继续标注为 planned。

## 16. Authority、安全与隐私

Script Runtime 的 `local` profile 是普通本地程序能力，不是受 Agent 审批
的沙箱。authority 边界来自：

- 当前 OS 用户；
- execution profile；
- Fleet 公共 typed operations；
- invocation-owned 资源；
- 明确 budgets 和 cancellation；
- 未来上层 Agent/softmgr 的独立策略。

运行时 MUST：

- 不绕过 Fleet broker 直接修改 GUI、PTY 或 workspace 私有状态；
- 不把 `agenterm-script.exe` 变成软件包签名信任根；
- 不在 audit/diagnostic 中保留 source、secret env value、credential、
  HTTP body、完整 terminal content 或任意大输出；
- 对日志只记录必要的 operation ID、计数、分类、duration、limit 和
  safe target facts；
- 对可能包含秘密的字段在 catalog 中机器标注；
- 在 child、stream、temp、pipe、task 和 worker 上保持明确所有权。

## 17. Budgets

每个 invocation MUST 有 hard ceilings，至少覆盖：

- wall time 与 CPU progress；
- 单次/累计输入输出 bytes；
- Rhai operations 或 progress ticks；
- collection/depth；
- tasks、children、streams 和 queue；
- HTTP body、redirect 与 deadline；
- module 数、source bytes 和 import depth；
- Fleet wait、event batch 和 capture bytes。

默认值、soft limits 和 hard ceilings MUST 由 `api --json` 公开。CLI 或
manifest override 只能在允许范围内调整。达到上限必须返回 typed limit
错误，清理 owned resources，并确保下一次 invocation 健康。

## 18. 完整示例

### 18.1 Rust-shaped 文件与 JSON

```rhai
let text = std::fs::read_to_string("agenterm.local.json");
let config = rhai::json::parse(text);

let output_path = std::path::PathBuf::from(config.output)
    .join("summary.json");

std::fs::write(
    output_path,
    rhai::json::stringify(#{ ok: true, source: "agenterm-script" })
);
```

### 18.2 argv-safe 子进程

```rhai
let command = std::process::command("git");
command.args(["status", "--short"]);
command.current_dir(std::env::current_dir());

let output = command.output();
if !output.success {
    throw output.error("git-status-failed");
}

print(output.stdout_text());
```

### 18.3 并发 HTTP 与进程

```rhai
let command = std::process::command("git");
command.args(["rev-parse", "HEAD"]);

let git = command.spawn();
let release = rhai::http::start("GET", release_url, #{
    timeout: std::time::Duration::from_secs(10)
});

let commit = git.wait_with_output();
let response = release.wait(std::time::Duration::from_secs(15));

print(#{ commit: commit.stdout_text().trim(), status: response.status });
```

### 18.4 Fleet observe 与 mutation evidence

```rhai
let active = fleet.tabs.active();
let capture = fleet.terminal(active.id).capture(8192);

let receipt = fleet.tabs.set_note(active.id, "captured");
receipt.wait(std::time::Duration::from_secs(5));

print(#{
    tab: active.id,
    text: capture.text,
    truncated: capture.truncated,
    confirmed: receipt.post_state.confirmed
});
```

这些示例表达目标 API，不代表 v0.1.9 能力已经 shipped。最终签名以机器
catalog 和一致性测试为准。

## 19. Conformance 与验收

一个能力只有同时满足以下条件才可标记 `shipped`：

1. catalog entry 完整，包括四类路径和 semantic differences；
2. Rust unit test 覆盖纯解析、状态与错误逻辑；
3. 通过 `agenterm-script.exe` 公共入口的黑盒测试；
4. 成功、typed failure、timeout、cancel 和 limit 有证据；
5. 无 child、worker、task、stream、pipe 或 temp orphan；
6. secret sentinel 未进入输出、audit 或 diagnostic bundle；
7. `api --json`、`check`、手册与 runtime registration 对齐；
8. pure/observe/local profile availability 符合 catalog；
9. GUI startup、PTY 和 server health 无回退；
10. 下一次 invocation 在故障后仍可成功。

规范级一致性套件 MUST 至少覆盖：

- Unicode、长路径、UNC、只读、占用和 access denied；
- environment overlay/replace/remove 与 parent isolation；
- executable/argv/cwd/stdin/stdout/stderr/nonzero exit；
- concurrent progress、backpressure、truncation、race 与迟到完成；
- loopback HTTP、disconnect、timeout、cancel 和隐私错误；
- module cycle/root escape/manifest version/invalid task visibility；
- Fleet stable target、receipt、event、post-state、restart 和 gap；
- malformed/oversized frame、panic、worker crash 和 parent exit。

## 20. 明确延后

以下能力不属于 v0.1.9 稳定合同：

- `rhai::package`、远程 registry、依赖解析、签名和安装事务；
- npm、Cargo crate、Node、Bun 或完整 Rust `std` 兼容；
- persistent script daemon、durable scheduler、watch mode 和 REPL；
- raw socket、listener、WebSocket 和公网 server；
- 任意远程 import；
- Agent 审批、自然语言权限策略和自主控制；
- 软件市场与 `agenterm-softmgr.exe`；
- 用 Rhai 替换 qualification、package 或 release 关键脚本；
- 把 executor 类型、Tokio 类型或 Rhai `Dynamic` 暴露为公开合同。

这些节点 MAY 保留在 catalog 中并标记 `deferred`，以便路线和手册能说明
“尚未交付”与“有意不做”的区别。

## 21. 首版待冻结问题

以下问题必须用 spike 和公开旅程收敛，本文暂不伪装成既定实现：

- `Command::output()`、`Child::wait_with_output()` 与统一 `Task` 的最终关系；
- `rhai::http::request` 返回直接 `HttpResponse` 还是可隐式等待的 Task；
- `Path`/`PathBuf` 是否首版都需要，或只提供一个不可歧义的 typed path；
- Task foreground/background 与脚本自然退出的精确规则；
- HTTP/TLS backend、executor 和 binary size 预算；
- local profile 的默认 soft budgets；
- Fleet destructive operation 的显式调用表面；
- prelude 是否除 `args`、`print` 外保持完全为空。

冻结这些问题时，决策顺序 SHOULD 是：

```text
用户最短路径
  -> 语义唯一性
  -> 取消与失败真实性
  -> 可生成 catalog
  -> 黑盒可证据化
  -> 实现与二进制成本
```
