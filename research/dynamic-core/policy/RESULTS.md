# Q15 RESULTS — 解释器作为 agent 产出代码的策略执行点（Windows/x86_64，实测）

**规格**：[`plan/design-policy-enforcement-experiment.md`](../../../plan/design-policy-enforcement-experiment.md)

Q9/Q12 测的是**可用性**（体积/速度/覆盖/硬化平台可运行），从未测过**安全性**。审稿抓出「解释器更安全」
从未被测量。本实验补这一刀：解释器能否成为 agent 产出代码的**执行时策略控制点**——能拦什么、拦不住什么、
每条指令代价多少、同样的策略 JIT 要付什么。**主判据 ②+④。**

## 判决 — **结构性安全优势存在，但错层：它在「指令级封闭」上真实且便宜，在「OS 危险面」上等于零**

一句话：**「解释更安全」是半对，且错在要紧的那一半。**

- **① 能拦（分类）**：解释器能在**指令层**施加内存越界 / 步数上限 / 分配预算 / OS 调用面 / 数据流（部分）——
  五类负面测试全部被拦下（对照：关掉检查时恶意 IR 全部得逞）。数据流**只是部分**：污点经查表 `hextab[nibble]`
  被洗掉，file→hash→hex 的输出照样写出去。
- **② 拦不住（更要紧）**：**intent 边界之外，解释器控制力为零。** `spawner` 载荷的危险内容
  （`cmd.exe /c exit 7`）**根本不是 IR 值**——rodata=0 字节、SpawnWait extern nargs=0，它活在 IR **之下**的
  seam 里，没有任何 `Val` 可供 taint/allowlist/bounds 去 gate。允许 SpawnWait 后子进程自由运行，解释器**只看到
  退出码 7**，对子进程做的任何事零观测、零约束。**控制点是 intent 那一层的 allow/deny，过了它什么都没有。**
- **③ 代价**：per-instruction 检查全开，pure_compute **1.47×**（vs Q9 纯解释）；解释器 eval-core **1908→2935 B
  (+1027 B / +54%)**。**便宜**——不是「代价过高」那档。
- **④ JIT 对照（正面回答）**：两类 **intent 边界**策略（OS 调用面、内存预算）在**两条路上都是 O(1)** 同一
  chokepoint——**JIT 无劣势**。三类 **per-instruction** 策略（越界、halting、数据流）解释器是 **O(1) 共享代码**
  （它本就有分发循环），JIT 是 **O(ops) emit 出来的 guard 字节**，或一个**装载时验证器**（§4.1 eBPF
  `verifier.c` = 20,065 行，与「几 KB 的核」不可兼得）。

**净判决**：解释路线相对 JIT 有一条**真实的结构性安全优势**，但它**限定在指令级封闭**（内存安全、可终止性、
资源上限）——那一层解释器天然是控制点、免费；JIT 要 emit guard 或跑验证器。**但 agent 代码的实际危险面不在
那一层**——危险在「调出去之后 OS/子进程干什么」（Q1 的 L1–L5），那里**两条路控制点完全相同（intent gate，
O(1) both）、且都控制不到边界之外**。所以对危险面而言，**解释相对 JIT 没有安全优势**。

---

## 度量条件

| 项 | 值 |
|----|----|
| 机器 | Windows Server 2022 Datacenter 10.0.20348（真机） |
| ISA / 目标 | x86_64 / `x86_64-pc-windows-msvc` |
| 编译器 | `rustc 1.97.0 (2d8144b78 2026-07-07)`，`-O`（release） |
| 依赖 | 无外部 crate；仅 `kernel32` FFI。IR/payloads/Q9 解释器 `#[path]` 复用 `../ir/`、`../interp/`（未改一行） |
| 字节测法 | `rustc --emit=obj -O --crate-type=lib` → `llvm-size .text`（与 Q9 同法，同工具链，Q9 core 复算得 **1908 B**，parity 通过） |

## 复跑命令

```sh
cd research/dynamic-core/policy && mkdir -p out
BIN="$(rustc --print sysroot)/lib/rustlib/x86_64-pc-windows-msvc/bin"

# ①②③④ 全部
rustc --edition 2021 -O -A dead_code main.rs -o out/driver.exe
(cd out && ./driver.exe)

# ③ 体积：policy eval-core vs Q9 eval-core（1908 B）
rustc --edition 2021 -O --crate-type=lib --emit=obj -A dead_code measure_core.rs -o out/policy_full.o
rustc --edition 2021 -O --cfg policy_measure_core --crate-type=lib --emit=obj -A dead_code measure_core.rs -o out/policy_core.o
rustc --edition 2021 -O --crate-type=lib --emit=obj -A dead_code ../interp/measure_core.rs -o out/q9_full.o
rustc --edition 2021 -O --cfg interp_measure_core --crate-type=lib --emit=obj -A dead_code ../interp/measure_core.rs -o out/q9_core.o
"$BIN/llvm-size.exe" out/q9_core.o out/policy_core.o out/q9_full.o out/policy_full.o
```

