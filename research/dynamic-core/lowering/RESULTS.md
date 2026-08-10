# Q2 RESULTS — 最小可用 IR→原生降级器的字节数 X，以及核内/核外

**规格**：[`plan/archive/design-lowering-cost-experiment.md`](../../../plan/archive/design-lowering-cost-experiment.md)
（规格与 `README.md` 内部沿用早期编号「Q3」；**问题板上的编号是 Q2**，本文件按板上编号。）

> **本文件的来源与授权。** 本实验原先**没有 RESULTS.md**——结论散在
> [`README.md`](./README.md)、规格 §8、以及提交 `9092acf4` 的信息里，而 X=3003 B 已被
> 「执行层判决」当作三条已决落地路之一引用。本文件把这些既有材料**归拢**成技术清单要求的
> 「每条 [实测] 在该 Q 的 RESULTS.md 里有复跑命令」的形态。**未重新测量、未改任何数字**；
> 源材料里找不到依据的，本文件标 **口径不明**，不补数。归拢依据见文末《来源对账》。

---

## 判决 — **真取舍，无免费午餐；决定性的意外是「X 不小」**

- **③ 总交付偏核内**（Linux 6360 vs 7816，差 ~1.5 KB 且**恒定**）；
- **④ 冻结 TCB 偏核外**（~2.93 KB vs ~6.2 KB，核内约 2.1×）；
- 之所以成为真取舍而不是显然题：**X ≈ 一个内核那么大**（3003 B vs 内核基线 2932 B），
  远超规格 §4.2 事前钉死的「X < 25% 内核 ⇒ 判核内」阈值；
- **② 的斜率不区分放置**：x86 专属块（139/301 行 ≈ 46%）无论核内核外都随 ISA 数复制。
  它量到的是**「有降级器」本身的新斜率**——codegen 让每 ISA 的原生足迹从 Q0 的 ~2.7 KB 近乎翻倍；
- **kill criterion 未触发**（X=3003 ≪ 8×内核 ≈ 23 KB），核内在字节上完全可行；
- 实现期冒出一条规格外的决定性约束（已量化）：**把降级器当 flat 非重定位 blob 跑，
  必须 memset-free + jump-table-free**，实测代价 **X +8%（2777→3003）** 且更脆。

**净判决**：按 §4 字面树（③ 领先）+ PIC-flat 约束 → 偏**核内**；若把「最小冻结 TCB」当压倒性
目标（④）→ 偏**核外**，但须为 PIC-洁净降级器（+8%、更脆）或一个会重定位的内核买单。
**两边都不免费。**

---

## 度量条件与执行状态（**先读这一节再引用任何数字**）

| 项 | 值 |
|----|----|
| 机器 | Windows Server 2022 Datacenter 10.0.20348（真机），x86_64 |
| 编译器 | `rustc 1.97.0`，`-O`（release），strip |
| 目标 | **Windows**：`x86_64-pc-windows-msvc`（PE，512 对齐）／**Linux**：`x86_64-unknown-linux-gnu`（ELF，无填充） |
| 构建 flag | `--edition 2021 -O -C panic=abort -C debuginfo=0`（+ flat blob 另加 `-C force-unwind-tables=no -C relocation-model=pic -C llvm-args=-min-jump-table-entries=200`，链接 `ld.lld --oformat binary -T build/flat.ld`；exe 链接 `--strip-all -static`） |
| 构建轴 | **`no_std` + `panic=abort`**（全部产物，无一例外） |
| 依赖 | 无外部 crate；Q0 的 `core/`、`adapters/`、`pack/variant_b_twolayer/` 原样复用 |

### 执行状态（**分产物，不是全轨一句话**）

| 产物 | 状态 |
|------|------|
| `A_lower_{pure,rhp,spawn}_windows.exe`、`B_lower_{...}_windows.exe`（6 个） | **真机执行**，三载荷语义与 Q0 逐位一致（163 / `a49d2cbecc13994f` / `exit=07`+退出码 7） |
| **`mx_lower_flat.bin` / `mx_driver_flat.bin`（X=3003 B 的来源）** | **仅字节测量，从未执行**。它们是 **Linux/ELF flat 度量脚手架**（本机无 WSL），且按构造就不是可运行产物 |
| `A_lower_*_linux` / `B_lower_*_linux` / `baseline_kernel_linux` / `lowblob_*_linux.bin`（③④ 的 Linux 列） | **仅字节测量，从未执行**（交叉编译，本机无 WSL） |

