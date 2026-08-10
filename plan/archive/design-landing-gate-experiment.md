# ⚠️ 已归档：dynamic-core 已判决实验规格

> **归档于 2026-08-10。** Q0–Q23 研究轨已经封闭，综合结论与重新开启条件由
> `research/dynamic-core/SYNTHESIS.md` 和 `research/dynamic-core/README.md` 拥有。
> 本文件只保存该问题的实验前判据与历史结果，不是活跃版本任务。

# Q12 — 落地关卡：跳进去之后，硬件与运行时会不会真的让字节跑（历史规格）

> ⚠️ **不是 AgenTerm 产品范围。** 动态核研究轨的一条实验（见
> `research/dynamic-core/README.md` 的 Q 索引）。不进任何版本 plan 的 must-ship，
> 不改 `PRD.md` 能力状态。

| 字段 | 值 |
|------|-----|
| **日期** | 2026-08-08 |
| **目的** | Q8 只测了第一关"能不能拿到可执行内存并跳进去"。综述（§7.2/§7.4）指出还有**第二关**：就算页面标成可执行、就算你跳进去了，**硬件与运行时仍可能拒绝执行那些字节**（CET-IBT/ENDBR64 落地指令、I-cache 一致性、Windows unwind 注册、放置 ±2GB 静默截断）。本实验**实测**：这些"第二关"里哪些在今天的真机上真的咬人，我们现有产物（Q2 降级器 / Q9 解释器）踩到了哪几条 |
| **实现位置** | `research/dynamic-core/landing/`（**不挂进根 workspace**） |
| **前置阅读** | `plan/reference-cross-target-execution.md` §7.2/§7.4（发现来源，**§11 注明未联网核对**）；`plan/archive/design-executable-memory-floor-experiment.md` + `platform/RESULTS.md`（Q8，本实验的第一半）；`.claude/skills/decisive-experiment/SKILL.md` |
| **来源纪律** | **从零探索。** 不照搬任何既有实现源码（含 Q9 解释器）；Win32/CPUID 契约取自公开文档与本机 SDK |
| **可信度纪律** | 本机 Windows Server 2022 / x86_64 真机——x86 侧必须实测。ARM（BTI/PAC/IC-DSB-ISB）本机无硬件，明确标"未验证的转述"，**不把综述未核实论断升级成结论** |

---

## 0. 背景与已确定的事

Q8 把第一关（原语 ①② 能否拿到可执行内存）测清了：Windows 默认放行，ACG 是 opt-in 硬化。
但 Q8 §7 明确**不回答**第二关（`design-executable-memory-floor-experiment.md` §7 最后一条：
"CET-IBT/BTI/PAC 落地指令、I-cache 跨线程一致性、x64 unwind 注册的执行后正确性——本实验只测能不能
跳进去，不测跳进去之后行为是否正常"）。本实验就是那一条。

### 已确定、不在本实验讨论范围内的事

1. **第一关的结论沿用 Q8**：默认放行、ACG 断三路（1655）。本实验测的是第一关之后。
2. **Q2 降级器的间接跳转结构已由读 `lower.rs` 确定**：入口 = `transmute+entry()` 间接 CALL；
   OS 回调 = `call qword [r15+idx*8]` 间接经内存；内部 = 直接 rel32 `jmp`/`jcc`。
3. **本实验不实现修补**（不发 ENDBR、不注册 unwind、不做 veneer）——只测哪些关卡咬人 + 估算修补代价。
4. **不做 ARM、不做 Linux**——本机无对应硬件/OS。

---

## 1. 硬约束（违反则实验无效）

1. **x86_64 侧结论必须来自本机运行的可执行文件**，每个数字第三方可复跑，命令写进 `RESULTS.md`。
2. **ARM 相关任何结论必须带可信度标签**（`实测` / `本地一手依据` / `未验证的转述`）。
3. **病灶探测器**：任何"因为想让结论轻松就不去开启对应缓解策略再测"的冲动，是本实验要**检测**的病。
   沿用 Q8 的双测姿态（默认测一遍、显式启用缓解再测一遍）——**但如实记录哪些缓解在 Windows 上根本
   没有运行时开关**（前向边 IBT 无开关，与 ACG 不同），那本身是结论。
4. **最该警惕的失败模式：只查文档不实测。** 综述那些论断本就是未验转述，照抄=零产出。
5. **时间盒**：出 ①②③⑤ 即停；④ 若太贵只给估算并说明。

---

## 2. 最小实验内容

| 维度 | 选择 | 理由 |
|------|------|------|
| 平台 | Windows/x86_64 实测；ARM 标转述 | 本机只有 x86 能真跑 |
| **CET/IBT 现状轴** | CPUID.7.0（硬件）× GetProcessMitigationPolicy（策略）× 实跳无 ENDBR 目标（强制） | 三层都测才能说清"没开"是硬件缺席还是策略没开 |
| **产物踩线轴** | 复刻 Q2 的两类间接跳（入口 P1 / 回调 P2）+ 基线（P0 无 ENDBR / P0b 有 ENDBR）+ ENDBR64 二进制扫描 + `RtlLookupFunctionEntry` | 直接测我们真实产物的模式，不是抽象测 |
| **放置约束轴** | near(<2GB) vs far(>2GB) 的 `call rel32`，用 Q2 `patch()` 逐字相同的 `as i32` 截断 | 隔离变量：同一发射路径，只动距离；确认失败模式=静默截断 |
| **解释免疫轴** | ACG 下跑净室 match 解释器 vs 同进程 codegen 路 | 用最狠第一关证明解释器零 codegen 表面 → 零第二关表面 |

