# AgenTerm v0.1.10 公开计划

状态：讨论稿  
工作主题：**可验证的只读 Agent 桥梁**  
版本定位：在 v0.1.9 完善通用 Rhai 运行时、模块任务与机器可读工具
schema 后，第一次把同一份 Fleet 事实稳定地开放给外部 Agent 客户端。

本文是版本执行计划与决策记录，不是产品事实，也不得成为实现依赖。
评审接受的能力、边界和验收条件必须同步进入对应 `PRD.md` 模块；完成后
保留本文作为交付历史。

## 〇、产品判断

AgenTerm 不需要通过堆叠可见按钮与竞品竞争。v0.1.10 延续以下准绳：

> 界面简单实用，软件稳定可靠，编程接口丰富，并为扩展保留足够空间。

这一轮的外部能力主要出现在接口层，而不是 GUI：

- 默认工作台不增加 MCP 面板、连接列表、授权弹窗或常驻状态动画；
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
v0.1.10  可验证的只读 Agent 桥梁
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
│  ├─ unavailable 能力显式可发现，不静默消失
│  └─ 为后续 Rhai control 与 agenterm-agent.exe 保留复用边界
│
├─ 第一优先级：自反馈与兼容资格
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
   ├─ agenterm-script.exe 完整 Node/Bun 级标准库的剩余扩展
   └─ 安装器、自动更新、联网组件安装与未单独批准的公开发布
```

## 二、北极星演示

v0.1.10 必须能够通过以下一条完整旅程解释自身价值：

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

### 完成定义

v0.1.10 public-ready 必须满足：

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

## 四、MCP 协议基线

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

## 五、Executable 与 server 选择

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

## 六、Resource 模型

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

## 七、唯一工具：`agenterm_wait`

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

## 八、架构边界

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
- 如评估第三方 MCP Rust 实现，必须先证明协议 revision 可固定、依赖审计
  可接受、release binary 不超预算、panic/stdio 行为符合本产品合同；
  否则实现经过 golden/conformance 测试的最小 typed subset。

## 九、错误与隔离模型

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

## 十、公共黑盒与自反馈

新增 `tests/mcp_smoke.ps1`，只驱动发布制品和公开接口。

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

## 十一、交付与文档

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

README 只增加：

1. 一句只读 MCP 定位；
2. 一个通用 stdio client 配置片段；
3. 一个五分钟资源读取与 wait 例子；
4. 明确写出“无控制工具、默认无 pane text”。

详细 URI、schema、错误、预算和未来角色留在 PRD/协议发现输出中。

## 十二、依赖图与并行实施

```text
波次 0：串行冻结合同
  protocol revision
  method/resource/tool catalog
  URI/schema/error/budget
  public demo and negative space
          |
          v
波次 1：可并行
  A. mcp_protocol + golden tests
  B. public IPC adapter + resource mapping
  C. wait/cancel core + race tests
  D. build manifest + qualification declarations
  E. black-box fixture + privacy/orphan harness
          |
          v
波次 2：串行集成
  agenterm-mcp.exe entry
  stdio loop + adapter + wait
  protocol-info/capability alignment
          |
          v
波次 3：并行验证
  lifecycle/conformance
  resource same-source
  wait/restart/gap/cancel
  crash/privacy/load
          |
          v
波次 4：串行候选
  full check
  clean qualification
  byte-identical package
  non-publishing release rehearsal
```

并行所有权：

| 分支 | 首选文件所有权 | 不应同时修改 |
|---|---|---|
| Protocol | `mcp_protocol.rs`, protocol unit fixtures | Win32 state machine |
| Catalog | `mcp_catalog.rs`, capability fixtures | runtime adapter |
| Adapter | `mcp_adapter.rs`, public IPC contracts | stdio parser |
| Wait | wait/cancel module与 race fixtures | resource schemas |
| Delivery | build/qualification manifests | protocol semantics |
| Black-box | `tests/mcp_smoke.ps1`, harness helpers | production internals |

`Cargo.toml`、`src/lib.rs`、PRD alignment、artifact manifest 和最终 binary
entry 是集成热点，只允许一个串行 owner 收口。

## 十三、验收门

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

## 十四、主要风险

| 风险 | 早期信号 | 应对 |
|---|---|---|
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

## 十五、第一次评审建议结论

建议直接接受以下默认决策：

1. 主题：**可验证的只读 Agent 桥梁**。
2. 新二进制：`agenterm-mcp.exe`。
3. transport：只做 stdio。
4. stable protocol revision：`2025-11-25`。
5. resources：instances、workspace、tabs、fleet snapshot。
6. tools：只做 `agenterm_wait`。
7. pane text：默认不提供，本版完全延后。
8. server 选择：复用 CLI explicit/zero/one/many 规则，不自动启动。
9. GUI：不增加 MCP 控件和状态动画。
10. control tools、subscriptions、tasks、HTTP、MCP client、Rhai execution、
    brain/flow、agent 权限全部延后。

仍需在实现波次 0 用 spike 决定：

- 使用外部 Rust MCP implementation 还是自有最小 typed subset；
- 最终 frame/resource/concurrency 预算；
- resource envelope 是复用一个通用 schema，还是每个 resource 有独立
  schema ID；
- `agenterm_wait` 的首发 event kind allowlist；
- 是否把一个真实第三方 MCP host 的手工兼容验证作为 release evidence，
  还是仅作为非阻塞互操作报告。

## 十六、建议第一刀

```text
第一提交
  offline mcp catalog
  protocol revision + methods + resources + tool + budgets
  capabilities --json
  golden schema tests

第二提交
  bounded stdio JSON-RPC
  initialize / initialized / ping
  malformed / oversize / EOF tests

第三提交
  instances / workspace / tabs / fleet snapshot resources
  与 CLI 同源对比

第四提交
  agenterm_wait
  cancel / timeout / restart / gap / target closed

第五提交
  crash / privacy / load / orphan qualification
  artifact / SBOM / README / PRD alignment
```

这条切法能够在不引入自主控制、不扩大 GUI、不开放网络 listener 的前提
下，让 AgenTerm 第一次成为任何兼容 MCP client 都能稳定观察的 Fleet。