> **要紧的一条：README 与「执行层判决」引用的头号数字 X=3003 B、以及 ③④ 的 Linux 列，
> 全部是 Linux/ELF 交叉编译的字节测量，那些产物从未被执行。** 被执行的是 Windows PE 那条
> （核内 7680 / 核外 9216），它是 PE 512 对齐数，看不到真实字节增量。两件事不要混。

### 体积口径四元标签（照 [`COMPARABILITY.md`](../COMPARABILITY.md) §6 R-S）

**① X 的口径**：

| 维 | 值 |
|----|----|
| **边界** | **只有 `lower.rs` 自身的机器码 + 它的常量。** OS intent/seam 层**不在里面**（`runner.rs` 的 env 表、Q0 adapters 是原生 substrate，明确 not in X）；IR 字节、内核、loader、`abi` panic handler、`mem_intrinsics` **都不在**（后两者在被减数与减数里各有一份，相减抵消） |
| **工具** | **flat-PIC blob 相减**：`mx_lower_flat.bin` 4089 − `mx_driver_flat.bin` 1086 = **3003**。是 code+data，无 ELF 头 |
| **构建** | `no_std` + `-O -C panic=abort -C debuginfo=0 -C force-unwind-tables=no -C relocation-model=pic -C llvm-args=-min-jump-table-entries=200` |
| **目标/执行** | x86_64 **ELF/Linux**；**该产物从未执行** |

**固定三档上报**（R-S 第 2 条）：

| 档 | 值 | 说明 |
|----|---|------|
| **L1 机制码**（纯机制，不含 OS 层、不含数据） | **3003 B** | = X |
| **L2 机制 + OS seam**（跑通真实载荷所需的全部代码） | **未测定** | Q2 的 OS seam 是 Q0 adapters 的**原生复用**，从未被单独隔离测量。**这是本实验的口径空洞**，与 Q9 的 3177 B（含 seam）不可并排——见 `COMPARABILITY.md` U1 |
| **L3 整个投递足迹**（code+data，flat 口径） | 核内 **6360 B** / 核外 **7816 B**（Linux，跑 rhp） | 见 ③ |

---

## ① X = 最小可用降级器的字节数 — **3003 B**（Linux/ELF flat，未执行）

| 度量 | Linux（无填充） | Windows |
|---|--:|--:|
| `mx_lower_flat.bin`（含降级器） | 4089 | —（ELF 目标，与宿主无关） |
| `mx_driver_flat.bin`（不含降级器，其余相同） | 1086 | — |
| **X = 相减** | **3003 B** | 同 3003（同一个 ELF flat 数，**Windows 侧没有独立的 X 测量**） |
| 核内可用的跳转表版（不抑制跳转表） | ~2777 B | — |
| **抑制跳转表的代价** | **+8%**（2777 → 3003） | — |

**"最小可用"的达成**（规格 §1.2 事前钉死）：24 opcode 的朴素寄存器机字节码
（`ir.rs`）的 x86_64 **单遍发射 + 一遍 rel32 回填**（`lower.rs`），整数/指针字子集，
固定 vreg→物理寄存器映射，**无优化器、无寄存器分配器、无 IR 语法糖**。三载荷语义
（100 万次乘加循环 / 逐字节 FNV-1a / 十六进制与十进制格式化 / 条件退出码 / 字节 load-store /
循环与分支）**全部走 IR、全部运行时降级**，核内核外两种打包在 Windows 全部实测通过。**非玩具。**
**未加第五类原语**——降级器是原语 ①（mem_alloc/mem_protect）与 ②（jump）的纯使用者。

**已知偏差（规格 §1.3 声明）**：两种打包都用 rustc 把降级器**源码**编成原生码，
故 **X 是「最小可用降级器成本的上界」**，手写汇编可能更小。结论不得写成绝对判断。

**度量脚手架**：X 含 ≤~20 B 脚手架（`black_box` 使 IR 不透明，防 DCE 把编码器优化掉）。

---

## ② 共享 vs ISA 专属拆分 — **x86 专属 139 行 / 共享 162 行（46% 专属，共 301 行）**

