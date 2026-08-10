# ⚠️ 已归档：dynamic-core 已判决实验规格

> **归档于 2026-08-10。** Q0–Q23 研究轨已经封闭，综合结论与重新开启条件由
> `research/dynamic-core/SYNTHESIS.md` 和 `research/dynamic-core/README.md` 拥有。
> 本文件只保存该问题的实验前判据与历史结果，不是活跃版本任务。

# Q9 — 解释执行作为一等后端：多大、多慢、覆盖多少、缝还在不在（历史规格）

> ⚠️ **不是 AgenTerm 产品范围。** 动态核研究轨的一条实验（见
> `research/dynamic-core/README.md` 的 Q 索引）。不进任何版本 plan 的 must-ship，
> 不改 `PRD.md` 能力状态。

| 字段 | 值 |
|------|-----|
| **日期** | 2026-08-08 |
| **目的** | Q8 实测把四原语劈成两组：①②（生成新代码）在 ACG/硬化进程/iOS 下**脆弱**，③④（够到已有代码）**稳固**。Q8 的直接后果是「解释执行必须从第一天就是一等退路」。本实验**建一个跑同一份 Q1 中立 IR 的解释器后端**，量它：能否跑通全部三个载荷、体积、速度代价、平台无关度、以及 Q1 的 L1–L5 缝在解释路线下是否原样存在 |
| **实现位置** | `research/dynamic-core/interp/`（**不挂进根 workspace**；`#[path]` 复用 `../ir/` 的 IR 与 lowerer，不重造） |
| **前置阅读** | `research/dynamic-core/platform/RESULTS.md`（Q8）；`research/dynamic-core/ir/RESULTS.md` + `spec/ir.rs`（Q1，本实验的输入）；`design-executable-memory-floor-experiment.md`；`.claude/skills/decisive-experiment/SKILL.md` |
| **来源纪律** | **从零探索。** 不照搬任何既有解释器实现源码。IR 定义与三个载荷复用 Q1 产物（本轨自产），Win32 契约取本机 SDK 头文件 |

---

## 0. 背景与已确定的事

**关键洞察（决定实验形状）：解释器跑的是同一份中立 IR——它是另一个后端，不是另一套架构。**
`ir/lower/common.rs::lower(m, target)` 把 IR 降成原生码；本实验写 `interp::run(m)` 直接走 IR
执行。两者消费完全相同的 `Module`，区别只在「降成机器码再跳」vs「逐 op 解释」。

### 已确定、不在本实验讨论范围内的事

1. **四原语定义不改**（Q0/Q8 已定）。本实验是原语 ② 的第三种实现（直接 RX / 跨进程 / **解释**）。
2. **IR 与三个载荷不改**：直接复用 `ir/spec/ir.rs` 与 `ir/payloads/payloads.rs`。若解释器需要
   改 IR 才能跑，那本身是**发现**（记为 J1 失败），不是允许的动作。
3. **只做 x86_64 / Windows 本机可真跑**。不做第二个 ISA（那是证 ④ 的对照，用 Q5 已有数字对照即可）。
4. **Q8 已确立**：ACG 下 ①②（生成新代码）断，③④（符号解析 + 调用）不受影响。解释器的算术/控制流
   **不需要可执行内存**，它的 OS intent 走的正是 ③④——所以解释器天然活在 ACG/iOS 下。本实验量它的**代价**。

---

## 1. 硬约束（违反则实验无效）

1. **同一份 IR**：解释器消费的 `Module` 必须与 Q1 原生 lowerer 消费的**逐字节同一个对象**
   （同一 `payloads.rs`，同一 `spec/ir.rs`）。不得为解释器造一份改写过的 IR。
2. **同一验收**：三个载荷的正确性判据与 Q1 一致——`pure_compute→163`、
   `read_hash_print→"a49d2cbecc13994f"`（定长 35 字节输入的 FNV-1a/64）、`spawn_echo→打印"exit=07"且返回7`。
