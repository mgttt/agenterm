# `agenterm-sql` 的 execute 语义设计：SQL 到底跑在什么之上

| 字段 | 值 |
|------|-----|
| **文档** | `crates/agenterm-sql` 第四脚本后端的 `execute` 设计——占位实现之后的决策文档 |
| 日期 | 2026-08-09 |
| 状态 | 设计稿 rev1（未实现，仅文档） |
| 关联 | `crates/agenterm-sql`（scaffold, commit `d50194fa`）、`src/script_engine.rs`（`ScriptEngineBackend` trait + `SqlEngineBackend`）、`plan/design-script-engine-trait.md` §2.6（原始设计预言）、`plan/plan-v0.1.16.md` §1 Rh SQL-M0 行 |
| 范围声明 | **只读 + 设计文档任务**；本文档不修改任何代码文件 |

---

## 0. 结论先行

**推荐 M1 落地方案 (a)「嵌入引擎」，具体选型 `rusqlite`（`bundled` feature），并把 (c)「host 状态虚拟表」
作为 (a) 之上的 M2 扩展，而不是独立方案。** 三条最强理由：

1. **现有工作负载不是 SQL 形状的查询负载，而是命令式 pipeline** ——`agenterm.tasks.json` 里
   30+ 个任务全部是 `scripts/rh/*.rh`（build、release、candidate-aggregate、supply-chain、
   target-report...），没有一个是"对结构化数据跑查询"的场景（详见 §3 表格前的负载调查）。
   这意味着 SQL 执行目标的第一个真实用户很可能不是"迁移现有任务"，而是"新增一种对 JSON
   manifest/build 报告做 ad-hoc 聚合/过滤"的能力——这恰好是 (a) 嵌入 SQLite/DuckDB 之后把
   JSON 文件当表导入的场景，而不是需要连一个外部数据库。
2. **agenterm 全仓库没有 `tokio`**（`grep -n tokio Cargo.toml`/`Cargo.lock` 零命中）——`execute`
   trait 方法是同步签名（`src/script_engine.rs:111-116`）。DataFusion 系的候选（DataFusion 本身、
   glaredb）是 async-native、构建于 tokio 之上，接入意味着给整个 workspace 引入一个此前不存在
   的异步运行时依赖，且需要在同步 trait 边界里 `block_on`；`rusqlite` 是纯同步 FFI 绑定，零
   impedance。这不是"哪个引擎更强"的问题，是"哪个引擎跟这个 codebase 的并发模型兼容"的问题。
3. **(c) host-state-as-virtual-tables 独立做是一个从零打的 FDW 层（`lib.rs:43-48` 原话），而挂在
   (a) 之上做只是"注册几张 virtual table"**——`rusqlite` 支持 `sqlite3_module`/virtual table
   接口，`src/script_fleet.rs` 已经证明了 fleet 状态可以被平铺成表状结构（`OPERATION_CATALOG`
   的 13 个 operation，见 §2.3）。独立选 (c) 意味着重新发明查询计划/执行器；选 (a)+(c) 混合意味着
   复用一个已经在生产的 SQL 执行器，只多写"这张表怎么取数据"的适配代码。

---

## 1. 问题定义

### 1.1 现有三个引擎的 `execute` 都回答了同一个问题："跑什么"是清楚的

`ScriptEngineBackend::execute` 的统一签名（`src/script_engine.rs:111-116`）：

```rust
fn execute(
    &self,
    source: &str,
    options: &ScriptInvocationOptions,      // project_root / arguments / budgets
    fleet_bridge: Option<ScriptFleetBridgeFn>, // (op_id, params_json) -> Result<String, String>
) -> Result<ScriptInvocationResult, ScriptEngineError>; // { stdout: String, value: Option<Value> }
```

三个已实现的引擎对"跑什么"给出了各自明确的答案：

| 引擎 | entry 概念 | `execute` 具体做什么 | 依据 |
|------|-----------|----------------------|------|
| rh | `fn entry()`，AOT 编译成 native cdylib | `transpile_cdylib_with_mode` 编译整个 pack，要求存在 `fn entry()`，否则报错 `"cdylib pack requires fn entry()"` | `src/script_engine.rs:202-231`；`tests/script_engine_exec_parity.rs:280-284`（引用 `crates/agenterm-rh/src/transpile.rs:267-269`） |
| lua | 无独立 entry 概念，**整个 chunk 就是入口** | 解释执行整个 chunk，返回值（或 `nil`→`0`）widened 成 `serde_json::Value` | `src/script_engine.rs:262-301`；`tests/script_engine_exec_parity.rs:296-313`（lua 无 `entry()` 时"fail-open"到 `0`，是文档化的 divergence，不是 bug） |
| qjs | 顶层 `function entry()` | 加载整个模块，要求存在 `entry()`，调用它，用 `JSON.stringify` 语义把结果转成 `serde_json::Value` | `src/script_engine.rs:336-370`；`crates/agenterm-qjs/src/eval.rs:66-69`（"no top-level `entry()` function"） |

三者共同点：**都是"单一命令式程序，跑到底产出一个标量/结构化值 + 一段 stdout"**——这正是
`ScriptInvocationResult { stdout: String, value: Option<Value> }` 这个 shape 天然对应的模型。