`lower.rs` 自带 `[X86_64]` / `[SHARED]` 分段横幅（第 42 行、第 195 行）。

| 段 | 行范围 | LOC | 性质 |
|---|---|--:|---|
| `[X86_64]` 机器码编码器（`e_*`） | 42–194 | **139** | **每加一个 ISA 要复制的量** |
| `[SHARED]` 驱动（IR 解码 / 控制流 / 标签回填 / call 策略 / vreg 映射）+ 文件头 | 1–41、195–365 | **162** | ISA 无关 |
| **合计** | 1–365 | **301** | — |

**LOC 口径（本次归拢时补明，原文档未声明）**：**非空行、非注释行**（`//` 与 `//!` 开头的整行
均计为注释行），不含测试与文档。下面的命令逐字复算出 139 / 162 / 301 三个已发表数字，
**与 Q1（350）、Q5（307）、Q9（55/81/136）同口径，可并排**：

```sh
cd research/dynamic-core/lowering
sed -n '42,194p' lower.rs | grep -cvE '^\s*(//.*)?$'                            # 139  [X86_64]
{ sed -n '1,41p' lower.rs; sed -n '195,365p' lower.rs; } | grep -cvE '^\s*(//.*)?$'  # 162  [SHARED]
grep -cvE '^\s*(//.*)?$' lower.rs                                                # 301  total
```

**斜率读法**：每加一个 ISA 复制 ≈46% × X ≈ ~1.4 KB。**但 ② 不是「核内 vs 核外」的判据**——
两种放置都每 ISA 一份。规格 §4 已声明：**只建了一个 ISA，② 是单 ISA 的源码结构投影，
不是两点实测斜率**，给方向不给确值。（真两点斜率由 Q5 补上：per-ISA 307–350 LOC。）

---

## ③ 总交付 — **核内小 ~1.5 KB，且是恒定差**

跑一个载荷（`read_hash_print`）所需的全部字节：

| 打包 | Linux（无填充） | Windows（PE 512 对齐） |
|---|--:|--:|
| **核内**（降级器静态链进产物，载荷=IR 字节） | **6360** | **7680** |
| **核外**（最小内核 + 降级器 flat blob，双段 mmap+jump） | **7816** | **9216** |
| — 核外的两文件形态（内核 3112 + `lowblob_rhp` 4881） | 7993 | — |
| **差** | **核内小 1456 B（~20%）** | 核内小 1536 B（**含 512 对齐块，不可读作真实代码增量**） |

差为**恒定**：服务 M 个载荷时两边每载荷只加 IR 字节。核外更大的原因：复制了一份
`mem_intrinsics`、吃 PIC 开销、还要单发一份内核。

---

## ④ TCB — **核内 ≈ 2.1× 核外**

| 打包 | Linux | Windows |
|---|--:|--:|
| **核内** = 整个产物（随载荷长） | **~6.2–6.4 KB**（`A_lower_{pure,rhp,spawn}_linux` = 6200 / 6360 / 6376） | 7680 |
| **核外** = 冻结的最小内核（恒定，与载荷无关） | **~2932 B** | **~3958 B** |

**X/内核 = 3003 / 2932 = 102%**（核内跳转表版 2777/2932 = 95%），**远超 §4.2 事前钉死的 25% 阈值**
→ 核内让冻结 TCB 近乎翻倍。这正是 ①②③④ 之所以构成真取舍的原因。

### ⚠️ 「内核基线 ≈2.93 KB」是怎么测的 —— **口径为本次重建，原文档未写**

审计 N2 点名：整个 in/out-kernel TCB 判决建在这个基线上，而**规格 §8 与 README 都只给了数值
（Linux ~2932 B / Windows ~3.96–4.10 KB），从未写它怎么测**。本次归拢在既有构建脚本与产物里
把它重建如下——**数值逐字节吻合，但这是重建，不是文档记载**：

| 候选口径 | 算式 | Linux | Windows |
|---|---|--:|--:|
| **(a) 变体 B 产物 − 内嵌的降级器 blob** | `B_lower_pure` − `lowblob_pure` | 7640 − 4708 = **2932** | 9216 − 5258 = **3958** |
| (b) 专建的 `baseline_kernel_*`（最小内核 + 内嵌 166 B 预编译原生 pure blob）整文件 | — | **3112** | **4096** |
| (b′) 同上 − 内嵌 166 B blob | 3112 − 166 / 4096 − 166 | 2946 | 3930 |

