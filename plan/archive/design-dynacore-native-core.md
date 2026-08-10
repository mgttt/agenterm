# ⚠️ 已归档：native-core 原生调用解释器设计

> **归档于 2026-08-10。** 本设计已经实现且不再主动投入；`dynacore` 的现行产品方向见
> [`../design-dynacore-logic-pack.md`](../design-dynacore-logic-pack.md)，保留实现事实见
> `crates/agenterm-nativecore/README.md`。本文只作历史设计与验收记录。

# dynacore（历史命题）：不靠编译器、不靠可执行内存的原生调用解释器

| 字段 | 值 |
|------|-----|
| **日期** | 2026-08-09 |
| **状态** | **归档（2026-08-09）。** §1–§9 全部已实现并已推送（`agenterm-nativecore` 进了根 workspace、接了产品、有 CLI、有 README、38 个真机测试全绿、含 §9 的签名注册表）。用户最终判断（2026-08-09）：这条crate 的名字（`agenterm-nativecore`）不该继续跟 `agenterm-dynacore` 抢"到底谁是 dynacore"这件事——反复的命名混淆本身造成的沟通损耗，已经超过继续投入的价值。**代码保留、不删、不退（已测试、opt-in、未配置时零成本），但不再作为"dynacore"这个名字的候选，也不再主动投入**。「dynacore」这个名字和后续投入，从这次起归 `crates/agenterm-dynacore`（logic pack）——见 [`design-dynacore-logic-pack.md`](../design-dynacore-logic-pack.md)。本文件保留作历史记录 |
| **前置** | [`research/dynamic-core/SYNTHESIS.md`](../../research/dynamic-core/SYNTHESIS.md)（Q0–Q22）；
  [`research/dynamic-core/assembled/`](../../research/dynamic-core/assembled/)（Q22，本设计复活它砍掉的那一半） |
| **早前的判断，已被"状态"行推翻** | 本文件曾经记录"这才是真身，该拿到 dynacore 这个名字，logic pack
  该改名让位"——这个技术判断没有错，但**执行代价（反复的命名混淆本身消耗的沟通成本）被用户判定
  为不值得继续背**，见上面"状态"行（2026-08-09）。`crates/agenterm-dynacore` 保留原名，
  不再计划把这个 crate 改名过去 |
| **crate 名，不再变** | `agenterm-nativecore`——见"状态"行，这个名字定了，不会再改成 `agenterm-dynacore` |

---

## 0. 初心，重新对齐

**dynacore 要做的是"动态执行二进制码"**——不是"调用宿主已经定义好的操作"，是**够到平台原生
API 表面本身**，且这件事不依赖编译器在场、不依赖真正申请可执行内存去运行生成的机器码。

## 1. 为什么这跟 rh 的 AOT pack 不是同一件事

`agenterm-rh` 从 M31a 起就有热加载、无限制访问 OS 的原生二进制执行——transpile→rustc→
native i64-ABI→dlopen。**如果 dynacore 只是再做一遍这个，它没有存在的理由。**

区别是研究阶段量出来的、真实的两条硬约束（不是猜的）：

| | rh 的 AOT pack | dynacore |
|---|---|---|
| 需要 rustc 在场（或预编译好目标机器的原生产物） | 是 | **否** |
| 需要真正申请可执行内存（RW→RX / RWX）去跑生成的码 | 是 | **否** |
| 在 ACG/iOS 这类硬化平台上 | Q8/Q12 实测：**三条申请可执行内存的路全断**（1655） | Q12 实测：**解释器结构性免疫全部四道关卡**——它从不申请可执行内存 |

dynacore 就是 Q9 那条"解释是地板"结论的正面应用：**不生成机器码，靠解释器直接够到原生 API**。
这是 rh 做不到、而且**在硬化平台上永远做不到**的事——不是"暂时没实现"，是路径本身在那类
平台上不存在。

## 2. v1 范围：复活 Q22 砍掉的那一半，这次带上验证

Q22 装配阶段本来就绑定了七个真实 Win32 API 原生调用（真机验证过）：
`Alloc` / `FileOpen` / `FileRead` / `FileClose` / `WriteStdout` / `SpawnWait` / `FileWrite`。
产品化 logic pack 时（`design-dynacore-logic-pack.md` §2）把这七个连同支撑它们的
`Op::Rodata`/`Inst::Store8`/`StoreW`/`Op::Load8`/`LoadW` 原生内存操作**全部砍了**，
只留 `FleetCall` 一种 intent。

**v1 就做这七个，原样复活**——不扩大范围，Q22 已经验证过它们能真机跑通。
但这次要修正 Q22 装配时留下的两个真洞：

