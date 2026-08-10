# dynacore：把 dynamic-core 研究收成一个真产品

| 字段 | 值 |
|------|-----|
| **日期** | 2026-08-09 |
| **状态** | 已实现并接产品（见 §7+ 之后的实现记录/commit 历史）；crate = `agenterm-dynacore`，
  **这个名字从 2026-08-09 起永久确定，不再计划改名**（曾有过"该让位给真身 nativecore"
  的判断，已被用户明确推翻——反复的命名混淆本身消耗的沟通代价，被判定超过改名收益）。
  「dynacore」这个名字往后**唯一**指这个 crate，后续投入（含测试套件加固）都在这里 |
| **相关但已归档** | [`archive/design-dynacore-native-core.md`](archive/design-dynacore-native-core.md)——
  另一个 crate（`agenterm-nativecore`，原生 Win32 调用，不靠编译器不靠可执行内存），
  功能完整、38 测试全绿，但已归档、不再投入，也不再是"dynacore"这个名字的候选。
  两个 crate 是平行关系，不互相依赖，IR/Op 定义不共享，只共享少量 intent 无关的机制
  （见该文档 §4） |
| **前置** | [`research/dynamic-core/SYNTHESIS.md`](../research/dynamic-core/SYNTHESIS.md)（Q0–Q23，已归档） |
| **产品归属** | 兑现 [`PRD_02_10_rhai_scripting.md`](../prd/PRD_02_10_rhai_scripting.md) 「Layered deployment」条目 |
| **决策人** | 本文件是产品范围决策，由本轮对话的负责研究员直接定稿，不是又一轮实验 |

---

## 0. 一句话

**把 22 个实验里已经验证过、且对 agenterm 真正有用的那一半，收成一个能被 agent 热加载、免重建、可验证执行的能力包机制。** 不需要的那一半（生码执行、跨 ISA codegen、任意原生 OS 调用）明确不做，理由见 §2。

---

## 1. 为什么现在能做产品决策，不是又一轮研究

研究阶段的目标是"agent 产出的逻辑要能在任意机器上正确跑起来"——这句话的完整版本
需要够到**任意**原生 OS API（`CreateProcessA`、`fork`……），这也是 22 个实验里大部分
硬骨头（R1 编排、R2 跨 ISA 重构、R6 形状边界、R8 等价验证）的来源。

**但 agenterm 产品不需要这个完整版本。** agenterm 已经有一套稳定、typed、versioned 的
宿主调用面——`fleet.*` 操作目录（`src/operations.rs::OPERATION_CATALOG`，77 个操作，
`rh`/`lua`/`qjs` 三个脚本引擎已经在用同一个 `fleet_call(operation_id, params_json)` 绑定
形状，见 `src/script_rh_host.rs`/`script_lua_host.rs`/`script_qjs_host.rs`）。

**一个 agenterm 的能力包不需要够到 `CreateProcessA`，只需要够到 `fleet.tab.close`。**

这一条把研究里最难的部分（OS 接口内容那条永久缝——L1–L5、R1、R2、R6 的大半）
**直接排除在 v1 范围之外**，不是因为解不开，是因为**产品不需要它**。
v1 要的东西，22 个实验里已经全部验证过：

| v1 需要什么 | 对应哪个 Q | 状态 |
|---|---|---|
| 一份可以装下未知逻辑的中立 IR | Q1 | 有边界的中立；边界正是 OS 接口内容——**v1 不碰这条边界，因为不需要** |
| 不需要生成机器码就能执行 IR | Q9 | 解释是地板，ISA 无关，硬化平台结构免疫（Q12） |
| 执行前验证 IR 没有格式错误 | Q19 | 98 行 / 634B 构造门，产出时、无需执行 |
| 单次调用（这里是 `fleet_call`）表驱动，不必每加一个操作就改代码 | Q7 | +1 intent = 0 编组器代码 |
| 能力包可以去重、可以多版本并存、不需要中央注册表 | Q3 + Q18 | 内容寻址 + 构建时钉死发现 |
| 组装成一个自洽系统、真的跑通 | Q22 | 219,136 B，四个真实载荷跑通，一条真实接缝洞（F1）已知且已记录 |

---

## 2. 明确不做（v1 边界，写清楚不是漏做）