---

## ① 能拦什么 — 分类表（布尔门：每类负面测试必须被拦下）

`driver.exe` J1（对照 = 关检查时恶意 IR 得逞 vs 开检查时被拒）：

| 策略类 | 控制点在哪 | 恶意 IR | 关检查 | 开检查 | 判定 |
|--------|-----------|---------|--------|--------|------|
| **内存越界** | 指令层（每个 Load/Store） | `oob_store`：Alloc(8) 后 Store8@base+64 | `Ok(0)`（静默越界写） | `REFUSED OutOfBounds` | **能** |
| **资源:可终止性** | 指令层（分发计数） | `infinite_loop`：永久自旋 | *不可跑*（永不终止） | `REFUSED StepLimit` | **能** |
| **资源:内存预算** | intent 层（Alloc 处） | `huge_alloc`：Alloc(1 TiB) | `Ok(0)`（调用照发） | `REFUSED AllocLimit` | **能** |
| **OS 调用面** | intent 层（每个 Call） | `spawner`：只调 SpawnWait | `Ok(7)`（子进程运行） | `REFUSED IntentDenied` | **能** |
| **数据流（污点）** | 指令层传播 + intent 层检查 | `echo_file`：读文件→原样 WriteStdout | `Ok(0)`（文件内容泄漏） | `REFUSED TaintedArg` | **能（直接流）** |
| — 数据流的洞 | — | `read_hash_print`：file→hash→hex 经查表 | — | **`Ok(0)`，照样写出** | **只部分** |

- **五类可拦**，负面测试全部在「开检查」时被拒、「关检查」时得逞——证明检查**非空转**。
- **数据流是唯一「部分」**：`echo_file`（污点缓冲直接进 stdout）被拦；但 `read_hash_print` 把 file 派生数据经
  `hextab[nibble]` 查表输出，**污点在查表处丢失**（loaded char 来自未污染的 rodata）——value-taint 追不过 index-flow。
  这是数据流的真实边界，不是实现缺陷。要补它就得做完整信息流分析 = §6 已排除的范围爆炸。

## ② 拦不住什么 — intent 边界之外控制力 = 零（主判据）

`driver.exe` J2（实测 + 结构事实）：

```
spawner IR facts the interpreter can see:
    rodata bytes ............. 0
    SpawnWait extern nargs ... 0
    -> the command 'cmd.exe /c exit 7' is NOT an IR value; it lives in the seam BELOW the IR.
allow SpawnWait -> child ran, interpreter saw ONLY: Ok(7)
```

- **危险内容不是 IR 值。** 要 spawn 什么（`cmd.exe /c exit 7`）**根本没进 IR**——它硬编码在 `do_intent`/`spawn_wait`
  的 seam 里（Q1 的 L3：struct 无中立形，命令是 Win 字面量）。解释器的三样武器（taint / allowlist / bounds）
  **全部作用于 `Val`**，而这里**没有 Val**。allowlist 只能决定「准不准 spawn」，决定不了「spawn 什么/它随后干什么」。
- **调出去即失控。** 允许 SpawnWait 后，`CreateProcessA` 一进 kernel32，子进程自由运行；解释器**只拿回一个退出码**，
  对子进程读写文件、开网络、再 spawn……**零观测零约束**。这正是 Q1 的 L1–L5 所在——**真正危险的地方**。
- **→ 控制点在 intent 那一层，不在指令那一层。** 这是本实验最重要的架构结论，且**与预期一致**（规格 §4 预期
  「没有」）。intent 是**信任边界**：边界内解释器逐指令有控制，边界外为零。

## ③ 代价 — 便宜（斜率 + 截距）

`driver.exe` J3 + `llvm-size`：

| 度量 | Q9 纯解释（基线） | policy 解释（全检查） | 增量 |
|------|--:|--:|--:|
| **速度**：pure_compute（1M 循环，per-instruction 全检查最坏情形） | ~21.3 ms | ~31.2 ms | **1.47×** |
| **体积**：eval-core `.text` | **1908 B** | **2935 B** | **+1027 B / +54%** |
| — 参考：整个解释器 `.text`（含 seam + 台账） | 3177 B | 5429 B | +2252 B |
| — policy 层 LOC（非空非注释，检查+污点+区域+intent 名） | — | ~90（`interp_policy.rs` 242 − eval_op/seam 复用部分） | — |