### 1.2 SQL 不满足这个模型的三处 impedance mismatch

1. **没有 entry() 概念**。一个 `.sql` 文件是一批 `;` 分隔的语句（`check.rs:29-32` 已经验证
   `Parser::parse_sql` 把多语句解析成一个 `Vec<Statement>`），不是"一个函数被调用一次"。
   `eval.rs:8-9` 原话："a `.sql` file is not a program with an entry point, it's a batch of
   statements that need *something* to run them against"。lua 的"整个 chunk 是入口"勉强算一个
   先例，但 lua chunk 仍然是单一表达式求值到一个值；SQL 批处理是**多个语句各自可能产生独立结果**，
   语义上更接近"多次调用"而非"一次求值"。
2. **结果集 vs 单一 JSON 值**。`SELECT` 语句产出的是一个二维表（行×列），`INSERT`/`CREATE TABLE`
   产出的是一个"影响行数"或空。`ScriptInvocationResult.value: Option<serde_json::Value>` 这个
   槽位对"多行多列"没有原生对应——需要一个编码决策（本文档 §2.1 采用"JSON 数组，每行一个
   object"），而这个决策在 rh/lua/qjs 里从来不需要做，因为它们的返回值天生就是标量/单个结构。
   `design-script-engine-trait.md:467-468` 已经预言了这一点："`ScriptInvocationResult.value`
   大概率是查询结果集序列化成的 JSON 数组，而不是单个标量"。
3. **副作用模型不同**。rh/lua/qjs 的副作用通道是显式的：`print()`→stdout，`fleet.*`/`__host.fleet_call`
   →host 状态变更（`src/script_fleet.rs`、`crates/... /LuaHostFunctions.fleet_call`）。SQL 的副作用
   通道是隐式的：`INSERT`/`UPDATE`/`DELETE`/`CREATE TABLE` 直接改变查询引擎自己持有的状态，这个
   状态在方案 (a)（嵌入引擎）下是"这次调用私有、调用结束就消失的"，在方案 (b)（外部 DB）下是
   "跨调用持久、可能被其他进程同时修改的"——**副作用的生命周期本身就是三个候选方案分歧最大的地方**，
   不是一个可以先忽略、以后再定的细节。

### 1.3 `fleet_bridge` 参数已经被设计阶段判定为"大概率不用"，且现有 impl 已经这样做了

`design-script-engine-trait.md:465-471` 原话："sql 语句本身没有'调用 fleet.* host 函数'这种脚本
语言概念，`fleet_bridge` 参数对 sql 可能永远是 `None`/未使用"；`SqlEngineBackend::execute` 的当前
签名参数名已经是 `_fleet_bridge`（`src/script_engine.rs:410`），印证了这个判断在接口层面已经生效——
这不是本文档要重新论证的问题，而是要在 §2 里说明：即便如此，"host 状态"仍然可能通过**方案 (c)
虚拟表**（而不是 `fleet_bridge` 回调）进入 SQL 的可见范围，这是两条不同的路径，不要混淆。

---

## 2. 三个候选方案

### 2.1 方案 (a)：嵌入引擎（embedded engine）

**架构草图**：`execute()` 内部为每次调用创建一个**私有、进程内、临时**的数据库实例（内存态或
`tempfile` 临时文件，`agenterm-sql` 已经依赖 `tempfile`——见 `crates/agenterm-sql/Cargo.toml:25`），
把 `source` 的每条语句依次执行到这个私有实例上，最后一条 `SELECT`（如果有）的结果集编码进
`ScriptInvocationResult.value`。

```
execute(source, options, _fleet_bridge)
  -> 打开一个新的 in-memory (或 tempfile) 数据库句柄   // 每次调用全新，不跨调用持久
  -> 用 check.rs 已有的 sqlparser 拆出语句列表（复用，不重新分词）
  -> 依次执行每条语句到该句柄
  -> 取最后一条产出结果集的语句的结果，编码为 JSON 数组（每行一个 object，列名为 key）
  -> stdout：初期可以留空（SQL 没有 print() 概念），或搬运引擎自身的 EXPLAIN/NOTICE 通道（可选）
  -> 返回 ScriptInvocationResult { stdout, value: Some(json_array) }
```

**`SELECT 1;` 的样子**：单语句，`value = Some(json!([{"?column?": 1}]))`（SQLite 风格列名）或
`Some(json!([{"1": 1}]))`，具体列名规则由所选引擎决定，需要在实现阶段固定并测试。

**多语句脚本的样子**（如 `CREATE TABLE t(id INT); INSERT INTO t VALUES (1); SELECT * FROM t;`）：
三条语句依次跑在同一个私有句柄上（`CREATE`/`INSERT` 副作用在这次调用内可见），`value` 只编码
**最后一条** `SELECT` 的结果集——这是一个需要显式决策并写进契约的点，因为"多条 SELECT 混在一个
脚本里，只有最后一条的结果被采用"和 rh/lua/qjs"最后一个表达式的值被返回"是同构的约定，可以直接
借用而不是发明新规则。

