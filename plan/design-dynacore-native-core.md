# dynacore（真身）：不靠编译器、不靠可执行内存的原生调用解释器

| 字段 | 值 |
|------|-----|
| **日期** | 2026-08-09 |
| **状态** | §1–§7 全部已实现并已推送——`agenterm-nativecore` 进了根 workspace，`try_execute_nativecore_pack_invocation` 接进了 `execute_inner`，2 个产品路径真机黑盒测试（真 `spawn_echo` 子进程、真契约拒绝）+ crate 自身 14 个测试全绿。§7.4 的改名清理仍未做，明确保留 |
| **前置** | [`research/dynamic-core/SYNTHESIS.md`](../research/dynamic-core/SYNTHESIS.md)（Q0–Q22）；
  [`research/dynamic-core/assembled/`](../research/dynamic-core/assembled/)（Q22，本设计复活它砍掉的那一半） |
| **纠正** | [`design-dynacore-logic-pack.md`](design-dynacore-logic-pack.md) 描述的东西**不是这个**——
  那个只调 `fleet.*`，是一种二进制形态的脚本，正确名字是「logic pack」，不该叫 dynacore。
  用户明确指出这个混淆（2026-08-09），本文件是修正后的、真正的 dynacore |
| **临时 crate 名** | `agenterm-nativecore`——`agenterm-dynacore` 这个名字现在被 logic pack 那个
  crate 占着，等它改名（后续清理任务，不阻塞本设计）后再把这个 crate 转正改回 `agenterm-dynacore` |

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

### 7.4 命名清理，明确记录、不阻塞本轮

`agenterm-dynacore` 这个名字现在被 logic pack 占着，`agenterm-nativecore` 才是真身。
这是历史遗留、需要修的错误命名，但**这一轮不做**——logic pack 已经在产品主流程里
被多个文件引用（`script_worker.rs`/`script_backend.rs`/`lib.rs`），且这个 checkout
同时有别的会话在改邻近文件，此刻改名风险大于收益。**留档，等两条并发工作都消停
再做一次干净的改名 + 引用替换**。

---

*产品设计文档。命名冲突（`agenterm-dynacore` 暂被占用）待 logic pack 改名后一并清理，
见 §7.4。*