- **斜率**：per-instruction 检查（步数计数 + 每 Set 的 taint 传播 + Load/Store 的区域查找）让计算热路径慢 **1.47×**。
  叠加 Q9 的 77×（vs 优化原生），策略解释器计算热路径 ≈ **113× vs 优化原生**——但**OS 密集路径这项 1.47× 同样被 OS
  调用淹没**（策略检查次数 ∝ 指令数，OS 密集时指令少）。
- **截距**：+1027 B eval-core。**远不是「代价过高」那档**（判决树节点 3 不触发）。
- **诚实**：pure_compute 无内存 op，区域查找恒 miss（None），但 taint_of 与步计数每条 Set 都跑——1.47× 是**真实开销**，
  不是被无 op 情形压低的假象。

## ④ 与 JIT 的对照 — 正面回答（主判据）

`driver.exe` J4（结构性 + 字节级）：

| 策略类 | 解释器代价 | JIT 代价 | 谁有优势 |
|--------|-----------|----------|---------|
| **内存越界** | Load/Store arm 里 1 个 `if` = **O(1) 共享代码** | 每个 mem op **emit ~10–15 B guard**（cmp+jae→trap）= **O(ops) 字节**，或装载时验证器 | **解释器**（结构性） |
| **资源:可终止性** | 分发循环里 1 个计数器 = **O(1)** | 每 block/回边 emit 计数递增 = **O(blocks) 字节**，或看门狗线程 | **解释器**（结构性） |
| **资源:内存预算** | Alloc 处 1 add+cmp = **O(1)** | Alloc 调用点 1 add+cmp = **O(1)** | **相同**（都 gate 那个 call） |
| **OS 调用面** | Call 处 allowlist 位 = **O(1)** | 已解析调用点 allowlist 位 = **O(1)** | **相同**（都 gate 那个 call） |
| **数据流** | 污点位向量 = O(1) 代码，**部分**覆盖 | 污点寄存器 emit per op = O(ops) 字节，**同样部分**覆盖 | 解释器省字节，覆盖同为部分 |

**关键读法**：
- **两类 intent 边界策略（OS 调用面、内存预算）在两条路上都是 O(1)** ——同一个 chokepoint（那个 Call/Alloc 调用点）。
  **JIT 无劣势。** 而这两类**正是对危险面有效的那类**（决定准不准调出去）。
- **三类 per-instruction 策略（越界、halting、数据流）** 解释器 **O(1) 共享代码**（它本就有分发循环，检查是循环体里
  一个 `if`），JIT 是 **O(ops) emit 出来的 guard 字节**——或者一个**装载时验证器**（rbpf `verifier.rs` 只有 13 KB 是
  因为它**根本不做**内核那套；内核 `verifier.c` = **20,065 行**，比整个核大三个数量级，「要 eBPF 的安全就得不到
  eBPF 的体积」）。**这一层解释器有真实结构性优势。**
- **但 per-instruction 那层不是 agent 代码的危险面**——内存越界/死循环伤的是**自己进程**，不是「agent 拿它去
  spawn/联网/删文件」。真正危险的（②：调出去之后）在 intent 边界，那里两条路等价。

**→ 落判决树**：② 显示 intent 外无控制；④ 显示 JIT 对**危险面相关**策略（intent gate）**同代价 O(1)**
（节点 1：那一层解释无安全优势），但对 **per-instruction** 策略 **O(ops)/验证器**（节点 2：那一层解释有结构优势）。
两个节点都触发，因为它们指向**不同的策略层**——这正是「错层」判决的来源。

## ⑤ 与 Q4 合并 — 信任图（产出时 × 执行时）

Q4 是**产出时**构造门（结构等价守卫）；Q15 是**执行时**控制点。合并到一张按「代码在哪一段」分层的表：

| 代码段 | Q4 产出时（构造门） | Q15 执行时（控制点） | 合并覆盖 |
|--------|--------------------|---------------------|---------|
| **中立核**（算术/内存/控制流） | **Tier A/A′** 结构等价，篡改中立字节 → 拒绝产出 | 指令层可拦越界/halting/预算（O(1)，JIT 要 O(ops)） | **两轴都盖住**——产出时验证等价 + 执行时封闭 |
| **intent 边界**（准不准调 OS） | 部分：intent 区域 schema 可查（Q7），但符号绑定真值是 trust | allow/deny + arg 检查（O(1)，两路同价） | **可 gate，但只是 allow/deny**——决定不了内容 |
| **intent 之外**（L1–L5：调出去后 OS/子进程干什么） | **洞**：0% pure → ~30–56% spawn 结构不可验 | **洞**：控制力为零，危险内容不在 IR 里 | **两轴都是同一个洞** |