**`fleet` 交互的样子**：**没有**——`fleet_bridge` 依旧忽略。方案 (a) 单独实现时，SQL 只能看到它
自己私有数据库里的数据，看不到 agenterm 的运行时状态。这正是 §2.3 要补的那块。

**依赖成本（选型对比，标注需要验证的地方）**：

| 候选 crate | 语言/绑定 | 已知特性 | 相对当前 `agenterm-sql` 依赖集的落差 | 确定性 |
|---|---|---|---|---|
| `rusqlite`（`bundled` feature） | Rust FFI 绑定 SQLite C 源码，`bundled` 把 SQLite amalgamation 编进本 crate 静态链接 | 成熟、长期维护、事实标准；`bundled` 免系统依赖；同步 API | 引入第一个 **C 编译依赖**（`cc` crate 参与构建）——`agenterm-sql` 当前依赖（`sqlparser`/`serde`/`serde_json`/`tempfile`/`walkdir`，见 `crates/agenterm-sql/Cargo.toml:20-25`）全部是纯 Rust，这是一个真实但可接受的落差（`agenterm-lua` 已经在用 `mlua` 的 `vendored` LuaJIT，即 C 编译依赖在本仓库已有先例） | 高（SQLite/rusqlite 的维护活跃度是业界共识）；**具体版本号/编译耗时数字需要在实现阶段用 `cargo build --timings` 验证，本文档不猜数字** |
| `duckdb-rs`（`bundled` feature） | Rust FFI 绑定 DuckDB C++ amalgamation | 面向 OLAP/分析，原生支持读 Parquet/CSV/JSON 文件当表（对 §2.3 虚拟表场景天然友好） | amalgamation 源文件体量远大于 SQLite（历史上是数十 MB 级源码、C++ 而非 C，编译时间显著更长），额外引入 C++ 工具链要求 | **中——需要验证**：DuckDB/duckdb-rs 的当前版本、维护活跃度、Windows MSVC 下的编译可靠性都需要在选型前用真实 `cargo build` 验证，不能仅凭训练知识断言 |
| DataFusion（Apache Arrow） | 纯 Rust | 无 C 依赖；Arrow 生态、列式执行 | **async-native，构建于 tokio 之上**——本仓库全局零 tokio 依赖（`grep tokio Cargo.toml/Cargo.lock` 零命中），引入意味着给整个 workspace 添加一个此前不存在的异步运行时，且要在同步 `execute()` trait 边界里 `block_on`；此外 DataFusion 定位是查询引擎而非通用 RDBMS，`CREATE TABLE`/`INSERT` 等 DML 支持历史上弱于/不同于 SQLite 语义 | 中——DataFusion 本身的活跃度是高确定性的（Arrow 生态头部项目），但"是否适合这个同步、非分析型的宿主"是设计判断，不是维护度问题 |
| glaredb 等更新的纯 Rust 分析引擎 | 纯 Rust | 号称兼顾 DuckDB 的功能面 + Rust 生态 | 成熟度/API 稳定性历史上晚于以上三者 | **低——需要验证**，训练知识对该项目当前状态的置信度不足，不建议在没有实测前作为候选主选项 |

**沙箱/budget 故事**：`ScriptInvocationOptions.budgets: Option<ScriptBudgets>`
（`src/script_protocol.rs:45-64`）已经有 `wall_time_ms`/`output_bytes`/`collection_items`/
`string_bytes` 等字段。方案 (a) 可以把这些映射到：`wall_time_ms`→执行前后打时间戳强制超时（SQLite
本身有 `sqlite3_progress_handler`/busy-timeout 钩子可用）；`output_bytes`/`string_bytes`→编码
结果集为 JSON 后校验长度，超出则截断或报错（和 rh/lua/qjs 现有 budget 执行方式一致，是 host 侧
后处理，不是引擎内置能力）；`collection_items`→限制结果集行数。**这些映射都是可行的，但都需要在
`execute()` 内部手写检查逻辑**——嵌入引擎本身不天然知道 agenterm 的 budget 概念，这点和 rh/lua/qjs
现状一致（它们的 budget 执行也是 host 侧包装，不是引擎原生功能），不是新增复杂度。

**它做不到什么**：看不到 agenterm 运行时状态（fleet/tabs/事件），因为它是一个全新的、空的私有
数据库；调用结束后数据全部丢失（无跨调用持久化，这是"embedded"的定义特征，不是缺陷）；不能被
外部工具直接连接查看。

### 2.2 方案 (b)：连接外部 PostgreSQL 兼容数据库

**架构草图**：`execute()` 从 `options`（或环境变量/配置文件——需要新增一个连接串来源）读取一个
DSN，用 `tokio-postgres`/`postgres`（同步版）建立连接，把 `source` 的语句发给远端执行，取回结果集
编码进 `value`。

```
execute(source, options, _fleet_bridge)
  -> 从某处解析出连接串（新的输入面：环境变量？project_root 下的配置文件？options 新增字段？）
  -> 建立/复用连接（连接池？每次新连？超时策略？）
  -> 依次发送语句，取回结果集
  -> 编码最后结果集为 JSON，返回
```