- **不做 codegen/JIT 后端。** 解释是唯一执行路径。理由：Q2/Q10 量出降级器 ≈ 整个内核大小，
  Q8/Q12 量出 codegen 在硬化平台（ACG）上被结构性挡死，解释器不需要这些代价。
  产品侧的能力包大概率是 agent 生成的中小规模逻辑（工作流片段、条件路由、批量操作），
  不是计算密集内循环——Q9 的 77× 代价只咬计算密集场景，v1 的目标场景是 OS/fleet 密集，
  代价是 1.0×。
- **不做任意原生 OS 调用面。** 能力包只能调 `fleet.*`。这不是能力削弱——`fleet.*` 本身
  就是 agenterm 全部产品能力的入口（77 个操作，覆盖终端/tab/composer/settings/事件……），
  能力包要做的事情，都能通过它做到。真要扩展 `fleet.*` 本身的覆盖面，走 `OPERATION_CATALOG`
  加条目那条路（P-catalog 系列已经验证过这个机制），不是给能力包开后门直连 OS。
- **不做跨 ISA。** 能力包和内核一样，每个 ISA 一份（Q5 已证明这个模型成本有界）。
  agenterm 目前的发布矩阵是 x86_64（Windows 主战场，aarch64/其它 ISA 是未来 GUI 移植的事，
  不是这个设计要解决的）。
- **不做能力包之间的运行时发现服务。** 名字到哈希的绑定在**打包时**钉死（Q18 的结论：
  发现是构建时问题），加载方（Control Center / 未来的超控智能体）负责决定装哪个哈希。

**这条边界本身遵守 AGENTS.md 的不变量**：`fleet.*` 无权限分层、无能力拒绝——
能力包能调用的操作集合就是任何脚本/CLI/GUI 今天已经能调用的操作集合，**没有新增限制**，
也**没有新增越权**。良构验证（Q19）挡的是格式错误的 IR，不是挡"哪些操作允许被调用"——
那条线仍然完全交给未来的 Agent harness，跟 Rhai/rh 现在的立场一致。

---

## 3. 产品形状

### 3.1 这不是第四个脚本引擎

`rh`/`lua`/`qjs` 是**人或 agent 写源码、走 CLI 跑任务**的东西——它们的产品位置是
"给一个任务写脚本"。`dynacore` 的产品位置不同：**它是让 agenterm 在不重建二进制的前提下
获得新能力的机制**——一份 typed、可验证的 IR 制品（"能力包"，logic pack），
在运行的 agenterm 进程里被加载、验证、解释执行，效果等价于给 `fleet.*` 目录旁边挂一段
新逻辑。消费者不是"运行一次任务的人"，是**运行中的 agenterm 自己**，或者操作它的
超控智能体（`plan/design-cc-hyper-control-agent.md` 里设想的那个）。

> **v1 明确不做签名/来源认证。** §3.2 组件表原稿在 pack 清单里写了"签名"，
> 与 §5「谁产出能力包」把信任链列为独立未决问题自相矛盾——**已改正，以 §5 为准**。
> 内容寻址给的是**完整性**（`store.get` 会重算哈希拒绝篡改/损坏内容，见 `store.rs`），
> **不是真实性**（这份内容是谁产的、该不该信）。v1 的信任边界就是"谁能把 pack 放进
> 加载方读取的 store 目录"这件事本身，跟 rh/lua/qjs 脚本文件今天的信任边界完全一样
> （谁能把 `.rh`/`.lua`/`.js` 放进磁盘）——**不是新洞，是复用既有的那个洞**。
> 签名/供应链认证是**信任链设计**的题目，属于 §5 未决问题，不在这份文档锁死。

这解决了 `PRD_02_10` 里"Layered deployment"条目一直悬着的问题：**Base runtime 稳定不常变，
Application layer（能力包）可以独立发布、独立更新，不用重新发一次 `agenterm.exe`。**

### 3.2 组件（对照 `research/dynamic-core/assembled/`，全部真机验证过）

```
crates/agenterm-dynacore/          机制 crate，无产品名（同 agenterm-platform 的定位）
├─ ir.rs        中立 IR 定义（Q1 的类型化三地址 IR，裁掉 OS-content 相关的 intent 词表——
│                不需要了，只留 fleet_call 一种"调用宿主"的原语）
├─ verify.rs    良构验证门（移植 Q19，加 Q22 发现的 F1 修复：验证要覆盖 fleet_call 的
│                arity/参数 schema，不能只验 IR 内部一致性）
├─ eval_core.rs 解释器（移植 Q9）
├─ store.rs     内容寻址 pack 存储（移植 Q3/Q18，构建时钉死 hash，无运行时发现服务）
└─ pack.rs      pack 清单格式（schema 版本、内容哈希、fleet-操作依赖清单——供加载方审计；
                 **不含签名/目标 ISA**，见上方 v1 范围说明）

src/script_dynacore_host.rs        宿主绑定（对齐 script_rh_host.rs 的形状：一个
                                     fleet_call(operation_id, params_json) 桥接，
                                     不是新发明，是复用三引擎已经验证过的同一个绑定）
```