3. **速度对照必须同机同载荷**：解释 vs Q1 的 Win64 原生降级码（`jit_run`），同一 `Module`，同一输入。
4. **病灶探测器**：任何「为了让解释器速度好看而优化」的冲动（threaded dispatch / superinstruction /
   inline caching / 编译缓存）——是本实验要**检测并拒绝**的病。要的是**最小可用**的字节数与倍数，
   不是性能工程。若真忍不住优化，记为发现，不得偷偷加。
5. **诚实条款**：若解释器慢到不可用（如 >100×），**如实报**。那意味着「一等退路」只能是
   「降级可用」而非「平替」，是重要区分，不许为让结论好看而调度量。
6. **时间盒**：出 ①②③④⑤ 五个数即停。不做 JIT、不做 threaded code、不做第二个 ISA、不做任何优化。

---

## 2. 最小实验内容

| 维度 | 选择 | 理由 |
|------|------|------|
| **载荷** | 复用 Q1 的 `pure_compute` / `read_hash_print` / `spawn_echo` | 同一份 IR 才能证「另一个后端」；`pure_compute`（1M 循环）是计算密集、暴露解释开销的最坏情形；两个 OS 载荷证覆盖 + 量缝 |
| **执行后端** | 解释器 `interp::run(m)` vs Q1 原生 `jit_run(lower(m, Win64))` | 隔离变量：固定 IR 与输入，只换后端 |
| **平台** | Windows/x86_64 本机真跑 | 本机唯一能真跑；④ 的 ISA 无关性用 Q5 已有的三向拆分对照，不新造 ISA |
| **体积测法** | LOC（无注释空行，主）+ `.text` 字节（rust-objcopy 抽 `.text`，辅） | LOC 与 Q5（shared 238 / per-ISA 307–350 / per-target 99–137）、IR ⑤（shared 350 / per-target 246）直接可比；字节与 Q0 内核 568B / Q5 aarch64 644B 对照 |

---

## 3. 判据（动手前钉死，事后不得改）

| # | 判据 | 度量 | 性质 |
|---|------|------|------|
| **①/J1** | 解释器能否跑通三个载荷的**同一份 IR**（不改 IR）？ | 三个载荷各自过/不过 + 跑不通哪个、为什么 | **布尔门（主判据之一）** |
| **②/J2** | 解释器本体多大 | eval-core（ISA 无关）与 OS-seam（intent 分发）各自的 LOC + `.text` 字节；与内核 568B、aarch64 644B、Q1 lowerer（350 shared + 246 per-target LOC）对照 | **截距（主判据之一）** |
| **③/J3** | 解释 vs 已降级原生码，同一载荷的倍数 | 每载荷 wall-clock 倍数；计算密集（pure）与 OS 密集（rhp/spawn）分开报 | 截距（硬伤，必须量） |
| **④/J4** | 解释器有多少是 ISA 相关的？ | eval-core 里 ISA 相关 LOC 计数（预期≈0）；与 Q5 三向拆分对照 | 清单 |
| **⑤/J5** | Q1 的 L1–L5（OS 接口内容）缝在解释路线下**是否原样存在** | 逐条 L1–L5：解释器的 intent 分发是否仍需符号名/注入常量/结构体布局/out-param 宽度/哨兵约定 | **清单（最强结论候选）** |

### 度量纪律

- 产物 `rustc -O`（release），本机 `x86_64-pc-windows-msvc`，rustc 版本记进 `RESULTS.md`。
- `.text` 字节：`rustc --emit=obj -O` 出 `.obj`，`rust-objcopy -O binary --only-section=.text` 抽出测文件大小。
  eval-core 与 seam 的拆分用 `--cfg` 桩法（seam 桩成 `unreachable` 测 eval-core，差值 = seam）。方法写进 RESULTS。
- LOC 不含注释与空行。速度取多次运行的稳定值（报测法：次数、取 min/中位）。
- 每个数字第三方可复跑，命令写进 `RESULTS.md`。

---

## 4. 判决规则与 kill criterion

