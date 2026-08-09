# dynacore：把 dynamic-core 研究收成一个真产品

| 字段 | 值 |
|------|-----|
| **日期** | 2026-08-09 |
| **状态** | 设计定稿，实现未开工 |
| **前置** | [`research/dynamic-core/SYNTHESIS.md`](../research/dynamic-core/SYNTHESIS.md)（Q0–Q22，22 个判决性实验 + 1 次装配） |
| **产品归属** | 兑现 [`PRD_02_10_rhai_scripting.md`](../prd/PRD_02_10_rhai_scripting.md) 「Layered deployment」条目（迄今 `[ ]` 未开工） |
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

*产品设计文档。一旦 v1 落地，回填 `PRD_02_10_rhai_scripting.md` 的「Layered deployment」
条目状态并链回本文件作为设计 SSOT。*