- 已发表的 **2932（L）与 3.96 KB（W）来自 (a)**（两者都逐字节对上）；
  规格 §8 的「Windows ~3.96–4.10 KB」这个**区间正是 (a) 与 (b) 两种算法的并存**，
  而 README 那句「≈2.93 KB (L) / ~4 KB (W)」**左边取 (a)、右边取 (b)——两侧不是同一把尺**。
- (a) 与 (b′) 相差 **14 B**（ELF 段对齐），对判决无影响，但说明「内核基线」在本实验里
  **有两个相差 14 B 的实例**，文档没有指定用哪个。
- 无论 (a) 还是 (b)，口径都是 **整个 strip 后的静态 ELF/PE 文件（含 ELF 头、entry bootstrap、
  `mem*` intrinsics、原语表）**，即 `COMPARABILITY.md` 的 **S-F 口径**，与 Q0 的
  「B kernel-only = 二进制 − blob」同法。Q0 该数为 ~2738 B（**spawn 能力之前**），
  spawn 使每个含内核的产物 +208 B（Q0 §④）→ 2738+208 = 2946 = 本实验的 (b′)，交叉吻合。
- **Windows 侧的相减不可信**：PE 512 对齐会把 Δ 取整到一个块（Q0 ④ 与 Q3 自己都说了）。

> **由此产生的一条必须随身携带的限定**：`X ≈ 整个内核`（3003 vs 2932）**是跨口径的比较**——
> 左边是 **flat blob 相减（不含 ELF 头/entry/mem\* intrinsics/原语表）**，右边是**整个 stripped 文件（含）**。
> 方向上偏保守（X 一侧不含开销，真实比值只会更高），但它作为 in/out-kernel 判决的支点，
> **引用时必须注明两侧口径不同**。详见 `COMPARABILITY.md` §2 U7。

---

## 判决 trace（按规格 §4 的树逐步走，规则事前写死）

1. **③（总交付）分出胜负？** 是——核外 > 核内 ~1.5 KB（~20%）。按 §4.1「核外明显 > 核内 → 判**核内**」，③ 指向**核内**。
2. **④（TCB）反向**：核内把冻结 TCB 从 ~2.9 KB 抬到 ~6.2 KB（≈翻倍），因为 **X ≈ 内核大小**（102% ≫ 25% 阈值）。④ 指向**核外**。
3. **②（斜率）不区分放置**：x86 专属块 ~46% 无论核内核外都随 ISA 数复制。② 量到的是「有降级器」本身的新斜率——**codegen 让每 ISA 的原生足迹近乎翻倍**，与放置无关。
4. **kill criterion**：X=3003 < 8×内核（~23 KB）→「核内不可行」**未触发**。
5. **净判决**：真取舍，两种读法都写出来（见文首判决）。**未为让结论好看而改度量。**

---

## 与规格不符 / 推翻预期之处（诚实条款）

1. **核外运行时的 flat-PIC 约束（实现期意外，已量化+已解决）**：核外变体初次运行崩溃——
   (a) 大栈数组零初始化发出 `memset` 调用（flat blob 满足不了），改为从原语 ① 申请 scratch；
   (b) dense 派发被编成 PC-relative 跳转表，展平复制后仍破（`llvm-objdump` 实证 emit 里有
   `jmpq *%rcx`），加 `-C llvm-args=-min-jump-table-entries=200` 抑制后消失。
   两条修好后核外三载荷在 Windows 全部跑通。**这不是偏差而是发现**，已并入判决：
   核外须 PIC-flat 洁净，**实测代价 X +8% 且更脆**。
2. **② 是单 ISA 投影，非两点斜率**（规格 §4 已声明），证据强度低于真两点。
3. **X 用 ELF flat 两 blob 相减度量**（Q0 blob 口径）：`driver_flat` 含 `abi` panic +
   `mem_intrinsics`（两半都有，抵消），故 X 隔离出降级器自身机器码；含 ≤~20 B 度量脚手架。