**合并结论（收敛且强）**：**产出时结构守卫（Q4）与执行时解释控制点（Q15）都恰好在 intent 边界停下。**
两者叠加把「中立核」两面盖死（Q4 验等价 + Q15 保封闭），把「intent 边界」变成一个 allow/deny 阀门，
但**剩下的洞完全一致**：OS 接口内容（L1–L5）——调出去之后。**这是同一条缝在两根不同的轴（产出/执行）上各自现身，
且都关不上。** 要关它只能封装 OS 调用，而封装被内核禁止（Q1 核心两难：封装不会消失，只会搬进 lowerer/seam 并按
O(targets × intents) regrow）。**信任边界是 intent，在产出与执行两根轴上都是。**

---

## 判决 trace（按规格 §4 的树走）

1. **主判据 ② ∧ ④。**
2. **②**：intent 边界外解释器控制力 = 零（`spawner` 危险内容不是 IR 值；子进程只回退出码）。**成立**。
3. **④**：对**危险面相关**策略（intent gate：OS 调用面、内存预算）JIT **同 O(1) 代价** → **落节点 1**：那一层
   **解释无安全优势**。对 **per-instruction** 策略（越界/halting/数据流）JIT **O(ops)/装载时验证器** → **同时落节点 2**：
   那一层**解释有结构优势**。
4. 两节点指向**不同策略层** → 合并判决：**结构性安全优势存在但错层**（在指令级封闭真实且便宜；在 OS 危险面为零）。
5. **③** 调整「值不值」：1.47× / +1027 B = **便宜**，节点 3（代价过高）**不触发**。
6. kill criterion **未触发**：解释器指令层策略全部拦下（越界/死循环/预算/禁 intent/直接污点）。
7. ⑤ 合并信任图已画（不参与判负）。

**净结论**：对编排者「解释更安全」的暗示——**半对，且错在要紧那半**。解释相对 JIT 的结构性安全优势是**真的**，
但限定在**指令级封闭**（内存安全 / 可终止性 / 资源上限），那一层 agent 伤的是自己进程；而 agent 代码的**实际
危险面**（调出 OS 之后干什么）在 intent 边界及之外，**那里两条路控制点相同、且都控制不到边界外**。
**所以对危险面，解释没有安全优势。** 控制点在 intent 那一层，不在指令那一层——**这是架构上要钉死的一条。**

## 与预期不符 / 推翻预期之处（诚实条款）

1. **推翻「解释更安全」的简单读法**：不是简单真、也不是简单假，而是**分层**——指令层真有优势（便宜的结构性
   优势），危险面（intent 外）零优势。**若只报「有优势」会误导编排者继续往「更安全」上带**——那正是审稿抓的错。
   **如实报：对危险面，解释相对 JIT 没有安全优势。**
2. **数据流的洞比预期大**：预期污点能拦「file→stdout」，实测直接流能拦，但**经查表就洗掉**（`read_hash_print`
   照样写出 hash）。value-taint 追不过 index-flow——这限定了「能阻止某值流向某调用」的答案是**只对直接流成立**。
3. **④ 的 JIT guard 字节是结构性估算，非全 emit**（规格 §4 时间盒允许）：O(1) vs O(ops) 的**形状**由内存 op 计数
   （pure=0 / rhp=4）+ 每 guard ~10–15 B 钉死；未真把 guard 全 emit 出来跑（那是做验证器 = §6 排除项）。
4. **未改预期方向的一处**：② 完全符合规格预期（intent 外无控制）——但它**不是**「解释无用」，而是「控制点定位
   在 intent 层」——这是**正向架构结论**，不是否定结论。
5. **§1.1 病灶未发作**：策略层保持在「一个 `if` + 一个计数器 + 一个 intent 位掩码 + 一个污点位向量」量级，
   没有滑向策略 DSL / 权限模型 / 装载时验证器。eBPF 20k 行是**作为 ④ 的对照被引用**，不是被实现。

## 独立参考值（证明不是自证）

- Q9 eval-core 在本工具链复算 = **1908 B**（与 Q9 RESULTS 报的 1908 B 逐字节相符）→ 证明 policy core 2935 B 的
  delta 1027 B 是同法同链下的真实增量，非度量漂移。
- `read_hash_print` 在 policy 解释器下仍产出 `a49d2cbecc13994f`（Q1/Q9 独立同值）→ 证明加了策略层的解释器**没有
  改变正确执行的结果**，policy 是外挂门而非改语义。
- `spawn`/`echo_file` 的退出码 7 / 文件回显与 Q9 一致 → seam 未动。

---

*研究轨投影（Q15）。不承诺版本归属，不改 PRD 能力状态。*