### 3.3 与三引擎的关系

`fleet_call` 桥接的形状（operation_id + JSON params → JSON result）三引擎已经用了很久，
是**验证过的稳定契约**。`dynacore` 复用它，不是重新设计一个。差异只在"逻辑从哪来"：
`rh`/`lua`/`qjs` 是脚本源码经过引擎自己的执行路径调用它；`dynacore` 是一份预先产出的
中立 IR，经解释器调用它。**四条路径共享同一个宿主绑定形状，不是四套接口。**

---

## 4. 验收标准（v1）

1. `agenterm-dynacore` crate 存在，`cargo check --workspace` 干净，未挂进任何禁止的
   跨平台边界（`crates/agenterm-platform` 之外不得出现原生 marker，见 `ARCHITECTURE.md` §6.1）。
2. 一份 pack（内容寻址、构建时钉哈希）能被加载、验证、解释执行，真实调用至少一个
   `fleet.*` 操作并观察到效果（例如 `fleet.tabs.list`）。
3. 故意构造的坏 pack（arity 不对、externid 越界）在执行前被拒绝，不 panic、不产生
   未定义行为——这是 Q22 F1 教训的直接验收项。
4. 两个不同 hash 的 pack 可以同时被加载、互不影响（Q3/Q18 的版本并存性质，产品化后
   仍要保持）。
5. 每条验收都要有黑盒测试，形状对齐 `tests/rhai_migration.rs` 一类现有黑盒套件的纪律
   （公共命令改动要同步 PRD/计数串，见 `AGENTS.md`/`ARCHITECTURE.md` 的既有规则）。

---

## 5. 未决问题（记录，不阻塞 v1，等 v1 跑通再看要不要开）

- **谁产出能力包？** 研究阶段没有回答"IR 从哪来"——v1 假设 IR 由外部工具/未来的
  dynacore 编译器产出，加载方负责信任链。**这是下一个设计文档的题目，不是这份的。**
  最直接的候选：让未来的超控智能体本身成为 IR 的产出方（agent 观察需求、生成 IR、
  经良构验证后热加载），这和北极星"agent 自主反馈式自进化"直接对齐，但需要独立设计
  信任与审计模型，不在此设计范围内。
- **要不要一个 `agenterm-dynacore` CLI（对齐 rh/lua/qjs 的 `check`/`eval`）？** 倾向要，
  用于开发期验证一份 pack，但不是 v1 的核心——核心是**进程内加载**这条路径能不能跑通。
- **能力包如何声明它依赖哪些 `fleet.*` 操作？** `pack.rs` 的清单里预留了字段，
  具体 schema 留到实现阶段定，不在此设计文档里锁死细节。

---

## 6. Capability boundaries, measured

一轮针对性探测（不是"再加覆盖率"，是**去找机制真正停止工作或变得不实用的地方**），
四个问题都用真实构造的 `Module` 跑过真实管线（`verify()` → `eval_core::run()`）取得证据，
证据全部落在 `crates/agenterm-dynacore/tests/capability_boundaries.rs`（24 个新测试，
`cargo test -p agenterm-dynacore --tests`：45 → 69）。诚实声明先摆在最前面：

**诚实声明**——下面四条里，Q1 和 Q4 是"真实、值得知道的硬边界"（不是缺陷，是设计选择的
直接后果，本节只是把后果量化）；Q2 的结论是"边界比想象中更宽松，且形状和直觉不同"
（`DEFAULT_MAX_STEPS` 几乎不实际约束真实编排包）；Q3 探测出一个真实但目前无害的 bug，
已在本节对应的 commit 里修复；Q3 里其余探测结果都是"符合预期、按设计工作"，明确写出来
是为了不留"没测过所以不知道"的空白，不是暗示还有别的问题。

### 6.1 Q1 — `params_json` 表达力上限：确认是一条硬的、全域的边界