主产出是**五个数 + 一个架构判决 + 一张缝清单**。

```
主判据 = ①（覆盖布尔门）+ ②（体积截距），二者联合。
1. 若 ① 三个载荷有任一跑不通且原因是「IR 表达不了」→ 解释器不是 IR 的完整后端 → 判「不可行」，记原因。
   （若跑不通是纯工程 bug，修；若是 IR 结构性缺陷，记为发现。）
2. ① 全过 且 ② 体积与内核同量级（KB 级、且 eval-core 显著小于 Q1 lowerer）
   → 「核永远带解释、平台允许时才 JIT」可行 → 判「解释可作一等退路」。
3. ① 全过 但 ② 解释器大到与降级器同量级（LOC/字节 ≳ Q1 lowerer）
   → 退路价值重估 → 判「可行但不比 JIT 省」，记明。
4. ③ 只决定退路是「平替」还是「降级可用」，不决定可行性：
   - 若倍数温和（≲ 十几×）→ 可作平替候选。
   - 若 >100× → 明写「只能降级可用，不能平替」。诚实条款生效。
5. ⑤ 独立于 ①②③④ 成立：无论判决如何，如实报 L1–L5 在解释路线下在不在。
   预期「原样存在」——若真如此，说明那条缝独立于执行方式，是比 Q1 更强的结论。
kill criterion: 若 pure_compute（无 OS，纯算术/控制流）都解释不出 163 → IR 的核心语义解释器都实现不了，
   先查 bug；若确系 IR 语义无法解释 → 「另一个后端」命题证伪，立即停。
时间盒: 做到 ①②③④⑤ 五个数在 Windows 出数即停。不做 JIT、不做优化、不做第二个 ISA。
```

---

## 5. 目录结构

```
research/dynamic-core/interp/
├─ interp.rs        ← IR 解释器：eval-core（走 op/inst/term）+ do_intent（Win32 OS seam）
├─ main.rs          ← 驱动：J1 覆盖执行 + J3 速度对照（复用 ../ir 的 lowerer 与 jit_run）
├─ measure_core.rs  ← J2 字节测量的 lib crate（--cfg 桩法）
├─ RESULTS.md       ← 逐判据数字 + 复跑命令（最重要的产出）
└─ out/             ← 构建产物（git-ignored）
```

---

## 6. 已排除的选项

| 选项 | 为什么排除 |
|------|-----------|
| **给解释器加 threaded dispatch / superinstruction / inline cache / 编译缓存** | 硬约束 4：要最小可用的倍数，不是性能工程；优化会掩盖真实解释开销 |
| **为解释器改一份更好解释的 IR** | 硬约束 1：必须同一份 IR，否则不是「另一个后端」 |
| **做第二个 ISA 的解释器** | ④ 的 ISA 无关性可由「eval-core 零机器码」结构证明 + Q5 对照，不需真造第二个 |
| **实测 Linux/macOS/iOS 上的解释器** | 本机无 WSL/其他 OS；④ 是结构论证，OS seam 的可移植性标注可信度即可 |
| **产品级解释器（异常/GC/完整错误处理）** | 超时间盒；本实验测「最小可用的体积与倍数」 |

---

## 7. 本实验不回答的问题

- 解释器的产品级健壮性（错误传播、越界检查、非法 IR 防御）。
- 第二个 ISA / 第二个 OS 上解释器的**实测**体积与速度（本机测不了）。
- copy-and-patch / threaded code / JIT-解释混合等**加速**路线（那是下一个实验，且被本实验硬约束排除）。
- 解释器与 JIT 之间的**切换策略**（何时降级、如何 AOT 预铺）。
- IR 本身是否该扩容以更好支持解释（本实验消费 Q1 的 IR 原样）。

---

## 8. 结论回填

见 `research/dynamic-core/interp/RESULTS.md`（第三方可复跑形态）与本轨 `README.md` Q9 行。

---

*研究轨投影。不承诺版本归属，不改 PRD 能力状态。*