**`SELECT 1;` 的样子**：需要一个可达的 PostgreSQL 实例先跑起来——这是最大的分歧点：rh/lua/qjs
的 `execute()` 都是**零外部依赖、零网络**的纯计算，方案 (b) 第一次让"能不能执行一段 SQL"依赖
"这台机器/这个 CI runner 能不能连到一个数据库"。

**多语句脚本/`fleet` 交互的样子**：多语句在同一个连接/事务里跑，语义上更接近真实 PostgreSQL 用法；
`fleet` 交互依旧没有——一个外部数据库天然不知道 agenterm 进程内的状态，除非反向搭一个"agenterm
把 fleet 状态写进这个外部库"的同步机制（复杂度远超本文档讨论范围）。

**依赖成本**：`tokio-postgres`（异步，同样撞上"本仓库零 tokio"的问题）或 `postgres`（其同步封装，
基于 `tokio-postgres` 内部仍然拉入部分 tokio 组件，需要在选型时验证同步封装是否真正剥离了异步
运行时依赖——**需要验证**）。

**沙箱/budget 故事**：`wall_time_ms`→连接/查询超时可以映射；但 `collection_items`/`output_bytes`
等本地资源约束对外部库没有约束力——一个恶意/失控的查询可以在远端服务器上跑很久、占用远端资源，
budget 只能限制"agenterm 这一侧等多久/收多少"，不能限制"数据库那一侧算多少"，这是和 (a)/(c)
本质不同的沙箱边界（(a)/(c) 的计算发生在被 `wall_time_ms` 包裹的同一进程里，(b) 的计算发生在
一个 agenterm 管不到的远端进程里）。

**它做不到什么**：不能离线运行（`agenterm.tasks.json` 里绝大多数任务明确标注 `network: []`，见
`agenterm.tasks.json:104-105`/`226-229`/`266-269` 等——离线优先是这个 codebase 现有任务的主流约束，
方案 (b) 天然违反）；引入凭证管理（DSN/密码存哪、怎么在脚本里不明文出现）这个仓库目前完全没有
先例的新问题面；测试环境需要额外起一个 PostgreSQL 容器/进程，`tests/script_engine_exec_parity.rs`
现有的四引擎并列测试模式（纯进程内、无外部服务）会被打破。

### 2.3 方案 (c)：host 状态暴露成虚拟表（host-state-as-virtual-tables）

**架构草图（独立实现，不借用 (a) 的执行器）**：从零写一个最小 SQL 执行层，只认识"表"是
`OPERATION_CATALOG`（`src/operations.rs:525`起）里那些只读 query 类 operation 的结果——例如
`tabs.list`（`src/operations.rs:693`）映射成一张 `tabs` 表，`events.read`
（`src/operations.rs:738`）映射成一张 `events` 表，`protocol.info`/`workspace.info`
（`src/operations.rs:587`/`678`）映射成单行表。

```
execute("SELECT * FROM tabs WHERE ...", options, fleet_bridge)
  -> 解析出 FROM 子句引用的表名（tabs / events / ...）
  -> 表名 -> 固定 operation_id 映射表（tabs -> "tabs.list"，events -> "events.read"，...）
  -> 通过 fleet_bridge(operation_id, params_json) 取一次快照（见 src/script_fleet.rs:16-31 的
     invoke() 模式，但这里换成走 lua/qjs 用的裸 (op_id, json) -> json 闭包，
     即 src/script_engine.rs:74-79 定义的 ScriptFleetBridgeFn，不是 rhai 专用的 FleetContext）
  -> 把取回的 JSON 数组当成内存表，自己实现 WHERE/ORDER BY/LIMIT（或更好：喂给一个真正的执行器，
     见下方"实现路径"）
  -> 编码为 JSON 数组返回
```

**独立实现 (c) 的真实工作量**：需要自己写"JSON 数组 → 可查询的表"这一层——过滤、排序、（如果
要支持）JOIN、聚合函数——这正是 `lib.rs:47-48` 原话说的"最远离已解决问题的选项，需要一整套目前
代码库里完全不存在的 virtual-table/FDW 层"。这不是夸张：`sqlparser`（本 crate 已依赖）只负责
**解析**出 AST，不负责**执行**——把 AST 变成对内存 JSON 数组的实际查询操作，等于从零写一个小型
查询执行器。

**混合实现路径（推荐）**：不独立写执行器，而是把 (c) 的"表"注册成 (a) 选定引擎（`rusqlite`）里的
**virtual table**（SQLite 的 `sqlite3_module` 接口，`rusqlite` 通过 `vtab` feature 暴露）或者更
简单地——每次 `execute()` 开始时，对脚本里引用到的 host 表名，先用 `fleet_bridge` 取一次快照，
`CREATE TABLE`+批量 `INSERT` 灌进本次调用的私有 SQLite 实例，再让 SQLite 自己的执行器跑
`WHERE`/`JOIN`/聚合。后一种做法**不需要**实现 SQLite 的 virtual table C API，只需要"JSON 数组 →
`INSERT` 语句"这一层薄适配，复杂度远低于独立写执行器。