**由构造证明的编译期不可能性**：`Builder::fleet_call(&mut self, operation_id: impl
Into<String>, params_json: impl Into<String>) -> Val`（`ir.rs`）——`params_json` 的类型约束
是 `impl Into<String>`，没有任何重载、没有姐妹方法能接受一个 `Val`。`ExternDecl { operation_id:
String, params_json: String }`（`Module::externs`）本身也是纯数据，不引用 `Val`。
`Inst::FleetCall(Val, u32)` 的第二个字段是一个**在 pack 构建期（`Builder::decl` 的返回值）
钉死的 `u32` extern 表索引**，不是解释器从 `vals[]` 读出来的值——所以连"调用哪个 extern"
本身都不是运行时可寻址的。这不是需要探测的运行时行为，是签名层面就排除的可能性。

**真实可行的变通方案（已构建并跑通）**：`tests/capability_boundaries.rs` 的
`small_fixed_workflow_needs_one_extern_per_distinct_runtime_target`——为"关闭 tab 0..4"
构建 5 个字面不同的 `params_json`（Rust `for` 循环在**构建期**生成 5 个不同字符串），
验证 `module.externs.len() == 5`（`Builder::decl` 的按值去重不会把它们合并，因为字符串
真的不同），并通过真实管线跑通，5 次调用按声明顺序精确到达 bridge。
`dispatching_among_pre_declared_externs_by_a_computed_value_costs_one_explicit_branch_per_case`
进一步证明：即使要"按运行时算出的值选择调用哪个预声明的 extern"，也只能靠一条显式
`BrCond` 分支链（O(N) 个二路分支，结构上等价于 if/elif 梯子），没有任何"按索引跳转/调用"
的捷径。

**真实成本**：一个想要 N 个不同运行时观测值的小工作流，需要 N 个 extern——这些字符串必须
在**pack 构建时**（不是 pack 运行时）已知。这对"N 是个已知的小整数范围"（如"关闭 tab
0 到 4"）是可行但笨拙的（N 个 extern，一条 N 分支的判定链）；对"N 在 pack 运行时才知道"
（例如"tabs.list 返回了多少个 tab，就关闭多少个"）是**彻底不可行**的——不是"笨拙但能做"，
是根本做不到，原因见 §6.4（Q4）：`tabs.list` 返回的实际内容永远进不了 `Val` 空间，
所以连"要枚举多少个 extern"这件事本身，pack 都无法在运行时知道。

**结论**：这是一条硬的、全域的边界，不是可以绕过的不便。"一个 distinct params 字面量
一个 extern"精确成立，且这条边界和 Q4 的边界是同一个根因的两个可观测面
（`FleetCall` 的 dest 只有布尔值，既不能读到返回内容，也不能拿它去合成新的 `params_json`）。

### 6.2 Q2 — step-limit 真实余量：比直觉宽松得多，形状也和直觉不同

真实机器（本次测量环境）上量出的数字：

| 场景 | 结果 |
|---|---|
| 纯控制流自旋（无 FleetCall）跑到 `DEFAULT_MAX_STEPS`（1,000,000 次 block dispatch） | **47.44ms** 触发 `StepLimitExceeded`（`default_step_limit_wall_clock_on_a_pure_control_flow_spin`） |
| 每次循环迭代打一次真实 FleetCall 的循环，跑到 `DEFAULT_MAX_STEPS` | **818.16ms** 内完成 **1,000,000** 次真实 FleetCall（`fleetcall_per_iteration_loop_makes_one_million_real_calls_before_the_default_limit`） |
| 单个直线 block 塞 200,000 条 FleetCall 指令（无回边） | 构建 **224.85ms**，`run_with_step_limit(vm, bridge, 1)`（预算=1，能给的最小非零预算）执行 **167.20ms**，200,000 次调用全部完成，`Termination::Exited`，从未触及 `StepLimitExceeded`（`a_single_straight_line_block_with_many_fleetcalls_is_not_bounded_by_the_step_counter`） |

前两行确认设计文档 `eval_core.rs` 自己的推理（"sub-second on real hardware"）——不是猜测，
是量出来的：47ms 和 818ms 都远在秒级以下。