1. **F1 那类"验证器不知道调用契约"的问题，从第一天就补上**——不是等装完了才发现。
   每个原生 intent 的 arity/参数形状要在产出时（`verify()`）就跟这个 intent 自己声明的
   契约核对，不能只验 IR 内部一致性。
2. **`STARTUPINFOA`/`PROCESS_INFORMATION` 这类结构体布局，用 Q13 的"烤了就验"模式**，
   不是裸烤——Q22 当时没有这层，这次要有。命名绑定（符号解析对不对）能不能同样接上
   Q14 的行为式验证，**你判断**，不是必须项，但如果代价便宜就做。

## 3. 硬约束（继承自 Q0–Q22 的全部纪律，不重新论证）

- **五条原语，host-conditional**：内存（RW↔RX）、执行（跳转）、可达（符号解析/系统调用）、
  调用（按签名描述调任意地址）、declare（发布/询问布局事实）。**不加第六条。**
- **只做解释器，不做 codegen/JIT**——这是 v1 存在的理由本身（§1），不是权宜之计。
- **只做 x86_64/Windows**——沿用 Q5 已证明的"N 份小核"模型，不是这次要扩的轴。
- **步数上限从第一天就有**（Q15 机制，logic pack 那边已经证明过怎么移植，直接抄）。
- **内容寻址 + 构建时钉哈希**（Q3/Q18），不做运行时发现。
- **不做任意扩展 API 面**——就是 Q22 验证过的那七个 intent，不多。真要加第八个，
  照着这七个的验证深度加，不能降级验证标准换取覆盖面。

## 4. 与 logic pack 的关系

**两个独立 crate，两套 IR，不共享 `Inst`/`Op` 定义**（各自的 intent 集合语义不同，
硬共享会导致其中一个的验证逻辑意外覆盖到另一个不该覆盖的东西——这正是 F1 教训的
一般化版本：清楚一个验证器到底在为谁的契约负责）。**可以共享**：`eval_core.rs` 的
主循环骨架（Set/Term 那部分跟 intent 无关的通用调度逻辑）、`store.rs`（内容寻址机制
本来就是 intent 无关的）、Q15 步数上限的实现模式。

不共享的判断依据：`FleetCall` 的契约来自运行时查询 `OPERATION_CATALOG`；原生 intent 的
契约是编译期就定死的 API 签名（`CreateFileA` 有几个参数、`STARTUPINFOA` 多少字节，
这些不会因为宿主状态变化）。两者验证的"依据从哪来"根本不同，硬塞进同一个 `verify()`
会两边都不干净。

## 5. 验收标准（v1）

1. `cargo check --workspace` 干净，`agenterm-nativecore` 不进根 workspace 的产品依赖图
   （先独立编译验证，像 Q22 那样，不急着接进 `agenterm` 主 crate——那是下一轮的事，
   本轮先把"不靠编译器、不靠可执行内存、真的够到原生 API"这件事在独立 crate 里做对）
2. 七个原生 intent，真机跑通 `pure_compute`/`read_hash_print`/`spawn_echo`（Q22 用过的
   同三个载荷，语义不变，只是这次连着完整验证链）
3. 故意构造的坏 IR：(a) 结构性错误（沿用 Q19 现有的五类）(b) **原生调用契约错误**
   （比如 `SpawnWait` 参数数量不对）——两类都要在执行前被拒绝，这是 F1 教训的直接验收项
4. 步数上限：一个真实的无限循环 native pack 被及时打断，不挂死宿主线程
5. 每条验收要有真机黑盒测试，不是纸面断言

## 6. 明确不做（v1，防止范围蔓延）

- 不扩大到 Q22 那七个之外的任何新 intent
- 不做跨 ISA（Q5 已经证明模型成立，但这轮只做一份）
- 不做 struct-by-value 超过寄存器宽度的调用（Q20 已经留白，理由沿用）
- 不做运行时发现服务（Q18 已判决是构建时问题）
- 不做跟 `agenterm` 主 crate 的深度集成——这次先把"能独立、正确地动态执行原生调用"
  这件事在 `research/dynamic-core/assembled/` 之外、作为一个真正的产品 crate 立住，
  接入产品主流程是下一轮的事，不要这轮就想两件事一起做

---

## 7. 产品接入（下一轮，本次追加）

§6 说"接入产品主流程是下一轮的事"——现在是这一轮。

### 7.1 接入形状，照抄 logic pack 那次已验证成立的模式

