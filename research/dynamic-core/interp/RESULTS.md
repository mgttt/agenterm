# Q9 RESULTS — 解释执行作为一等后端（Windows/x86_64，实测）

**规格**：[`plan/design-interpreter-backend-experiment.md`](../../../plan/design-interpreter-backend-experiment.md)

Q8 把四原语劈成两组：①②（生成新代码）在 ACG/硬化进程/iOS 下脆弱，③④（够到已有代码）稳固，
结论是「解释执行必须从第一天就是一等退路」。本实验建一个**跑同一份 Q1 中立 IR 的解释器后端**，
量它多大、多慢、覆盖多少、多平台无关、以及 Q1 的 L1–L5 缝是否原样存在。

## 判决 — **解释可作一等后端（first-class fallback）**

- **① 覆盖：全过。** 三个 Q1 载荷用**逐字节同一份 IR**（未改一行 `payloads.rs`/`spec/ir.rs`）跑通。
- **② 体积：KB 级，且远小于降级器。** eval-core **55 LOC / 1908 B**，整个解释器 **136 LOC / 3177 B**，
  是 Q1 原生 lowerer（487 LOC / 14819 B）的 **≈21%**。
- **③ 速度：硬伤只在计算密集内循环。** 计算密集载荷解释比**优化原生**慢 **≈77×**、比 Q1 的
  naive 降级码慢 **≈5×**；**OS 密集载荷 ≈1.0×**（解释开销被 OS 调用完全淹没）。
- **④ 平台无关：eval-core 的 ISA 相关 LOC = 0**（实测无任何机器码/寄存器/x86-64 token），
  一次编写、跨 ISA 原样复用。把 Q5 的 per-ISA 307–350 LOC **塌成 0**。
- **⑤ 缝：L1–L5 原样存在，内容与 `win64.rs` 逐条相同。** 解释**并不消除**那条缝——它独立于执行方式。
  这是比 Q1 更强的结论：那条缝是 OS 接口的属性，不是 JIT 的属性。

**主判据 ①+② 成立 →「核永远带 ~2–3 KB 解释器、平台允许时才 JIT」是可行的优雅降级。**
③ 把它进一步分级：作**可用性退路**是一等的；作**性能平替**，OS 密集路径 1.0× 直接平替，
计算热路径 77× 只能「降级可用」。

---

## 度量条件

| 项 | 值 |
|----|----|
| 机器 | Windows Server 2022 Datacenter 10.0.20348（真机） |
| ISA / 目标 | x86_64 / `x86_64-pc-windows-msvc` |
| 编译器 | `rustc 1.97.0 (2d8144b78 2026-07-07)`，`-O`（release） |
| 依赖 | 无外部 crate；仅 `kernel32` FFI。IR 与三载荷 `#[path]` 复用 `../ir/`（未重造） |
| 字节测法 | `rustc --emit=obj -O --crate-type=lib` → `llvm-size` 读 `.text`（Berkeley text 列） |
| LOC 测法 | 去空行与纯注释行；与 Q1 ⑤ 同法（本文 asm/common/win64 复算得 202/148/137，与 Q1 一致，校准通过） |

## 复跑命令

```sh
cd research/dynamic-core/interp
BIN="$(rustc --print sysroot)/lib/rustlib/x86_64-pc-windows-msvc/bin"

# ①③ 覆盖 + 速度
rustc --edition 2021 -O -A dead_code main.rs -o out/driver.exe
(cd out && ./driver.exe)

# ② 字节（.text）
rustc --edition 2021 -O --crate-type=lib --emit=obj -A dead_code measure_core.rs  -o out/interp_full.o
rustc --edition 2021 -O --cfg interp_measure_core --crate-type=lib --emit=obj -A dead_code measure_core.rs -o out/interp_core.o
rustc --edition 2021 -O --crate-type=lib --emit=obj -A dead_code measure_lower.rs -o out/lower.o
"$BIN/llvm-size.exe" out/interp_full.o out/interp_core.o out/lower.o
```

---

## ① 覆盖 — 布尔门（PASS）

`out/driver.exe`：

```
== J1 — coverage: does interp::run pass all three, on the UNCHANGED Q1 IR? ==
  pure_compute     -> 163   (expected 163)   OK
  read_hash_print  -> "a49d2cbecc13994f"   (expected "a49d2cbecc13994f")   OK
  spawn_echo       -> printed "exit=07", ret=7   (expected "exit=07", 7)   OK
```