第三行是**这轮探测的真实发现**：step 计数器按"block dispatch 次数"计（每跳一次
`Br`/`BrCond` 才计一次），**不按指令数计**。一个没有回边的直线 block，无论塞进多少条
`FleetCall` 指令，只 dispatch 一次——`max_steps == 1`（能给的最小非零预算）都能让 200,000
次调用完整跑完。也就是说，`DEFAULT_MAX_STEPS` **只约束真正带回边的循环**，对纯顺序编排
（设计文档 §2 说的"工作流片段、条件路由、批量操作"，dynacore 的目标形状）**基本不设防**——
这类 pack 的真实体量上限是构建期内存/时间（`Val`/block 目标/extern id 都是 `u32`，量级足够大）
和 pack 作者的耐心，不是这个 host 侧步数上限。

**这条不是缺陷，是"边界比想象中宽松"的正面发现**——但也说明 `DEFAULT_MAX_STEPS` 这个安全网
真正防的场景很窄（只有"写错了的循环"），如果未来有 pack 形状是"没有回边但极度指令稠密"，
这个安全网目前对它完全不生效；这是否需要额外一层（比如指令数上限）不在本节结论范围内
（见 §6.5 non-goals）。

### 6.3 Q3 — 对抗性 schema 校验：一个真实 bug（已修复），其余符合预期

逐项真实探测结果（`tests/capability_boundaries.rs::q3_adversarial_schema`，13 个测试）：

| 探测 | 结果 | 分类 |
|---|---|---|
| 嵌套 JSON object/array 作为 `"string"` 类型参数的值 | 正确拒绝（JSON 类型不匹配） | 符合预期 |
| `value_type` 声明为这份 schema 没有 case 的类型（如 `"object"`）| **接受任意 JSON 形状**（落进 `json_type_matches` 的 `_ => true` 兜底） | 符合预期（`json_type_matches` 自己的文档明确写了这个取舍）；`src/operations.rs` 审计过，目前产品 catalog 没有任何参数声明 `"object"`/`"array"`，是休眠状态，不是被利用的洞 |
| JSON object 重复键（如 `{"n":1,"n":999}`）| 确认 **后一个键值获胜**（两个方向都测过，排除"前者获胜"或"拒绝重复键"两种可能）| 符合预期（`serde_json` 标准语义），文档化下来而非放任猜测 |
| 超过 `u64` 范围的巨大整数字面量 | `serde_json` 无 `arbitrary_precision` 特性时溢出会退化为 `f64` 近似值，`as_u64()` 变 `None`，因此在 `"uint32"` 类型检查处被正确拒绝 | 符合预期 |
| 负整数对 `"uint32"` | 正确拒绝 | 符合预期 |
| `"number"` 类型参数的 `minimum`/`maximum` 遇到**浮点数写法**的值 | **发现真实 bug（已修复）**——见下 | **bug，已修复** |
| 字符串参数里的 Unicode/控制字符（emoji 代理对、`\0`、RTL override、多字节 UTF-8）| 全部作为纯字符串通过类型校验，内容不做任何审查 | 符合预期（内容审查不是 schema 校验的职责，和 `rh`/`lua`/`qjs` 脚本字符串现状一致） |
| 空/退化 `params_json`（`""`、`"   "`、`"null"`、`{}` + 尾随垃圾）| 全部正确拒绝为"不是合法 JSON"或"不是 JSON 对象"；`"{}"` 在操作无声明参数时正确接受 | 符合预期 |

**发现的 bug（本节对应 commit 已修复，`verify.rs::check_param_value`）**：数值边界检查原来用
`value.as_i64()` 判断"这是不是一个数"，但 `serde_json` 对任何带小数点/指数的 JSON 数字字面量
（哪怕数值上是整数，比如 `100.0`）都归类为 `Number::Float`，`as_i64()` 对这种值恒返回
`None`——于是 `"number"` 类型参数只要值写成浮点数形式，`minimum`/`maximum` 边界检查就被
**静默跳过**，`99999.9`（远超声明的上限 100）能通过校验。修复为统一用 `as_f64()`（对整数
和浮点数两种 JSON 数字表示都返回值），回归测试
`number_typed_param_bounds_are_enforced_even_when_the_value_is_written_as_a_float`
验证了修复前会通过、修复后正确拒绝的具体值。**真实影响面**：审计 `src/operations.rs` 后
确认，今天产品 catalog 里没有任何 `"number"` 类型参数声明了 `minimum`/`maximum`（`x`/`y`/
`delta_y` 等 `"number"` 参数的 `minimum`/`maximum` 都是 `None`）——和上一轮发现的 `uint32`
问题同一性质："此前没被观测到，不是被利用过"，零行为代价地对已经在范围内的值不产生任何变化。