**`SELECT * FROM tabs` 的样子（混合路径）**：`execute()` 用 `sqlparser` 的 AST 扫出脚本引用的表名
是 `tabs`，查表名到 operation_id 的映射表得到 `"tabs.list"`，调
`fleet_bridge("tabs.list", "{}")`（`ScriptFleetBridgeFn` 签名见 `src/script_engine.rs:79`），
拿到的 JSON 数组灌进私有 SQLite 的临时表，再正常执行 `SELECT * FROM tabs ...`。

**能暴露哪些表**：`OPERATION_CATALOG` 里只读、无副作用的 query 类 operation 是天然候选——
`protocol.info`(`operations.rs:587`)、`workspace.info`(`:678`)、`tabs.list`(`:693`)、
`tabs.active`(`:708`)、`events.read`(`:738`)。**有副作用的 operation**
（`workspace.shutdown`(`:1415`)、`server.kill`(`:1400`)、`ui.window.activate` 等，见
`src/script_fleet.rs:167-171`/`212-217` 的 `mutate()` 分支）**不适合映射成表读操作**——SQL 的
`SELECT` 语义是只读、可重复求值的，把一个"关闭 workspace"的副作用包装成"查一张表"是概念错配，
如果要支持写操作，应该走 `INSERT`/`UPDATE` 映射到 `mutate()`-类 operation，这是比只读虚拟表
远得多的复杂度，本文档不建议 M1/M2 涉及。

**它（无论独立实现还是混合实现）做不到什么**：不适合表达"发起一个有副作用的 fleet 操作"（见上）；
只读快照意味着**同一次 execute() 调用内多次查询同一张表看到的是不同时刻的快照**（除非在调用开始
时统一取一次快照——这是一个需要显式决策的一致性模型，混合路径下"进 execute() 时统一灌一次表"
是最简单也最可预测的选择，本文档建议采用）。

---

## 3. 对比表 + 推荐

### 3.1 现有工作负载调查（"三个引擎实际拿来干什么"）

- `agenterm.tasks.json` 的 30+ 个任务（`build`/`check`/`release`/`candidate-aggregate`/
  `supply-chain`/`target-report`/`timing-summary`/`preflight`/... ）**全部**是 `entry:
  "scripts/rh/*.rh"`——命令式构建/发布/CI pipeline 脚本，没有一个是"对结构化数据跑查询"的场景。
- `scripts/lua/`（`build_identity.lua`/`check.lua`/`hello.lua`/`stage-build.lua`/
  `timing-summary.lua` + `lib/fleet.lua`/`lib/build_identity.lua`）同样是命令式任务脚本的镜像子集
  （规模远小于 rh，符合用户记忆里"rh(Lnx)/lua(Win)/qjs(gated)"三引擎路线图的定位）。
- `scripts/qjs/` 目前**只有** `lib/fleet.js`（host binding 库），**没有任何实际任务脚本**——qjs
  是三个已实现引擎里成熟度最低的一个。
- 结论：**没有证据表明现有工作负载里存在"SQL 天然适合表达"的场景**。但仔细看任务名字——
  `candidate-aggregate`（聚合多平台候选清单）、`supply-chain`（生成 SBOM 报告）、`target-report`
  （汇总 cargo target 目录信息）、`timing-summary`（汇总质量计时数据）——这些任务的输入输出都是
  **结构化 JSON 文件的聚合/过滤/汇总**，这正是 SQL（尤其是嵌入引擎 + "JSON 文件当表"能力）擅长
  表达、而当前只能用 rh 手写循环/过滤逻辑实现的场景。这是"SQL 执行目标该服务谁"的关键洞察：**不是
  替换现有 rh 任务，是给未来的"对 manifest/报告类 JSON 数据做 ad-hoc 聚合"场景提供一条更短的路。**

### 3.2 对比表

| 维度 | (a) 嵌入引擎（`rusqlite`） | (b) 外部数据库连接 | (c) host 状态虚拟表（独立实现） | (a)+(c) 混合 |
|---|---|---|---|---|
| 离线可用（对齐 `agenterm.tasks.json` 里 `network: []` 主流约束） | 是 | 否——新增网络/进程依赖 | 是 | 是 |
| 与现有 `execute()` 同步 trait 边界的契合度 | 高（同步 FFI） | 中（需验证同步封装是否真正无 tokio） | 高 | 高 |
| 服务"JSON manifest 聚合"这类真实潜在负载（§3.1） | 高（直接 `INSERT` JSON 行） | 低（数据得先搬进外部库） | 中（只服务 host 状态类数据，不服务任意 JSON 文件） | 高（两种数据源都能喂给同一个执行器） |
| 新增依赖复杂度 | 中（新增 C 编译依赖，本仓库已有 `mlua` vendored 先例） | 高（连接池/凭证/超时/异步封装不确定性） | 低（复用 `sqlparser`，但需要从零写执行器） | 中（同 (a)，外加薄适配层） |
| 实现工作量（M1 可交付的最小切片） | 小——单文件私有 SQLite + `INSERT`/`SELECT` 往返 | 中——连接管理+凭证故事必须先设计好才能写第一行 | 大——执行器从零写，`SELECT 1` 都要自己实现表达式求值 | 小到中——建立在 (a) 之上，M2 阶段再加 |
| 长期可扩展到"host 状态查询" | 需要 (c) 补充 | 需要单独同步机制，复杂度高 | 是（原生目标） | 是（M2 直接扩展） |
| 测试隔离（对齐 `tests/script_engine_exec_parity.rs` 现有的纯进程内四引擎并列模式） | 好——不需要外部服务 | 差——需要起数据库 | 好 | 好 |