logic pack 的 `src/script_dynacore_pack.rs`/`try_execute_dynacore_pack_invocation`/
`execute_inner` 三段式接入（进程内、env var 触发、`Ok(None)` 原样落空、真机黑盒测试证明
不是子进程）已经审过、验过，是真实可用的模式。nativecore 照这个形状接，**但更简单**——
`crates/agenterm-nativecore` 的公共 API 已经确认：

```rust
pub fn verify(m: &Module) -> Result<VerifiedModule<'_>, IrFault>;   // 不需要 catalog/bridge 参数
pub fn run(vm: &VerifiedModule) -> RunOutcome;                       // 不需要 fleet_bridge 参数
```

**没有 bridge 要穿过去**——nativecore 的 pack 直接调 `seam.rs::do_intent` 落到真实 Win32 API，
不经过 fleet broker。这意味着接入比 logic pack 更薄：不需要处理 `ScriptFleetBridgeFn`/
`DynacoreFleetBridgeFn` 那层类型对齐，`try_execute_nativecore_pack_invocation` 的签名可以
比 `try_execute_dynacore_pack_invocation` 少一个参数。

### 7.2 交付物

- `src/script_nativecore_pack.rs`（新，对齐 `script_dynacore_pack.rs`/`script_rh_pack.rs` 的
  进程内缓存形状）：从 `AGENTERM_NATIVECORE_PACK_STORE`/`AGENTERM_NATIVECORE_PACK_HASH`
  加载、验证、缓存一份 `VerifiedModule`（或等价可运行制品）
- `src/script_backend.rs`：`try_execute_nativecore_pack_invocation`——`Ok(None)` = 没配置
  （原样落空到 rh/lua/qjs/sql/logic-pack 现有链条），`Ok(Some(_))` = 跑完了，`Err` = 验证/
  步数超限失败
- `src/script_worker.rs`：`execute_inner` 里加一段调用（放在哪个位置相对其它几条分支
  不重要，因为触发条件互斥——各自靠不同的 env var，不会同时命中）
- 真机黑盒测试：证明产品路径调用的确实是**真实 Win32 API**（不是 mock），
  至少覆盖 `spawn_echo`（进程真的被创建、真的被等待）与一次故意的验证失败（契约不对的
  IR 在执行前被拒绝，不 panic）

### 7.3 明确不引入新的权限层

nativecore pack 够到的是**跟 rh/lua/qjs 今天已经有的同一份"无限制本地运行时"**——
`AGENTS.md` 早就写死"没有权限分层、没有能力拒绝，Agent 策略归未来的 harness 管，
不归引擎管"。良构验证（`verify()`）是**正确性门**（挡格式错误的 IR），
**不是权限门**（不判断"这个操作允不允许做"）。接入时不要顺手加一层"nativecore 需要
额外授权"的逻辑——那会制造一个跟 rh/lua/qjs 不一致的新姿态，不是这次要做的事。

### 7.4（作废，见文件头"状态"行，2026-08-09）

本节曾计划"改名清理：把 dynacore 这个名字从 logic pack 转正给这个 crate"。
这个计划被推翻了——不是因为技术上做不到，是因为反复的命名混淆本身的沟通代价，
用户判定不值得再背。`agenterm-dynacore` 保留原名，此 crate 永久叫
`agenterm-nativecore`，不会再改名。本节原文保留在 git 历史里，不在这里重复。

## 8. v1 冻结，转研究：Q23

§5/§7 的验收标准全部达成，但产品化到这一步后暴露了一个没被之前任何一问覆盖的
真实缺口：**七个 intent 是编译期写死的**——加第八个，要改 Rust、重新编译、重新
发行 `agenterm.exe`。这跟`[[project_agenterm_self_evolution_north_star]]`（agenterm
要能让大模型自己反馈式自进化）根本对不上——一个"动态执行原生二进制码"的核，如果
扩展它自己的能力面还是得靠人先手动改代码再发版本，它跟 rh 的 AOT pack 在"动态"这
个维度上其实没有本质差别，只是慢、覆盖窄。

**用户判断（2026-08-09）**：当前七个 intent 的实际产品价值薄（§1 的两个差异化点——
不需要编译器、硬化平台免疫——都还没被 agenterm 今天的真实部署场景吃到），继续在
这个方向堆产品化工作不值——**v1 到此冻结**：代码保留（已测试、opt-in、未配置时
零成本，不删不退），不再主动加 intent/扩 CLI/收录 PRD。精力转回研究，而不是继续
产品化。

**Q23（下一问，未派出）**：native intent 能不能在 pack 加载时声明（而不是编译期
写死），同时保住 F1 那类"验证器核对真实调用契约"的强度和 Q13 的 bake-and-detect
布局自检——也就是说，一个 agent 能不能在不重新编译 `agenterm.exe` 的前提下，教会
dynacore 一个它原来不认识的 Win32 API？