4. **被降级的只是载荷逻辑层**（Q0 的 `logic.rs`）：adapters/内核保持 Q0 原样原生 substrate
   （规格 §1.2 已声明）。**OS 可达性中立性是 Q1 的问题，不是本实验的**——这也是 X **不含 OS seam** 的原因。
5. **④ `call` 子集沿用 Q0**：整数/指针字、≤11 参，未扩大。
6. **未加第五类原语**：降级器纯用 ①②，§1.1 的病灶未发作。记为发现。
7. **本次归拢新增的两条口径披露**（原文档缺，审计 N1/N2 点名）：
   ② 的 LOC 口径（非空非注释，已补复跑命令）与 ④ 的内核基线口径（重建，见上）。
8. **Windows 列的 X 是复用 Linux 数**（审计 N3）：③ 表里 Windows 格写「3003（ELF flat）」，
   **Windows 侧没有独立的 X 测量**，不要读成「两 OS 各测一次都得 3003」。

---

## 复跑命令

```powershell
# Windows：构建 6 个可执行产物 + 基线内核 + X 隔离 blob，并打印全部字节数
pwsh research/dynamic-core/lowering/build/build_lowering.ps1
# 该脚本自行打印：lower flat = 4089 B ; driver flat = 1086 B ; X = 3003 B

cd research/dynamic-core/lowering/out
[IO.File]::WriteAllText("$PWD\input.txt","dynamic-core experiment 2026-08-08`n")
.\A_lower_pure_windows.exe;  $LASTEXITCODE   # 163   (核内)
.\A_lower_rhp_windows.exe                     # a49d2cbecc13994f
.\A_lower_spawn_windows.exe; $LASTEXITCODE    # 打印 exit=07，退出码 7
.\B_lower_pure_windows.exe;  $LASTEXITCODE    # 163   (核外，双段)
.\B_lower_rhp_windows.exe                      # a49d2cbecc13994f
.\B_lower_spawn_windows.exe; $LASTEXITCODE     # 打印 exit=07，退出码 7
```

```sh
# Linux 产物（交叉编译；本机无 WSL，**只量字节、不执行**）
bash research/dynamic-core/lowering/build/build_lowering_linux.sh   # 打印 X + 全部字节数
```

```sh
# ② 的 LOC（非空非注释）
cd research/dynamic-core/lowering
sed -n '42,194p' lower.rs | grep -cvE '^\s*(//.*)?$'   # 139
grep -cvE '^\s*(//.*)?$' lower.rs                       # 301
```

**独立参考值（证明不是自证）**：FNV-1a/64 of 那 35 字节输入 = `a49d2cbecc13994f`
（Python：offset basis `0xcbf29ce484222325`，prime `0x100000001b3`），与 Q0 独立同值 →
降级出来的载荷不是与内核一起错的。

---

## 来源对账（本文件每一段的出处）

| 本文件的内容 | 来源 |
|---|---|
| ①②③④ 的全部数值、判决 trace、与规格不符 1–6 | 规格 [§8 实验结论回填](../../../plan/archive/design-lowering-cost-experiment.md)（2026-08-08 写入，提交 `9092acf4`） |
| ① 的 4089/1086、③ 的 3112+4881 | 规格 §8 逐字 |
| ④ 核内 6200/6360/6376、内核基线重建表 (a)(b)(b′) | `build/build_lowering{.ps1,_linux.sh}` 的产物 + 本机 `out/` 现存构建输出（`out/` 已 gitignore，须重跑上面的脚本复现） |
| ② 的行范围与 LOC 口径复算 | `lower.rs` 的 `[X86_64]`/`[SHARED]` 横幅 + 上面的 `grep -cvE` 命令 |
| 构建 flag、执行状态、基线内核怎么建的 | `build/build_lowering.ps1`（含「This is the ④ TCB baseline」注释）与 `build/build_lowering_linux.sh` |
| 判决摘要与 Reproduce | [`README.md`](./README.md) |
| 口径标签体例、S-F/U7/N1/N2/N3 的编号 | [`../COMPARABILITY.md`](../COMPARABILITY.md) |

**本次归拢没有产生任何新数字。** 唯一新增的是：② 的 LOC 口径声明与复算命令（N1）、
④ 内核基线的口径重建（N2，标记为重建）、以及每个产物的执行状态。

---

*研究轨投影（Q2）。不承诺版本归属，不改 PRD 能力状态。*