---

## 3. 判据（动手前钉死，事后不得改）

| # | 判据 | 度量 | 性质 |
|---|------|------|------|
| **①** | 本机 CET/IBT 状态 | CPUID CET_SS/CET_IBT 位 + 进程策略位 + 实跳无 ENDBR 目标是否 fault | 布尔门 + 清单 |
| **②** | 我们产物踩线吗 | 间接跳目标数、ENDBR64 计数、开 IBT 后跑不跑（本机以"无硬件/无策略"实测）、unwind 表是否注册 | 清单 |
| **③** | 放置约束实际边界 | far 案例失败模式：静默截断跳错 vs 报错 | 布尔门（含义相反） |
| **④** | 修补代价 | 发 ENDBR/注册 unwind/保证放置各加多少字节/行，对照 X=3003B、Q9=1908B | 截距估算 |
| **⑤** | 对解释路线影响 | 解释器是否天然免疫全部四道第二关（结构 + ACG 下实测） | 布尔门（本实验预期最值钱） |

### 度量纪律
- 产物 `rustc -O` release，rustc 版本记进 `RESULTS.md`；错误码以本机 `winerror.h` 为准。
- ARM 结论一律带可信度标签。每个数字附复跑命令。

---

## 4. 判决规则与 kill criterion

主产出是**一张"哪些第二关咬人"的清单 + 可信度分层**，外加一个架构判决：

```
1. 主判据 = ⑤（解释路线是否结构免疫第二关）。
   - 若免疫成立 → 解释路线又得一条结构性优势（比体积/ISA 更硬）；codegen 路的第二关硬化
     成为"硬化平台部署前提"。
2. ③ 若 far 案例确为"静默截断跳错、不报错" → 放置约束判"最坏一类隐患"，
   并检查我们产物是否结构性触发（Q2 是否发 PC 相对外部引用）。
3. ①② 给"本机哪些关卡咬"清单：每条标"实测咬 / 实测不咬 / 潜伏未触发 / 转述"。
4. ④ 给修补代价估算；若免疫（⑤）成立，解释路线该项 = 0。
kill criterion: 若本机连 P0（间接跳无 ENDBR）都 fault → 本机 IBT 强制开启，与预判相反，
   立即改测"IBT 咬人下我们全部产物是否可跑"。
时间盒: 做到 ①②③⑤ 出数即停；④ 只估算。不做 ARM、不做 Linux、不做修补实现。
```

---

## 5. 目录结构

```
research/dynamic-core/landing/
├─ probe_landing.rs           ← ① CET/IBT 状态 + ② 产物间接跳模式 + unwind 表检查
├─ probe_reach.rs             ← ③ near/far ±2GB 静默截断
├─ probe_interp_immunity.rs   ← ⑤ ACG 下解释器 vs codegen 路
├─ RESULTS.md                 ← 逐判据数字 + 复跑命令（最重要产出）
└─ out/                       ← 构建产物（git-ignored）
```

---

## 6. 已排除的选项

| 选项 | 为什么排除 |
|------|-----------|
| 为测 ARM BTI/PAC 装 ARM 真机/模拟器 | 超时间盒；本机无 ARM 硬件，只诚实标转述 |
| 实现完整 ENDBR/unwind/veneer 修补 | 本实验测"哪些咬人 + 代价形状"，不做产品级修补（④ 只估算） |
| 手动构造穿帧 SEH 展开触发 unwind UB | 可做但超时间盒；改用 `RtlLookupFunctionEntry`=NULL 证明帧未注册（潜伏） |
| 照搬 Q9 解释器源码测免疫 | 违反从零探索纪律；改写净室 match 解释器 |

---

## 7. 本实验不回答的问题

- ARM BTI/PAC 落地垫、ARM `IC/DSB/ISB` 一致性、arm64e PAC 签名的**实测**（本机无 ARM）。
- Linux `.eh_frame`/`__register_frame`、macOS `sys_icache_invalidate` 的实测（本机无对应 OS）。
- ENDBR/unwind/veneer 的**产品级修补实现**与其运行时开销。
- 若真在开了 CET-IBT 的 Windows/ARM 真机上，我们产物的完整可跑性（需要那样一台机器）。

---

## 8. 结论回填

见 `research/dynamic-core/landing/RESULTS.md`（第三方可复跑形态）与本轨 `README.md` Q12 行。

**要点**：综述点名的第二关里，**在本机 x86_64 上今天没有一条真的咬我们产物**——CET 硬件缺席
（CPUID CET_IBT=0）、I-cache x86 免费、Windows unwind 潜伏未触发、放置 ±2GB 被 Q2 设计结构性避开。
第二关从"悬着的真隐患"**降级为"未来硬化平台的部署前提"**，与 Q8 对第一关同构。**两条硬结论**：
① 放置约束是最坏一类（**实测确认静默截断、跳到错地方、不报错**），Q2 现在躲开仅因它不发 PC 相对
外部引用，任何转 copy-and-patch 的改动会当场引爆；② **解释路线对全部四道第二关结构免疫**（结构证明
+ ACG 下实测），这是解释路线继 Q9 体积/ISA 之后的第三条、也是最硬的一条结构性优势，④ 里 codegen 路
要补的 ENDBR/unwind/放置代码解释器一行不用加。完整判决 trace 与可信度分层见 RESULTS.md。

---

*研究轨投影。不承诺版本归属，不改 PRD 能力状态。*