- **三个载荷全过，未改一行 IR。** `interp::run(m)` 消费的 `Module` 与 Q1 `common::lower(m, Win64)`
  消费的**是同一个对象**（同一 `payloads.rs`）。
- **跑不通的没有。** IR 的 13 个 Op / 4 个 Inst / 4 个 Term / 6 个 Intent 全部可解释——
  证明这套中立 IR 是「可被完整解释的」，解释器是它的**完整后端**，不是子集后端。
- `pure_compute`（无 OS）证核心语义（算术/循环/分支）；`read_hash_print`/`spawn_echo` 证 OS intent
  全链路（Alloc/FileOpen/FileRead/FileClose/WriteStdout/SpawnWait）。

**→ 主判据 ① 是 PASS，命题不被证伪。**

## ② 体积 — 截距

`llvm-size` `.text`（字节）+ LOC（无注释空行）：

| 部件 | LOC | `.text` 字节 | 说明 |
|------|----:|----:|------|
| **eval-core**（`run`+`eval_op`，ISA 无关） | **55** | **1908** | 解释器本体：走 op/inst/term，纯 u64 字运算 |
| **OS seam**（`do_intent`+`spawn_wait`，L1–L5） | **81** | **1269**（=3177−1908） | Win32 FFI，实现 6 个 intent |
| **整个解释器** | 136 | **3177** | — |
| Q1 原生 lowerer（`asm`+`common`+`win64`） | 487 | **14819** | 同法测（对照） |
| — ~~参考：Q0 四原语内核~~ | — | ~~568~~ | **已撤回，见下方口径框** |
| — ~~参考：Q5 aarch64 内核~~ | — | ~~644~~ | **已撤回，见下方口径框** |

> **口径（2026-08-08 口径审计后补，并撤回两行参考值）**
>
> 本表所有**未撤回**的字节数同口径：`rustc -O --crate-type=lib --emit=obj` →
> `llvm-size` Berkeley **`.text`**，`x86_64-pc-windows-msvc`，**std + 默认 panic**（保留
> unwind 路径），未 strip。**1908 / 1269 / 3177 / 14819 四个数彼此可比**（同一命令、同一份
> IR、同一构建），所以「解释器 = 降级器的 21%」「小 4.7×」在本口径内**成立**。
>
> **撤回的两行**：568 / 644 B 是 **Q5 的 `isa/kernel/prim.rs`**（四原语的一份全新最小转写）
> 在 `{x86_64,aarch64}-unknown-linux-gnu` 上、**`no_std` + `panic=abort` + `--crate-type
> staticlib`** 的 `.text`。两处问题：(a) 568 B **不是 Q0 的内核**（本表原先如此标注，是错的；
> Q0 自己的内核产物是整个 ELF ~2.7 KB 那一档，另一种口径）；(b) 与本表其余数**跨 std/no_std
> 且跨 target**，不可相除。故「1908 B ≈ 内核 568 B 的 3.4×」**不成立，改判为该轴未测定**。
> 详见 [`../COMPARABILITY.md`](../COMPARABILITY.md) §2 U5。

读法：
- **eval-core 55 LOC / 1908 B 顶替了整个 350-LOC 共享 lowerer**（`asm` 202 + `common` 148）：
  解释不需要编码器、不需要帧/寄存器机制。55 vs 350 = **16%**。
- **整个解释器 3177 B = 降级器 14819 B 的 21%（小 4.7×）。** 降级器的字节大头是 `asm.rs` 的
  x86-64 编码器——正是解释器**完全没有**的那部分。
- ~~vs 内核：eval-core 1908 B ≈ 四原语内核 568 B 的 3.4×~~ —— **撤回（跨口径，见上方口径框）**。
  能说的是**绝对量**：eval-core **1908 B**、整个解释器 **3177 B**，本身就是 **KB 级**；
  「与四原语内核同量级」这个**比值**在现有文档下**未测定**。它之所以只有这么大，是因为它分发的是
  **完整 IR**（13 op/4 inst/4 term/6 intent），不是只有 4 个原语。

**→ KB 级、且在本表口径内 firmly 小于降级器（21%）。落判决树分支「解释可作一等退路」。**
（原文此处写的「内核量级」依赖上面那个被撤回的比值，已删——判决树分支**不依赖它**：
分支条件是「解释器是 KB 级而非 MB 级」，1908/3177 B 独立满足。）