### 3.3 推荐

**M1 选 (a)，引擎选 `rusqlite`（`bundled`）；M2 在 (a) 之上叠 (c) 的只读虚拟表子集（混合路径，
不独立写执行器）；(b) 不建议在当前路线图上投入**，理由已在 §0 给出三条最强理由，此处补两条
次要但仍然值得记录的判断：

- **`duckdb-rs` 不是 M1 首选，但值得在 M2/M3 复核**——DuckDB 原生支持"直接 SELECT 一个 JSON/
  Parquet 文件当表"，如果 §3.1 里"聚合 manifest JSON"这类场景成为主要用例，DuckDB 的读文件
  能力可能比"手写 INSERT 灌数据进 SQLite"更省代码。但 DuckDB 的构建成本、Windows 下的可靠性
  在本文档写作时**未经验证**（本仓库当前是 Windows 开发环境，见 env 信息），贸然选它作为 M1
  首选引擎会把"SQL 执行目标"这个本来就悬而未决的问题和"一个未验证的新构建依赖能不能在这台机器
  上顺利编译"这个新的不确定性捆在一起，不划算。`rusqlite` 的成熟度确定性显著更高，先用它把
  execute() 的语义（结果集编码、budget 映射、多语句顺序执行）落地验证，引擎替换是后续可选的
  优化，不是阻塞项。
- **(b) 不是"错"，是"当前不值"**——如果未来 agenterm 长出一个"多个 fleet 实例共享一份持久
  状态"的真实需求（例如跨 CI 机器共享构建历史），(b) 会重新变得有意义；但那是一个独立的、
  尚未出现的需求，不应该驱动"SQL execute 语义"这个当下就该解决的设计问题。

---

## 4. 与 exec-parity 契约的对齐

`tests/script_engine_exec_parity.rs` 顶部文档（`:13-30`）已经明确写出 sql **暂时排除**在
`trivial_entry_value`/`stdout_capture`/`execute_missing_entry_fails_closed`/`error_not_panic`
四个场景之外，理由是"execute 是占位，enroll 进去只会产生假失败"，并给出了排除的替代锚点：
`sql_execute_placeholder_contract`（`tests/script_engine_exec_parity.rs:453-476`）单独钉住"占位
期永远 fail-closed"这一契约。M1 落地之后，这四个场景（以及 `src/script_engine.rs` 里对应的
`#[cfg(test)]` 单测）应当逐个重新评估是否 enroll，而不是自动全部 enroll——理由如下：

| 场景 | sql 是否 enroll | 理由 |
|---|---|---|
| `trivial_entry_value` | **不直接 enroll，需要新场景** | sql 没有"一个 42 程序"的对应物——`SELECT 1;` 的 `value` 是 `Some(json!([{"?column?": 1}]))` 这样的数组，不是标量 `Some(json!(42))`；直接塞进这个测试会破坏它现有的"三引擎标量语义一致"断言，应该新增一个 sql 专属的 `sql_trivial_select_value` 测试，和这个测试并列，而不是混进同一个函数 |
| `stdout_capture` | **不 enroll**（除非 M1 决定给 SQL 加一个 `PRINT`/`NOTICE` 之类的非标准扩展） | SQL 没有 `print()` 概念；`stdout` 字段对 sql 的自然值是空字符串，写一个独立的 `sql_stdout_is_empty` 断言即可，不需要伪造"打印"语义凑齐这个测试 |
| `execute_missing_entry_fails_closed` | **不适用，需要一个语义类似物** | sql 没有 entry() 概念，所以"missing entry"这个问题本身对 sql 不成立；这个场景对 sql 的类似物是"空脚本"或"只有非 SELECT 语句、没有可返回结果集的脚本"该怎么办——建议 M1 决定"空 `value` 用 `None` 而不是报错"（比照 lua 的 fail-open 先例，`tests/script_engine_exec_parity.rs:296-313`），并单独写一个 `sql_execute_no_result_set_is_none_not_error` 测试钉住这个决策，不要默认抄 rh/qjs 的 fail-closed，因为 sql 从设计上就没有"入口点缺失"这个概念可失败 |
| `error_not_panic` | **可以直接 enroll** | 语法错误已经在 `check()` 阶段挡掉（`SqlEngineBackend::check` 早于 `execute` 运行，`src/script_engine.rs:397-404`），但 `execute()` 阶段仍然可能遇到"引擎运行时错误"（例如查询一个不存在的表、除零、类型不匹配）——这类错误必须是 `Err(String)`，不是 panic，这条断言和 rh/lua/qjs 完全同构，可以直接 enroll，不需要新场景 |
| `disabled_backend_errors` | **已经 enroll，继续保持** | 这个场景在 `enabled()` gate 之前就返回，不依赖 execute 是否真的实现（`tests/script_engine_exec_parity.rs:343-374` 已经覆盖四引擎），M1 落地不改变这一层，无需改动 |
| `check_accepts_valid_rejects_broken` | **已经 enroll，继续保持** | 同上，`check()` 是真实现，不受本设计影响 |
| `sql_execute_placeholder_contract` | **M1 落地后删除或改写** | 这个测试断言的是"占位期永远返回 `sql_eval_not_implemented`"——M1 落地后这个契约不再成立，必须删除或改写成"M1 之后 execute 的新契约"，不能留着一个断言"永远失败"的测试和一个"现在会成功"的实现同时存在 |