这问题目前是开放的，故意不在本文件里预判方向（避免重蹈 Q0 判决树先射箭后画靶的
错——见 SKILL.md 的教训）。候选方向至少有三条，留给 Q23 自己测量，不在这里先定：
1. **签名描述语言**：pack 里带一段结构化的调用签名描述（参数个数/宽度/调用约定），
   `verify()` 在加载时依据这段描述生成契约，而不是依赖 `Intent::contract_arity()`
   这种编译期硬编码的 match——五原语里的"declare"本来就是干这个的，只是 v1 只把它
   用在了 `STARTUPINFOA` 这类布局自检上，没有用在"这个符号本身能不能被安全调用"上。
2. **符号解析层**：加一层 Q14 那种行为式验证，在运行时真正探测一个新符号是否可达、
   签名是否与声明一致，而不是完全信任 pack 作者的声明——这是 declare 原语"发布 vs
   询问"两半里，v1 只做了"发布"（bake-and-detect 检查已知布局），没做"询问"（
   对未知符号做运行时行为验证）。
3. **维持编译期白名单，但把"扩白名单"这件事做便宜**——如果 1/2 两条量出来代价
   太高（比如安全性没法在运行时验证到 F1 级别的确定性），那就承认"能力面扩展
   仍需要人审查+重新编译"，把研究结论写成"为什么这是对的取舍"，而不是硬做一个
   看起来动态、实际不安全的东西。这本身就是一个合法的判决结果，不是失败。

### 8.1 Q23 结果，已测（2026-08-09）

真机测完了，结论是**判决性拆分**，不是三选一，也不是简单的能/不能——
详见 [`research/dynamic-core/runtime-intent/RESULTS.md`](../../research/dynamic-core/runtime-intent/RESULTS.md)
（本节只摘要，不重测；数字口径见该文件）：

- **能扩展，真机验证**：一个 nativecore 七个 intent 里从没出现过的 kernel32 导出
  （`MulDiv`/`lstrlenA`）**纯靠运行时解析的文本 pack + 通用调用 trampoline 跑通，
  零重编译、零 codegen**——机制源码里没有一处 `match symbol`，符号只是 pack 里的
  字符串。方向①成立的那一半是真的。
- **F1 那类"验证器核对真实契约"的强度，能不能保住，答案是"能，但只有一种做法
  对"**：契约必须从 pack 自己的调用 recipe **推导**出来，不能是 pack 作者**另立
  的一个声明字段**——另立声明会原样重开 F1（作者同时握着 IR 和声明，构成单作者
  自证循环，正是编译期 `Intent::contract_arity()` 靠"第二方给出契约"这件事本身
  打断的那个圈）。这是比编译期路径**更严格**的做法：`contract_arity()` 和 seam
  实际访问的参数下标是人手动保持同步的两个常量，推导消掉了"两个常量能不能对上"
  这个问题本身。
- **诚实的负结论（真正的天花板，不是这次实现得不够好）**：一个 pack 把真实
  3 参的 `MulDiv` 谎报成 1 参，contract 从 recipe 推导出来是"1"，跟 IR 内部一致，
  `verify()` **放行**——然后真调用用 1 个参数打真实 3 参 API，读到垃圾寄存器，
  返回错误结果。**单作者的数据 pack 里没有任何东西能告诉 `verify()`"这个符号真实
  的 ABI 是 3 个参数"**——Windows x64 的原生导出不带可查询的签名元数据，这条路
  在结构上走不通,不是这次测得不够细。**更关键的澄清**：编译期的 `contract_arity()`
  其实也从没机器验证过这一半——它就是一个人读了 API 文档手写的常量。**重新编译
  真正买到的东西,是"一个独立第二方（人审 + git）对真实签名的断言",不是一道
  机器能做的 F1 级别检查。**
- **方向②（Q14 行为式探针）在运行时能把"有真实外部契约可比对"的符号的绑定
  错误抓回来**（真机复现：把 `lstrcmpiA` 冒充 `strlen` 绑定，探针跑出 `-1≠5`
  当场 FIRE）,但探针强度封顶于"声明的签名有多准"——**抓得住错绑定，抓不住
  欠声明的 arity**（欠声明的情况根本构造不出一个"已知输入→已知输出"的探针）。
- **指向的具体折中方案（未做，Q23 只是指出来）**：一份**人审一次的签名注册表**
  （不需要给每个新 intent 单独重编译，只需要注册表本身被审过、被签过）——这比
  "每加一个 intent 都要重新编译发行 agenterm.exe"轻，又不丢掉重新编译买到的那个
  "独立第二方断言"。**这是 nativecore 如果要真正配得上自我进化北极星，下一步
  值得做的产品方向**——但本轮不做，Q23 到此为止是研究结论，不是实现任务。