## ③ 速度 — 唯一硬伤，如实量

`out/driver.exe`（reps: pure=200, rhp=200, spawn=30；per-call = total/reps；两次运行稳定）：

| 载荷 | 优化原生（JIT 天花板） | Q1 naive 降级码 | 解释器 | 解释 / naive | 解释 / 优化 | 主导 |
|------|--:|--:|--:|--:|--:|------|
| `pure_compute`（1M 循环） | ~0.29 ms | ~4.4 ms | ~22 ms | **≈5×** | **≈77×** | 计算 |
| `read_hash_print` | — | ~0.40 ms | ~0.40 ms | — | **≈1.0×** | OS |
| `spawn_echo` | — | ~15 ms | ~15 ms | — | **≈1.0×** | 进程创建 |

**诚实条款（规格 §1.5）：**
- 「解释 vs 已降级原生码」的**字面答案是 ≈5×**——但那会 flatter 解释，因为 Q1 的降级码是
  **故意 naive**（无寄存器分配，每 op 重载栈槽），本身就慢。对照**优化原生**（Rust `-O` 的
  `reference_pure`，即一个带寄存器分配的真 JIT 会产出的东西）是 **≈77×**。**两个数都报。**
- **硬伤完全集中在计算密集内循环。** 一旦载荷触碰 OS（open/read/write/spawn），解释开销被
  OS 调用**彻底淹没**，解释 ≈ 原生（1.0×）。这不是「到处慢 77×」，是「热计算循环慢 77×、
  其余不慢」。
- **77× < 100×**，但对计算热路径是实打实的代价。判语：作**可用性退路**一等；作**性能平替**，
  OS 密集路径直接平替，计算热路径「降级可用」。**未为让结论好看调过任何度量。**

## ④ 平台无关 — 卖点，已证

- **eval-core 的 ISA 相关 LOC = 0（实测）。** 对 `interp.rs` 第 25–93 行（`run`+`eval_op`）grep
  `rax|rbp|rcx|rsp|r11|mov|shadow|syscall|jmp|encode|register` —— **可执行代码零命中**
  （仅两处命中在注释里，描述「被降级的代码做了什么」）。eval-core 只是 Rust 解引用 u64、做
  wrapping 算术，**在任何 ISA 上原样编译运行**。
- **把 Q5 的 per-ISA 成本塌成 0。** Q5 三向拆分：shared 238 / **per-ISA 307–350** / per-target 99–137。
  解释器**没有 per-ISA 桶**：eval-core 55 LOC 是**跨 ISA 共享**的，新增一个 ISA 的边际成本 ≈ 0
  （只剩 per-OS 的 seam，而 seam 是 per-OS 不是 per-ISA）。
- **字节侧印证**：降级器 14819 B 的大头是 x86-64 编码器（换 ISA 要整个重写）；解释器 eval-core
  1908 B **一个字节的编码器都没有**。这正是「解释是唯一同时小/安全/可移植」里「可移植」的机器级证据。

## ⑤ 缝 — L1–L5 在解释路线下**原样存在**（最强结论）

逐条对照解释器 `do_intent`/`spawn_wait` 与 Q1 `win64.rs::emit_call`/`emit_spawn`：

| 缝 | Q1（lowerer）在哪 | 解释器（seam）在哪 | 一样吗 |
|----|------|------|:--:|
| **L1** 外部引用不可中立命名 | `win64::SYMBOLS` 9 个 kernel32 符号 | `mod seam` extern 块**同样 9 个符号、同顺序**（实测逐字相同） | **一样** |
| **L2** 语义 arity ≠ native arity，注入常量 | `FileOpen`→7 args 注入 `GENERIC_READ/OPEN_EXISTING...`；`SpawnWait`→10 args | `do_intent` 同样把 `0x8000_0000/3/...` 注入，`FileOpen` 1→7、`SpawnWait` 0→10 | **一样** |
| **L3** OS 结构体布局无中立形 | `STARTUPINFOA` 104 / `cb`@0 / `PROCESS_INFORMATION.hProcess`@0 写进 VirtualAlloc 缓冲 | `#[repr(C)]` 结构体，**同样 104 / cb@0 / hProcess@0**（`assert_eq!(size_of, 104)` 运行时通过） | **一样** |
| **L4** out-param 是 32 位布局事实 | `ReadFile`/`GetExitCodeProcess` 读 4 字节 out DWORD | `do_intent` 用 `u32` out-param，读回 zero-extend | **一样** |
| **L5** 错误/哨兵约定不同 | Q1 回避（不测哨兵） | 解释器同样回避（handle 当指针，不测 `(HANDLE)-1`） | **一样（同为潜在）** |