**entry 契约故事总结**：sql **没有** `entry()`，这一点和 rh/qjs（都要求 entry）不同，反而更接近
lua（"whole-chunk is the entry point"，`src/script_engine.rs` 注释 `:296-301` 一带）。但 sql 和
lua 也不完全一样——lua 的"无 entry 时 fail-open 到 0"是一个**返回值兜底**策略，sql 更自然的
对应策略是"没有产出结果集时 `value = None`"（用 `Option` 表达"没有结果"而不是伪造一个默认值）。
这是一个 **M1 阶段必须显式写进代码注释和测试的决策**，不能隐式套用 lua 或 rh/qjs 任何一方的现成
惯例。

---

## 5. 分期（M1–M3）

### M1（最小可落地，验证方向是否成立）

- 引入 `rusqlite`（`bundled` feature）到 `crates/agenterm-sql/Cargo.toml`。
- `eval.rs::eval_entry` 或一个新的 `execute_entry` 函数：每次调用开一个 `rusqlite::Connection::
  open_in_memory()`，用已有的 `sqlparser` 语句切分（`check.rs` 里 `Parser::parse_sql` 的产出
  可以直接复用其 `Vec<Statement>`，避免重复分词）依次执行每条语句，取最后一条产出结果集的语句
  编码为 `Vec<serde_json::Value>`（每行一个 object）。
- `SqlEngineBackend::execute`（`src/script_engine.rs:406-421`）从"永远转发 `eval_entry` 的错误"
  改为真正调用新的执行路径；`_fleet_bridge` 参数继续忽略（M1 不做 (c)）。
- budget 映射：至少落地 `wall_time_ms`（超时）和 `output_bytes`/`string_bytes`（结果集编码后
  长度校验）两项，其余字段可以先记录"暂不校验"而不是假装校验了。
- 测试：§4 表格里标"可以直接 enroll"或"需要新场景"的每一行，至少落地一个断言；不需要在 M1
  就把所有分支都覆盖到 rh/lua/qjs 的详尽程度。
- **验收标准**：`agenterm-sql eval fixtures/ok.sql`（CLI，`crates/agenterm-sql/src/main.rs:75-77`
  现在的占位分支要跟着从"打印 not-implemented 退出 2"改成真正跑）能对 `SELECT 1;` 返回
  `[{"?column?": 1}]`（或引擎实际给出的列名，需要在实现时钉死具体值），且 `cargo test -p
  agenterm-sql` 和根 workspace 的 `tests/script_engine_exec_parity.rs` 全绿。
- **不做**：(c) 虚拟表、CLI 的 `run`/`pack`/`qualify`/`task` 四个仍然保留占位（`main.rs:75`
  的动词表可以先只把 `eval` 一个动词接上真实现，其余继续 fail-closed，避免一次改动面铺太大）。

### M2（虚拟表：把 M1 的执行器接上 fleet 只读状态）

- 建一张"表名 → operation_id"的静态映射表，覆盖 `OPERATION_CATALOG` 里的只读 query 类
  operation（`protocol.info`/`workspace.info`/`tabs.list`/`tabs.active`/`events.read` 五个，
  见 §2.3）。
- `execute()` 开始时，用 `sqlparser` AST 扫出脚本引用了哪些映射表里的表名，对每个引用到的表名
  调一次 `fleet_bridge`，把取回的 JSON 数组 `INSERT` 进本次调用的私有 SQLite 实例的同名临时表，
  再照常执行脚本剩余部分。
- 决策点：同一次 `execute()` 内的多次查询共享同一份"进入时刻"的快照（不逐语句重新拉取）——
  §2.3 已经论证过这是最简单、最可预测的一致性模型，M2 实现时应当在代码注释里显式记录这个决策，
  不要留成隐式行为。
- **验收标准**：`SELECT * FROM tabs;` 在有真实 fleet 连接（`fleet_bridge` 非 `None`）时返回真实
  tab 列表；`fleet_bridge` 为 `None` 时，引用了映射表名的脚本应该报一个清楚的错误（"host 状态
  表在当前调用上下文不可用"），不是静默返回空表——静默空表会让"没连上 fleet"和"fleet 里确实
  没有 tab"这两种情况无法区分，这是一个需要在 M2 显式测试锁死的边界。
- **不做**：写操作（`INSERT`/`UPDATE` 映射到 `mutate()`-类 operation）——§2.3 已经论证这是概念
  错配更大、复杂度远超只读虚拟表的下一步，不在 M2 范围。