## 9. v2：签名注册表——解冻，理由是 Q23 已经把安全边界量出来了

§8 冻结 v1 是因为"扩能力面"当时是没量过的猜测。Q23 量完了，边界很精确
（§8.1）：**扩展本身没问题，问题只出在"契约的真实性谁来担保"**——单作者 pack
不行，需要一个独立第二方。这不再是猜测，是可以直接设计的东西，所以这里**只
为这一条**解冻,不是全面重开 v1 的范围（新 intent 之外的其它 v1 非目标——跨
ISA/struct-by-value/运行时发现——**继续不做**，见 §6，不受本节影响）。

### 9.1 机制

- **注册表是数据，不是编译进 `agenterm.exe` 里的 Rust match**——一张
  `(符号名, 模块名, 真实 arity, 调用约定)` 的表，人审一次、跟着 crate 一起发布
  （v2 first cut：编译进 crate 常量表即可，是不是要做成运行时可更新的独立文件
  是后续问题，不在本节强制——**先证明机制本身，不先做分发基础设施**）。
- pack 里的新符号调用，contract **不再允许来自 pack 自己声明**——必须在注册表
  里查到才能通过 `verify()`；查不到就拒绝（这正是 S5 那个洞的关闭方式：`verify()`
  不再相信 pack 作者说的话，只相信注册表）。
- 沿用 Q23 的核心发现："derive, not declare"——如果注册表命中，contract 从
  **注册表**推导（不是 pack 自报），跟 Q22 的 `Intent::contract_arity()`
  给固定七个 intent 的方式是同一类保证，只是来源从"编译期 match"换成
  "编译期常量表"——**这次解冻换来的不是"不再需要人审"，是"人审一次可以
  覆盖多个符号，不必每个符号单独走一次完整的 intent 工程改动**（新增一个
  `Intent` 变体今天要改 `ir.rs`/`verify.rs`/`seam.rs`/`step_table.rs` 四处；
  注册表命中的符号只要注册表加一行 + 通用 trampoline，不碰这四个文件）。

### 9.2 验收标准

1. 复现 Q23 的 S1/S2（`MulDiv`/`lstrlenA` 从注册表跑通，零改动这四个核心文件）
   ——但这次是在 `agenterm-nativecore` 真实源码里，不是 `research/` 下的原型
2. 复现 S3/S4（欠参 IR 在注册表命中路径下被 `verify()` 拦下,且必须是**推导**,
   不能让 pack 再带一个平行的 declared arity 字段）
3. 复现 S5 的**修复**：pack 谎报一个注册表里没有的符号的 arity → 因为查不到
   注册表条目直接拒绝，不再是"内部自洽就放行"
4. 一个不在注册表里的符号 → 明确拒绝（不是静默失败,不是 panic），错误信息里
   写清楚"这个符号不在签名注册表里，不能通过这条路调用"，不能让人误以为是别的
   验证失败
5. 七个原有 intent 的行为**完全不变**——注册表是它们之外的第二条路，不是替换
   `Intent::contract_arity()`，两条路径共存，各自的测试互不影响
6. 每条验收都要真机黑盒测试，不是纸面断言，跟这条轨一直以来的纪律一致

### 9.3 明确不做（v2，防止范围蔓延）

- 不做注册表的运行时热更新/远程分发——先证明机制，签名注册表本身怎么审、
  怎么签、怎么分发，是产品化的下一步，不在这轮
- 不做通用 FFI（任意参数类型、任意调用约定）——沿用 Q23 的范围，只做
  0–4 个整数/指针宽度参数，跟 ④ 原语现有的形状边界一致（Q20 已经把这条线
  划过一次，这轮不重新划）
- 不碰 §6 列的其它 v1 非目标（跨 ISA、struct-by-value、运行时发现）
- 不碰任何被并发会话占用的文件——这轮全部工作限定在 `crates/agenterm-nativecore/`
  自己的目录内，不需要接产品主流程（`execute_inner` 那层）才能验收，接不接
  产品是又一轮的事,不跟这轮的验收标准绑在一起

---

*产品设计文档，本文件描述的 crate（`agenterm-nativecore`）已归档（2026-08-09，见文件头
"状态"行）——保留作历史记录，不再是当前投入方向。「dynacore」这个名字和后续投入见
[`../design-dynacore-logic-pack.md`](../design-dynacore-logic-pack.md)。*