### 6.4 Q4 — FleetCall 只有布尔返回值：确认是全域边界，探到"能做到多复杂"的真实上限

**grep 确认的全域性**：`eval_core.rs`'s `run_with_step_limit` 里，`Inst::FleetCall` 分支对
`vals[]` 的赋值只有一行——`vals[*d as usize] = u64::from(result.is_ok());`。整个文件里没有
第二处对 `vals[*d]` 的赋值出自 `FleetCall` 分支，`Op` 枚举里没有任何"解析字符串/JSON"的变体，
`Inst` 里没有任何读取 `FleetCallRecord::result` 里 `Ok(String)` 内容的指令。这是可由构造
（穷举 `ir.rs` 全部变体）证明的总边界，不是需要运气才能触发的运行时限制。

**探到的真实上限——两个能跑通的真实编排模式**：

1. **失败回退链**（`fallback_chain_module` + 3 个真实跑通场景）：依次尝试 A、B、C，
   一旦某个成功就立刻返回，不再尝试后续——`fallback_chain_short_circuits_on_first_success`
   证明 A 成功时 B/C **真的从未被调用**（bridge 对它们 panic，测试仍然通过）；
   `fallback_chain_falls_through_to_a_later_success` 证明 A 失败会真的走到 B；
   `fallback_chain_reports_overall_failure_when_every_attempt_fails` 证明三个都失败时
   正确报告整体失败，且三个都被真实尝试过。
2. **有限次重试直到成功**（`bounded_retry_loop_stops_at_first_success_within_k_attempts`）：
   把 Q2 的循环机制和 Q4 的布尔分支组合起来——最多重试 K=5 次，一旦成功立刻停；真实跑通
   两个场景：第 4 次尝试成功时只调用 4 次（不是 5 次），全部失败时耗尽全部 5 次预算后
   报告整体失败。

这两个都是**真实、非平凡、fleet 编排用户会想要的模式**，且完全建立在"只知道上一次调用
成功还是失败"之上——证明这条边界之内能做的事情比"只能设一个开关"丰富得多。

**变得不可能的具体模式（真实跑通后确认，不是抽象推断）**：
`a_fleetcalls_rich_result_content_is_visible_to_the_host_after_the_run_but_never_to_the_packs_own_control_flow`——
用一个真实返回富 JSON 内容（`{"tabs":[{"id":1,"title":"draft: notes"},{"id":2,"title":"final"}]}`）
的 bridge 跑一次调用：`outcome.calls[0].result` 里**host 侧 Rust 调用者能看到完整原文**
（`RunOutcome` 不丢信息），但 `outcome.result()`（pack 自己唯一能感知、能据此分支的东西）
**只能是 0 或 1**，与返回内容毫无关系。具体被挡死的模式：**"关闭标题包含『draft』的所有
tab"**——pack 甚至数不出 `tabs.list` 返回了几个 tab（不只是"读不到标题"，连"数量"这个最基础
的整数都进不了 `Val` 空间），所以连"循环 N 次"这个前提都无法建立，Q1 的"N 个 extern"变通
方案在这里也用不上（N 本身就是不可知的）。

### 6.5 Non-goals（本节明确不做的事）

本节的目的是**刻画现有机制的真实边界**，不是给下一步设计定方向。以下明确不在本节范围内、
也不应该被本节的发现悄悄升级成路线图：

- **不重新设计 IR。** Q1/Q4 揭示的边界（`params_json` 不能读运行时值、`FleetCall` 只有布尔
  返回）是 §2"明确不做"里已经写清楚的产品范围决策的直接后果，不是本节发现的新问题——
  本节只是把后果第一次用真实数字/真实构造量化出来。
- **不提议给 `DEFAULT_MAX_STEPS` 加指令级计数。** §6.2 的发现（直线 block 不受 step 限制约束）
  值得知道，但"要不要为此改变计数粒度"是设计取舍，不在本节结论里。
- **一个值得未来考虑的方向，点到为止**：如果 §5"谁产出能力包"那条未决问题里设想的
  "超控智能体自己产出 IR"成为真实路径，Q1/Q4 这两条边界会直接决定它能编排到多复杂——
  这值得在设计下一版 IR（如果真的要做）时作为已知输入，但**是否要做、怎么做，本节不展开**。

---

*产品设计文档。一旦 v1 落地，回填 `PRD_02_10_rhai_scripting.md` 的「Layered deployment」
条目状态并链回本文件作为设计 SSOT。*