### M3（打磨：CLI 动词表全部接上、budget story 补全、可选的引擎替换评估）

- `main.rs` 里 `run`/`pack`/`qualify`/`task` 四个仍然占位的动词，逐个评估是否需要真正实现（`pack`/
  `qualify` 的语义在 rh/lua/qjs 三引擎里本来就分别有各自的形状，不必强行给 sql 凑一个一样的）。
- budget 的其余字段（`collection_items`/`broker_requests`/`event_items`——这几个尤其和 M2 的虚拟表
  查询相关，"一次查询最多允许发起几次 `fleet_bridge` 调用"是一个 M2 遗留的、值得在 M3 显式补的
  约束点）补齐映射。
- 视 M1/M2 实测的编译时间/结果，评估是否值得把引擎从 `rusqlite` 换成/并行支持 `duckdb-rs`
  （§3.3 已经标注这是"值得复核但不是 M1 首选"）。
- 三个 exec-parity 场景（`trivial_entry_value`/`stdout_capture`/`execute_missing_entry_fails_closed`
  的 sql 类似物，§4 表格中标"需要新场景"的三行）如果 M1/M2 阶段只用独立测试覆盖，M3 阶段应该
  评估是否值得把这些独立测试收敛进 `tests/script_engine_exec_parity.rs` 主文件（同 rh/lua/qjs
  并列展示），提升可读性和"四引擎对比一目了然"的价值，但这是可选的文档/组织性工作，不是功能性
  阻塞项。

---

## 6. 非目标 + 风险

### 非目标

- **不在本文档决定具体的列命名/类型编码规则**（例如 `SELECT 1` 的结果列名到底是 `?column?` 还是
  `1`，NULL 怎么编码进 JSON，日期/时间类型怎么序列化）——这些是 M1 实现阶段需要写代码+测试钉死
  的具体行为，本文档只定方向（"结果集编码成 JSON 数组"），不定字节级细节。
- **不涉及外部数据库连接的凭证管理设计**（方案 (b) 未被推荐，凭证故事无需现在设计）。
- **不涉及写操作（`INSERT`/`UPDATE`/`DELETE`）经虚拟表反向改变 fleet 状态**——§2.3/M2 已经明确
  这是概念错配更大的下一步，本文档不展开设计。
- **不重新评估/修改 `check()` 的 `PostgreSqlDialect` 选择**（`check.rs:9-27` 的现有决策），M1 的
  `execute()` 用 `rusqlite`/SQLite 方言执行，和 `check()` 用 PostgreSQL 方言解析这两者之间存在
  "parse 用一种方言、execute 用另一种引擎的方言"的不一致——这是一个真实的、值得在 M1 实现阶段
  用注释显式记录的已知缺口（"check 说这是合法 PG 语法，execute 未必能在 SQLite 上跑"），但解决
  它（例如切换 `check()` 也用 SQLite 方言，或者做双重校验）超出本文档范围，留给 M1 实现者决定
  是否值得在 M1 就处理还是记录成已知限制推迟。

### 风险

1. **方言不一致风险（见上）**：`check()` 用 PostgreSQL 方言认为合法的语法，`execute()` 用 SQLite
   引擎跑可能报错（例如 PostgreSQL 特有的 `RETURNING`、某些类型转换语法）。M1 落地后第一次出现
   "check 过了、execute 报语法错误"的情况时，这个风险会从理论变成真实用户可见的困惑，需要在错误
   信息里明确说明"这是 execute 引擎（SQLite）的语法限制，不是 check 阶段漏检"。
2. **依赖引入风险**：`rusqlite` 的 `bundled` feature 引入 C 编译步骤，虽然 `agenterm-lua` 已有
   `mlua` vendored LuaJIT 的先例（`crates/agenterm-lua/Cargo.toml:11`），但 M1 实现前仍应该在
   目标平台（本仓库当前是 Windows Server，见 env 信息；同时 `agenterm.tasks.json` 里的任务矩阵
   覆盖 windows/linux/macos 三平台）验证一次干净构建，确认没有平台特定的编译失败。
3. **结果集大小失控风险**：一个 `SELECT` 出百万行的查询会产出一个巨大的 JSON 数组，如果 M1 没有
   认真落地 `output_bytes`/`collection_items` 的 budget 映射（§5 M1 已经把这个列为验收项之一，
   但值得在此重申），`execute()` 可能在编码阶段就把整个进程的内存吃满，这是一个比"查询本身跑
   多久"更容易被忽视的风险面。
4. **M2 虚拟表快照一致性被误解的风险**：如果"同一次 execute() 内看到的是进入时刻的快照，不是
   实时数据"这个决策没有被清楚地写进面向脚本作者的文档/错误信息，用户可能会写出"先查 tabs 数量，
   再操作，再查一次 tabs 数量校验变化"这种在虚拟表模型下**不会按预期工作**的脚本（因为两次查询
   如果落在同一次 `execute()` 调用内会看到同一份快照）——这是一个纯文档/预期管理风险，不是实现
   缺陷，但如果不主动说明，会被用户当成 bug 报告。