**唯一区别是语法**（Rust FFI+结构体 vs 寄存器摆放+VirtualAlloc 缓冲），**内容逐条相同**。

**→ 解释执行并不消除那条缝。** 这印证了规格 §4 分支 5 的预期，且给出更强结论：**L1–L5 是 OS 接口的
属性，独立于执行方式**。JIT 与解释都得知道 `CreateProcessA` 要 104 字节的 `STARTUPINFOA`。
解释路线消掉的是 **ISA 特定机制**（编码器、寄存器分配——14819→1908 B 的塌缩），消掉的**不是**
OS 接口缝。**执行方式与 OS 缝正交。**

---

## 判决 trace（按规格 §4 的树走）

1. **① 全过、无需改 IR** → 不落「不可行」。
2. **① 全过 且 ② KB 级、eval-core（55 LOC/1908 B）显著小于 Q1 lowerer（487 LOC/14819 B）**
   → 落分支 2 → **判「解释可作一等退路」**。核可永远内嵌 ~2–3 KB 解释器，平台放行时才 JIT。
3. **③** 决定退路是「平替」还是「降级可用」，不决定可行性：OS 密集 1.0×（平替）、
   计算热路径 77× vs 优化原生（>十几×，**降级可用，非平替**）。
4. **④** eval-core ISA 相关 = 0 → ISA 轴上解释器的边际成本 ≈ 0，把 Q5 的 per-ISA 307–350 LOC 塌成 0。
5. **⑤** L1–L5 原样存在，内容与 lowerer 逐条相同 → 缝独立于执行方式。
6. kill criterion 未触发（`pure_compute` 解释出 163）。

**净结论**：把解释做成一等后端是**可行的优雅降级**——本体 ~2–3 KB（同口径下是降级器的 1/5；
「内核量级」这个跨口径说法已撤回，见 ② 的口径框）、
ISA 无关（新 ISA 边际成本≈0）、覆盖全部三载荷。**代价是计算热循环 ~77× 慢**（OS 路径不慢），
所以它是**可用性上的一等退路 + OS 密集下的性能平替**，但**不是计算热路径的性能平替**。
那条 L1–L5 的 OS 缝**在解释路线下原样存在**——因为它本就与执行方式无关。

## 与规格/预期不符之处（诚实条款）

1. **③ 的字面倍数被 naive 基线严重压低**：对 Q1 降级码只 5×，看着「解释几乎不亏」——这是假象，
   因为基线本身慢。加测优化原生后真实倍数 77×。**两个数都印，读者用优化原生那个判性能。**
   （这是相对规格的一处主动加测，非改度量：规格 §3 只要求「vs 已降级原生码」，我多给了天花板。）
2. **推翻预期的地方**：预期解释会「到处慢」；实测是**只有计算密集慢、OS 密集完全不慢（1.0×）**。
   这个分裂很关键——它说明解释的硬伤能被「大部分真实工作是 OS 密集」这一事实大幅稀释。
3. **② 的 seam/core 字节拆分是近似**：`--cfg` 把 `do_intent` 桩成 `unreachable`，Call 分发胶水
   会在 core/seam 之间小幅移动，故 seam=1269 B 记为近似值；LOC 拆分（55/81）是精确的。
4. **④ 未真造第二个 ISA**（规格 §1.6 时间盒禁止）：ISA 无关性由「eval-core 零机器码」结构证明 +
   Q5 已有 per-ISA 数字对照，未做第二 ISA 的实测。
5. **⑤ 的 L5 双方都未作为硬失败触发**（与 Q1 一致）：两个后端都回避哨兵测试，故 L5 记为
   「潜在、同为回避」，不是实测触发。

## 独立参考值（证明两后端不是一起错的）

FNV-1a/64 of `"dynamic-core experiment 2026-08-08\n"`（定长 35 字节）= `a49d2cbecc13994f`
（Q1 独立算得同值）。解释器与 Q1 Win64 原生降级码对同一输入产出**同一哈希**——两条独立执行路
（解释 vs 降级）交叉验证，非共同错误。
