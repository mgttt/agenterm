# COMPARABILITY — 度量口径可比性审计 (Q0–Q13)

**为什么有这份文件.** Q0–Q13 各自独立设计,各自定义了自己的度量口径。综合(synthesis)会把这些
数字**并排放进同一张表**——并排的前提是同口径。本文件逐个检查已经被并排使用的数字,判定它们
是否可比,并给出未来实验必须共用的那把尺。

**方法与纪律.** 不重新测量、不写实验代码。依据只有各 `RESULTS.md`(以及 Q2 的 `lowering/README.md`)
里**已经写下的口径说明**与它们指向的构建脚本/测量源文件。某个数字的口径若在文档里没写清,
本文件把它标为 **口径不明**,不推测它大概是什么。

审计范围:`research/dynamic-core/{,ir/,lowering/,reuse/,equiv/,isa/,platform/,tables/,interp/,primitives/,stencil/,landing/,declare/}`
+ `README.md` 的「技术清单」与「执行层判决」两节。(`policy/` = Q15,`Q14` 在跑,不在范围内,未触碰。)

---

## 0. 主表 — 每个体积数字的"边界"在哪

这是本次审计的核心资料。**同一行内的数字才可以并排。**

| # | 数字 | 出处 | 测量工具 / 口径 | 构建 | 目标 | 含 OS intent 层? | 含 panic/unwind 机制? | strip? | 该产物本身被执行过? |
|---|---|---|---|---|---|---|---|---|---|
| S-A | **3003 B** (X)、2777 B(jump-table 版)、+8% | Q2 `lowering/README.md` ① | **flat-PIC blob 相减**(两个只差 `lower.rs` 的 flat blob),code+data,无 ELF 头 | `-O -C panic=abort -C debuginfo=0 -C force-unwind-tables=no -C relocation-model=pic -C llvm-args=-min-jump-table-entries=200` + `ld.lld --oformat binary` | x86_64 **ELF/Linux** | **否** — env 表/Q0 adapters 在 `runner.rs`,README 明写 "not in X" | 否 (`panic=abort`, no_std) | 等价于 strip | **否**(执行的是 Windows PE 7680 B 那条) |
| S-A2 | **2883 B**(emit 2709 + lower_and_run 174) | Q10 ① 表(测的是 Q2 的产物) | `llvm-size -A` 逐函数求和,**仅 code** | 同上 | 同上 | 否 | 否 | — | 否 |
| S-B | **5826 B**、4515 B(code)、651 B(applier)、571/1210 B(stencil data) | Q10 ① | 5826 = **与 Q2 逐字相同的 flat 相减**(Q10 measurement conditions 明写 "identical to Q2 … directly comparable");4515/651 = `llvm-size -A` 仅 code | 同 S-A | 同 S-A | 否 | 否 | 等价 | 三载荷在 Windows 执行(PE 版);5826 本身是 ELF-flat 数 |
| S-C | **3177 B**(整个解释器)、**1908 B**(eval-core)、1269 B(seam)、**14819 B**(Q1 lowerer) | Q9 ② | `llvm-size` **Berkeley `.text`** of `--crate-type=lib --emit=obj` | **`-O` 仅此**(无 `panic=abort`)、用 `Vec<u8>`/std | **x86_64-pc-windows-msvc** | **3177 含**(`do_intent`+`spawn_wait` = 6 个 intent,含 spawn,1269 B);1908 不含(近似,见 Q9 deviation 3) | **是**(保留 panic/unwind 路径) | 否(object) | 解释器语义经 `driver.exe` 执行;3177 这个 object 本身不是被执行的产物 |
| S-D | **568 / 644 B**(四原语内核 x86/aarch64)、+76 B/+13% | Q5 ④ | `kernel/textsize.rs` 读 object `.text` | `-O -C panic=abort --crate-type staticlib --emit obj` | `{x86_64,aarch64}-unknown-linux-gnu` | n/a(只有四原语) | 否 | — | 否(aarch64 亦不可执行) |
| S-E | **550 → 732 B**,**+182 B .text**(Declare) | Q6 ③ | `llvm-objdump -h` 的 `.text` 段求和 | `-O --crate-type=lib --emit=obj`,**std harness** | msvc | n/a | 未声明 | 否 | 否(只测 Δ) |
| S-F | 内核 **~2.93 KB (L) / ~4 KB (W)**、TCB ~6.2–6.4 KB、总投递 6360/7816 | Q2 ③④ / Q0 | **整个 stripped 静态 ELF 文件**(kernel-only = 二进制 − 内嵌 blob) | Q0 `build_linux.sh`(`--strip-all -static`) | Linux ELF / Windows PE(512 对齐) | 含 ELF 头、entry bootstrap、mem* intrinsics、原语表 | 否 | 是 | Windows 侧执行,Linux 侧否 |
| S-G | **+609 B**(CA loader)、+1648 B(verify) | Q3 ③ | **整个 stripped ELF 相减**,且被减数自身是 `loader_embed − 841 B 内嵌 blob` 推导出来的 | 同 Q0 | Linux ELF | 含(loader 通过 ③④ 读 store) | 否 | 是 | 否(Windows 因 PE 512 对齐无法给出该数) |
| S-H | 发射码字节:281/1046/1249/1251/1557(x86)、192/660/728/840/956(a64)、1216(Q7 win rhp) | Q1 ①③ / Q5 ① / Q7 ③ | **运行时发射的 code image 原始字节**(`out/*.bin`),无 entry wrapper、无 panic handler | n/a(不是编译产物) | x86_64 / aarch64 | 含(intent 区就在里面) | 否 | n/a | Win64 侧执行;SysV/aarch64 仅字节测量 |
| S-I | 基线 blob 166/1128/856 | Q0 变体 B payload blob | flat blob 整文件 | Q0 构建 | sysv64 ELF | 含 | **含 entry trampoline + panic handler** | 是 | 否 |

**一眼可见的结论:表里存在 5 种互不相通的体积口径**——flat-blob 相减(S-A/S-B)、object `.text`
(S-C/S-D/S-E)、整个 stripped 二进制(S-F/S-G)、运行时发射码(S-H)、含 entry+panic 的 blob(S-I)。
另外还有一条正交轴:**no_std + `panic=abort`** vs **std + 默认 panic**。

**轨内自证这条轴有多大:** 同为"x86-64 IR→原生降级器"这一类产物,
Q1 的 lowerer 在 S-C 口径下是 **14819 B**,Q2 的 lowerer 在 S-A 口径下是 **3003 B**——**相差 4.9×**。
两者实现不同(Q1 多 6 个 intent 与 spawn 结构建造,Q2 是 24-opcode 通用 CALL),所以这 4.9× 里
有多少是设计、有多少是口径,**无法从现有文档拆开**。但它足以证明:**跨这两个口径比较字节数是不成立的。**

---

## 1. 可比的 — 可以并排,附口径

| # | 可并排的数字 | 共同口径 | 说明 |
|---|---|---|---|
| C1 | **Q2 3003 B ↔ Q10 5826 B(1.94×)**;Q2 2883 B ↔ Q10 4515 B(1.57×);Q2 TCB ~6.2 KB ↔ Q10 ~8.7 KB | S-A/S-B 完全相同(Q10 measurement conditions 明确声明"与 Q2 逐字相同,故 X_total 直接可比") | **全轨口径纪律的正面样板。**「stencil 比 Q2 大 1.94×」成立。 |
| C2 | Q2 in-kernel PE 7680 ↔ Q10 10752/11264/11776 | 同为 Windows PE(512 对齐)整文件 | 趋势确认,可比。 |
| C3 | **Q5 内核 568 ↔ 644 B(+13%)** | S-D,**同一份 `prim.rs`、同一命令、只换 `--target`** | 全轨最干净的一个 Δ。 |
| C4 | **Q6 kernel4 550 ↔ kernel5 732(+182 B)** | S-E,同一文件对、同一命令 | Δ 本身干净;**跨实验的百分比表述不干净**(见 E7)。 |
| C5 | Q9 **1908 ↔ 3177 ↔ 14819 B**(21%、4.7×) | 全部 S-C,同一命令、同一份 IR、同一构建 | Q9 内部一致。「解释器 = Q1 降级器的 21%」在 **Q9 口径内**成立。 |
| C6 | Q1 x86 发射码 ↔ Q5 aarch64 发射码 ↔ Q7 表驱动发射码 | 全部 S-H,同一 IR、同一批载荷、同为 naive stack-slot | 281/192、1249/1216 等并排成立。 |
| C7 | Q1 ③ 169/93/111/146/182% ↔ Q5 ⑥ 59/65/98/112/116% | 分子同为 S-H,**分母是同一组 Q0 sysv64 blob** | 两组比率**彼此可比**;但都不是"中立化的真实代价"(见 U3)。 |
| C8 | Q4 的 0% / 29.3–40.8% / 44.8–55.6% | 分子分母同为 S-H 的同一份 image,按 region 切分 | 内部完全自洽。 |
| C9 | Q3 baked 1274/1912 ↔ CA 1058/1378;+609/+1648 B | 同为 S-G/整 blob,同一构建 | Q3 内部一致;Linux 是干净数(Windows 被 PE 对齐糊掉,Q3 自己说了)。 |
| C10 | Q13 ~18–38 LOC/fact ↔ Q6 +182 B / 0 B 的两种形态 | **不是同类量**,但 Q13 明确报"kernel bytes = 0 + payload LOC",不是拿 LOC 冒充 bytes | 合规:它比较的是**位置**(kernel-in vs kernel-out),不是大小。 |
| C11 | LOC 三向拆分 shared 238 / per-ISA 307–350 / per-target 99–137(Q5)、Q1 350+246、Q9 55/81/136 | 全部 "非空行非注释行",且 **Q9 显式复算 Q1 的 202/148/137 并对上** | **本轨最佳实践**。LOC 轴是目前唯一被交叉校准过的轴。 |
| C12 | Q9 77×(vs 优化原生)与 5×(vs Q1 naive 降级码) | 同一载荷、同一机器、同一 reps | **双基线是好实践**;两个数都印。 |

**关于双基线的抽查(任务点 3):** 只有 Q9 明确用了双基线。其余倍数类都是单基线,但**都把基线写出来了**:
Q10 1.94×/1.57× 基线是 Q2 同口径数;Q5 +13% 基线是 x86 同文件;Q2 +8% 基线是 jump-table 版自身。
**没有发现"偷偷只报有利那个基线"的情形。** 唯一接近的是 Q1 ③/Q5 ⑥ 的 200% 天花板(见 U3)。

---

## 2. 不可比的 — 不能并排,以及正确的比较是什么

| # | 被并排的数字 | 为什么不可比 | 正确的比较应该是什么 |
|---|---|---|---|
| **U1** | **Q9 解释器 3177 B ↔ Q2 降级器 3003 B**(触发本次审计的那条) | **三重口径错位**:(a) **3177 含 OS intent 层**(`do_intent`+`spawn_wait` = 1269 B,6 个 intent 含 spawn),**3003 不含任何 OS 层**(Q2 README 明写 env 表/adapters "not in X");(b) 3177 是 **object `.text`**,3003 是 **flat blob code+data 相减**;(c) 3177 是 **std + 默认 panic**,3003 是 **no_std + `panic=abort` + strip**。轨内证据表明 (c) 单独就能造成数倍差异(14819 vs 3003)。 | **现有文档无法给出一个成立的比较。** 最接近同边界的一对是 **Q9 eval-core 1908 B ↔ Q2 2883 B**(两者都不含 OS 层),但仍跨 (b)(c) 两轴,**仍不可直接相除**。结论:**单 ISA 体积轴目前是"未测定",不是"解释器不更小"。** 要判它,必须把 `interp.rs` 按 Q2 的 flat-blob 口径(no_std/`panic=abort`/flat 相减)重测一次,并声明含不含 seam。 |
| **U2** | **Q10 5826 B ↔ Q9 3177 B**(「stencil 比解释器还大」) | 5826 = S-B(flat, no_std),3177 = S-C(object `.text`, std, 含 OS seam)。同 U1 的 (b)(c),且方向相反地含/不含 OS 层。 | 只能说 **stencil 5826 B > Q2 3003 B**(C1,成立)。与 Q9 的大小关系**未测定**。 |
| **U3** | Q1 ③ 的 169%/146%/182% 与 Q5 ⑥ 的 59–116%,作为"中立化的代价" | 分子 = 纯 code image(**无 entry wrapper、无 panic handler**),分母 = Q0 变体 B blob(**含 entry trampoline + panic handler**);且分子是 naive stack-slot,分母是优化 `rustc` 输出。Q1 自己在 ③ 下面列了 (a)(b)(c) 三条 caveat,Q5 deviation 2 也记了跨 ISA 基线。 | 比率彼此可比(C7),但"**没有触及 200% 天花板**"这句判决**必须带着这三条 caveat**一起被引用。README board(Q1 行)与技术清单引用时**没有带**。 |
| **U4** | **Q7 编组器 70–112 LOC ↔ Q1 每 target 90–110 LOC** | **覆盖面不同**:Q7 的引擎是 **single-native-call 家族(5 个 intent)**,**`SpawnWait` 完全不在其中**(Q7 ⑤:"SpawnWait is absent from both tables → the marshaller cannot lower it");Q1 的 90–110 LOC/target **恰恰包含**了 spawn 的结构建造/fork 序列——即 Q7 排除掉的那部分。另外 Q7 的 **57–58 LOC/target 数据**在这句并排里被丢掉了。 | 正确形式:**在 single-call 家族内**,Q7 = 固定引擎 70–112 LOC + **每 target 57–58 LOC 数据**;Q1 = **每 target 90–110 LOC 代码**(且含 spawn)。斜率结论(+1 intent/+1 target 的**代码**增量 = 0)不受影响——那是 Q7 真正的产品,且是结构性验证过的。 |
| **U5** | Q9 eval-core **1908 B ↔ 四原语内核 568/644 B**(「内核量级」「3.4×」) | 1908 = S-C(msvc, std, object `.text`),568/644 = S-D(linux, no_std `panic=abort`, object `.text`)。跨 std/no_std 与跨 target。另外 **568 B 是 Q5 对一份重新转写的 `prim.rs` 的测量**,Q9 表里记作"Q0 四原语内核",Q0 自己的内核产物是 ~2738 B 整个 ELF(S-F)。 | 无同口径对照。"解释器是 KB 级"这句**结论**不依赖这个比值(1908 B 本身就说明了量级);**把"3.4×"和"内核量级"作为并排结论则不成立**。 |
| **U6** | Q6 **+182 B** 表述为"**≈+28% over Q5's 644 B**" | Δ 测在 550 B 的 msvc/std-harness 基线上(S-E),却除以另一实验的 644 B linux/no_std 内核(S-D)。 | 用 Q6 自己的形式:**550 → 732,+182 B,+33%**。跨实验的百分比应删。 |
| **U7** | Q2 **X ≈ 整个内核**(3003 B vs ~2.93 KB) | 3003 = flat blob 相减(**不含** ELF 头/entry/mem* intrinsics/原语表),2.93 KB = **整个 stripped ELF 文件**(**含**这些)。两侧包含的"非机制字节"完全不同。 | 同边界的说法应是:**X(3003 B)≈ 四原语核心(568 B)的 5.3×**,或**X 与内核二进制同量级**。"X ≈ 整个内核"这句在方向上偏保守(X 一侧不含开销),但它作为 in/out-kernel 判决的支点,应该注明两侧口径不同。 |
| **U8** | Q12 "unwind 硬化 ≈ **X 的 1%**" | 1% = 34/3003。但 **34 B 是每个"被生成的函数"运行时发射的 `RUNTIME_FUNCTION`+`UNWIND_INFO` 数据**,不是加到降级器二进制里的字节;而同时要加的 **~40 行编码器源码**会让 X 增长一个**未测量**的量。**把"发射产物的字节"与"降级器自身的字节"混在一个百分比里。** | 应拆两句:(a) 每个生成函数 +~34 B **发射数据**;(b) 降级器 X 增加 = ~40 LOC 编译后的字节数,**未测量**。ENDBR 的 "+4 B/entry + 1 行" 同理(+4 B 是发射字节)。 |
| **U9** | 「执行层判决」三路 size 列并排:1908/3177 ‖ 3003 ‖ 5826 | 第 2、3 格同口径(C1),第 1 格跨口径(U1)。**整列不是一把尺。** | 见 §3 已修正内容。 |
| **U10** | Q0 的 ①②④ 字节(Linux 无填充 vs Windows PE 512 对齐) | Q0 自己在 ④ 说明了 "Linux 是无填充、Windows 按 512 对齐,Δ 会向上取整到一个块"。Windows 侧 +512 B 的 Δ **是对齐块,不是真实代码增量**。 | 只有 **Linux 列**能读作代码增量(+208/+432);Windows 列只能读作"≤ 一个对齐块"。技术清单未引用 Windows 侧,合规。 |

---

## 3. 已经出错的 — 逐条点名(文档里已经写下的不当比较)

**E1 —「执行层判决」的"诚实体积注解"(README 原第 175–177 行)。** 原文:
"on *raw single-ISA* bytes the interpreter's 3177 B is **not** smaller than Q2's minimal 3003 B lowerer"。
**这句不成立**(U1):它把**含 OS intent 层、std 构建、object `.text`** 的 3177 与
**不含 OS 层、no_std+`panic=abort`、flat 相减**的 3003 并排。这不是"诚实的注解",而是一条
**基于不可比数字的、对本节结论不利方向的错误让步**。→ **本次已修正**(§4)。

**E2 —「执行层判决」四轴表的 size 列**混装三种口径(U9)。→ **本次已修正**(逐格标注口径 + 加一条口径注)。

**E3 —「执行层判决」JIT 行的 "needs ~1% hardening"**(U8):百分比的分子是**发射字节**,分母是
**降级器二进制**;同时要加的 ~40 LOC 对 X 的影响未测量。→ **本次已修正**(改为不给百分比,分列两项并标注未测)。

**E4 — 技术清单 ① stencil 行**:"Bigger than Q2's hand-written lowerer *and* than **Q9's interpreter (3177 B)**"。
前半成立(C1),**后半跨口径不成立**(U2)。同一句也出现在 `stencil/RESULTS.md` ⑤ "Cross-Q closer"
("bigger than Q2's compiler *and* bigger than Q9's interpreter"),以及**问题板 Q10 行**
("Bigger than both Q2's compiler *and* Q9's interpreter (3177 B)",本文件初版漏点)。
→ **已修正(2026-08-08 修正轮)**:三处与 Q9 的比较全部**降级为「该轴未测定」**,与 Q2 的那半
(同口径)原样保留。Q10 的判决不依赖被删的那半。

**E5 — 技术清单 ① interp 行**:"3177 B … kernel-magnitude (vs 568/644 B)";同源于
`interp/RESULTS.md` ② 表的"参考:Q0 四原语内核 568 / Q5 aarch64 644"两行与 "3.4×" 读法。
跨 std/no_std + 跨 target(U5),且 568 B **误标为 Q0** 的内核(实为 Q5 的 `prim.rs` 转写)。
→ **已修正(2026-08-08 修正轮)**:`interp/RESULTS.md` ② 表的两行参考值**划掉并加口径框**
(说明 568/644 的真实出处与两条不可比轴),"3.4×"与"内核量级"**降级为该轴未测定**;
README 的 Q9 板行同步改写。判决树分支不受影响——条件是"解释器是 KB 级",1908/3177 B 独立满足。
**同一处误标也出现在 `primitives/RESULTS.md`("vs Q0 568 B"),一并改正**(见 E7)。

**E6 — 技术清单 ④ table-driven marshalling 行**:"Engine = **70 LOC fixed** … vs Q1's
**~90–110 LOC/target** that grows per intent AND per target"。覆盖面不同(U4):Q7 引擎**根本不做 spawn**,
Q1 的 90–110 **包含 spawn**;且 Q7 的 57–58 LOC/target 数据被丢出了并排。
→ **已修正(2026-08-08 修正轮)**:`tables/RESULTS.md` ③ 加一个 ⚠️ 框 + 一张三列对照表
(固定成本 / 每目标 / 每 intent,并标出代码 vs 数据),判决 trace 第 4 条同步改写;
README ④ 表行改成"斜率先行、并排限定在 single-call 家族内"。**斜率结论原样保留**
(+1 intent / +1 same-ISA target = 0 引擎代码,结构性验证过)。

**E7 — `primitives/RESULTS.md` ③ 表**:"+182 B .text … **≈+28% over Q5's 644 B**"(U6)。
→ **已修正(2026-08-08 修正轮)**:跨实验百分比**删除**(不是修补),保留 "550→732, +182 B, +33%"
(同文件对、同命令、同 target,干净);表头 "vs Q0 568 B / Q5 644 B" 改为 "Δ",并加口径框说明
为什么不修而删。deviation 3 同步改正 568 B 的**出处误标**(Q5 的 `prim.rs`,不是 Q0 的内核),
并把 "550 ≈ 568 同量级" 降级为**量级 sanity note**,明说它不使两数可除。

**E8 — 结构性 provenance 缺陷(不是算错,是引用资格):** 技术清单开头写"每条 **[实测]** 都在
**该 Q 的 `RESULTS.md`** 里有复跑命令",但 **Q2 没有 `RESULTS.md`**(链接指向 `lowering/README.md`),
且 **Q2 在问题板上的状态仍是 "running"**——而 X=3003 B 已经作为**已判决的三条落地路线之一**
被写进「执行层判决」。另外 **3003 B 是 Linux/ELF-flat 数,该产物从未被执行**(执行的是 Windows PE 7680 B 那条)。
→ **已修正(2026-08-08 修正轮)**:
- 新建 [`lowering/RESULTS.md`](./lowering/RESULTS.md),内容**全部归拢自既有材料**
  (规格 §8 回填 + `lowering/README.md` + `build/build_lowering{.ps1,_linux.sh}` + `lower.rs` +
  提交 `9092acf4`),文末有《来源对账》逐段列出处。**未重新测量、未新增任何数字。**
- 问题板 Q2 状态 **running → decided**,并写入结论与全部口径限定。
- **执行状态逐产物标注**:6 个 Windows PE 产物**真机执行**;**X=3003 B 与 ③④ 的整个 Linux 列
  是交叉编译的字节测量,那些产物从未被执行**(度量脚手架 `mx_*_flat.bin` 按构造就不可运行)。
  README、技术清单 JIT 行、执行层判决 size 列三处同步标注。
- 「X ≈ 整个内核」按 U7 加**跨口径限定**(左侧 flat 相减不含 ELF 头/entry/mem* intrinsics/原语表,
  右侧整个 stripped 文件含);内核基线口径按 N2 标为**重建**(见下)。

**E9 — Q1 ③ / Q5 ⑥ 的 caveat 在向上引用时丢失**(U3):README board 与技术清单只引"never trips 200%"
与 "59–116%",不带 Q1 自己列的 (a)(b)(c) 三条。→ 建议引用时带一句"分子无 entry/panic,分母有"。

**E10 — 小瑕疵.** Q4 的 29.3% 在 README 与 Q4 verdict 里被写成 "~30–41%"(向上取整,方向不自利);
Q13 的 "~18–38 LOC/fact" 未计入 FACT 1 的 constructive setup(+11 LOC,Q13 表里有列)。两条都已在
各自 RESULTS 里可追溯,记为**披露充分的小瑕疵**,不需修正。

---

## 4. 「执行层判决」是否需要修正 — 需要,已改

**结论先说:该节的核心判决(解释是地板、JIT 是有条件加速器)不因口径问题而倒。**
它的四条腿里有三条是干净的:

- **ISA 轴**(0 vs 307–350 LOC/ISA):LOC 口径全轨交叉校准过(C11),**成立**。
- **第一关(ACG)**:Q8 实测三条路全断 1655,Q9/Q12 实测解释器在 ACG 下算对,**成立**。
- **第二关**:Q12 实测四道关卡 + 结构证明,**成立**。
- **单 ISA 体积轴**:**不成立——不是"解释器不更小",而是"未测定"**(U1/U2)。

原文其实已经写了"the inversion rests on ISA-scaling + the two platform gates, **not on single-ISA
byte count**"——所以修正**不改变判决**,只是把那条**基于不可比数字的错误让步**换成
"该轴未测定",并给 size 列加上口径标注。

**已做的三处精确改动**(仅限「执行层判决」节):

1. 四轴表 size 列:每格标出口径来源(S-C / S-A / S-B),并在表下加一条 **"size 列不是一把尺"** 的注。
2. JIT 行的 "needs ~1% hardening" → 拆成"发射字节 +4 B/entry、+~34 B/函数"与"降级器自身增长未测量"。
3. 段末的"诚实体积注解" → 改为"单 ISA 体积轴口径不可比,**未测定**",并指向本文件。

另外把 ISA 列的 "~307 LOC/ISA" 补成 "**307–350 LOC/ISA**"(Q5 实测两个 ISA 分别是 350/307,
只引小的那个不必要地弱化了自己的论点方向,但仍属单侧引用)。

---

## 5. 口径不明 — RESULTS 里根本没写清的

| # | 数字 | 不明之处 |
|---|---|---|
| ~~N1~~ **已查明** | Q2 "**162 shared / 139 x86-specific lines**"(以及 X 的 301 行) | ~~未声明是否去注释/空行~~ → **是「非空行、非注释行」,与全轨同口径,可与 Q1 350 / Q5 307 并排。** 依据:`lower.rs` 自带 `[X86_64]`(42–194 行)/`[SHARED]` 分段横幅,按该口径逐字复算出 **139 / 162 / 301** 三个已发表数字(命令写进 `lowering/RESULTS.md` ②)。**没有补数,只是把已存在的口径写出来。** |
| N2 **部分查明,标为「重建」** | Q2 "**minimal kernel baseline ≈ 2.93 KB (L) / ~4 KB (W)**" | **文档确实从未写它怎么测**(规格 §8 与 README 都只给数值)。本轮在构建脚本与产物里**重建**出唯一能逐字节对上的算法:**变体 B 产物 − 内嵌的降级器 blob** = 7640−4708 = **2932**(L)、9216−5258 = **3958**(W) —— 已发表的 "2.93 KB"/"3.96 KB" 由此而来。另有一个**相差 14 B** 的同义实例:专建的 `baseline_kernel_linux` 3112 − 内嵌 166 B 原生 blob = 2946(= Q0 kernel-only 2738 + spawn 的 +208,交叉吻合)。**规格 §8 的 "Windows ~3.96–4.10 KB" 这个区间正是两种算法并存的痕迹,而 README 那句 "≈2.93 KB (L) / ~4 KB (W)" 左边取一种、右边取另一种——两侧不是同一把尺。** 两种算法的口径都是 **S-F(整个 stripped 文件族)**,与 Q0 的 "binary − blob" 同法。→ 已写进 `lowering/RESULTS.md` ④ 并**明确标为重建、非文档记载**;in/out-kernel TCB 判决在 README 两处加了限定。**判决方向不变**(2932 与 2946 的 14 B 差、以及 X 一侧的口径偏保守,都改不动 "X ≈ 一个内核" 这个量级)。 |
| ~~N3~~ **已注明** | Q2 Windows 列 "**3003 (ELF flat)**" 与 Linux 列同值 | 表头是 "Windows (PE-aligned)",格内却写 "(ELF flat)" —— 即 Windows 侧**没有独立的 X 测量**,复用了 Linux 数。→ 已在 `lowering/README.md` 与 `lowering/RESULTS.md` 的该格里明写 "*no independent Windows measurement; the Linux number is reused*"。 |
| N4 | Q6 kernel4/kernel5 的构建是否 `panic=abort` / 是否 std | 复跑命令只有 `-O --crate-type=lib --emit=obj`;是 std harness(deviation 1 说是 "std harness"),但 `.text` 是否含 panic 路径未说明。影响 +182 B 与 S-D(568/644)的可比性。 |
| N5 | Q9 seam/core 拆分 1269/1908 是**近似** | Q9 deviation 3 已诚实标注(Call 分发胶水会在 core/seam 间小幅移动)。**这是披露充分的不明**,但意味着 "eval-core 1908 B" 不能当作精确的"不含 OS 层"数使用。 |
| N6 | Q1 "ABI placement ~20–30 / OS content ~90–110 LOC" | 是对 137/109 行文件的**估算拆分**,不是逐行统计(Q1 用了 "~")。被 Q7 当作基线并排时,应记住它是估算。 |
| ~~N7~~ **已注明** | Q12 "~40 行 + ~30/34 字节/函数" | 明写是**估算,时间盒内未实现**,但技术清单/执行层判决引用时未标"估算"。→ 技术清单该行的 provenance 标签已改为 **`[实测·真机执行；硬化代价为估算、未实现] Q12 ②④`**;执行层判决那格已由 E3 修正带上"not measured"。 |
| N8 | Q10 stencil data "**571 B(紧凑编码)vs 1210 B(Rust 实体化)**" | 5826 B 里到底计入哪一个,文档未直说(从 4515+1210≈5725 与 5826 的差可推是后者,但**没写**)。 |

---

## 6. 建议的统一口径 — 给后续实验(Q14/Q15/…)用的那把尺

### R-S 体积:任何字节数必须带一个四元口径标签

`口径 = {边界, 工具, 构建, 目标/执行}`

1. **边界(最重要).** 明写哪些模块在里面、哪些不在。**必须显式回答两个问题**:
   (a) **OS intent/seam 层在不在?** (b) **载荷/env 表/adapter 在不在?**
   这两条是本次审计发现的头号杀手(U1)。
2. **固定三档上报**,任何"这个技术多大"的问题都给三个数,而不是一个:
   - **L1 机制码**(纯机制,不含 OS 层、不含数据)
   - **L2 机制 + OS seam**(该路线要跑通真实载荷所必须的全部代码)
   - **L3 整个投递足迹**(code+data,flat 口径)
   Q10 已经自发这么做了(applier 651 / 整代码 4515 / code+data 5826),**把它定为全轨规范**。
3. **工具**:`llvm-size` Berkeley `.text` / `llvm-size -A` 逐函数 / flat-blob 相减 / 整个 stripped 文件
   —— **四选一并写出来,禁止跨工具相除**。
4. **构建**:凡是要与内核/blob 数并排的,一律 **`no_std` + `-O -C panic=abort -C debuginfo=0`**;
   std + 默认 panic 的数**只能与同类相比**。理由见 §0 末(轨内 4.9× 的证据)。
5. **目标/执行**:写清 ISA/OS,并写清**被测的那个产物本身是否被执行过**(Q2 的 3003 B 就不是)。

### R-C 比较卫生

- **任何跨 Q 的并排,必须先声明"两侧同口径"**;做不到就写 **"该轴未测定"**,不要用近似口径凑一个数。
- **表格的一列 = 一把尺**。某路线在该尺下没有数,写 "not measured in this口径",**不许拿别的尺的数填格**。
- 新实验若要与既有数比较,**优先复用被比较方的口径**(Q10 复用 Q2 口径,是全轨最佳实践,应作为强制要求)。

### R-L 行数

- 统一 **"非空行、非注释行"**,并写出统计命令。
- **强制交叉校准**:新实验必须用自己的方法复算**一个既有实验的文件**,把结果贴出来对上
  (Q9 复算 Q1 的 202/148/137 —— 直接把这条抄成规范)。
- 比较两份代码的 LOC 前,先声明**两者覆盖的能力集相同**;不同就按能力集分档报(U4 的教训)。

### R-R 倍数与比率

- **基线必须内联写出**;基线若不是 like-for-like(naive vs 优化、跨 ISA、含/不含 entry+panic),
  **报两个基线**(Q9 的 5×/77× 是范本)。
- **不得跨口径相除**。分子分母必须来自 §R-S 的同一四元标签。
- **不得把"发射产物字节"与"工具二进制字节"混进同一个比率**(U8)。

### R-P 百分比

- 写清分子/分母各是哪个产物;区间端点**如实取整**(29.3% 就写 29%,不写 30%)。
- 跨实验的百分比一律禁止;Δ 只除以**自己那次测量的基线**(U6)。

### R-E provenance:三分标签不够,补一个执行状态字段

技术清单声明只有 **[实测] / [转述未验] / [转述·一手查证]** 三类。抽查结果:
**没有发现该标 [转述未验] 却标了 [实测] 的行**(Linux/aarch64 那些确实靠开头的全轨 posture 注
"Linux/SysV 与全部 aarch64 产物是字节测量+编码器验证,**未执行**"兜住了,Q8/Q12 还各自附了可信度分层表)。
**但 [实测] 这个标签在承担两件事**:"在真机上跑过"与"在本轨交叉编译出来量了字节但从未运行"。
Q2 的 X=3003 B 是后者,却与 Q8 的 1655、Q13 的 [FIRE] 共用同一个标签。

**建议:每个 [实测] 数字补一个执行状态**——`执行验证` / `仅字节测量` / `编码器验证` / `结构推断`,
写在行内而不是靠文首的全局 posture 注。全局注是对的,但它让**单行被摘出来引用时丢失限定**——
本轨的数字正在被大量单行摘引(技术清单、执行层判决),这个风险是现实的。

---

## 7. 一句话总结

**可比 12 组,不可比 10 组,已写下的不当比较 10 条,口径不明 8 处。**

> **修正轮状态(2026-08-08,审计之后)** —— 本文件既是审计报告,也是工单;工单已执行完:
>
> | 条目 | 状态 |
> |---|---|
> | **E1 / E2 / E3**(「执行层判决」内) | 审计当轮已修正 |
> | **E4 / E5 / E6 / E7**(技术清单与各 RESULTS 内) | **已修正**——三条降级为「该轴未测定」/删除跨实验百分比,一条改写为同能力集下的正确并排;**四条的原判决方向均未改变** |
> | **E8**(Q2 无 RESULTS + 板上仍 running) | **已修正**——新建 `lowering/RESULTS.md`(纯归拢,零新数字)、状态落定 decided、逐产物执行状态、跨口径限定 |
> | **E9**(caveat 向上引用时丢失) | 部分——执行状态标签已上行内化,Q1 ③ 的 (a)(b)(c) 三条仍只在 Q1 内 |
> | **E10** | 披露充分,按原判不修 |
> | **N1** | **已查明**:非空非注释,可并排 |
> | **N2** | **部分查明**:口径**重建**出来并逐字节对上,但**文档从未记载**——按重建标注,并给判决加限定 |
> | **N3 / N7** | 已注明 |
> | **N4 / N5 / N6 / N8** | 仍为口径不明(各自 owner) |
> | §6 建议的统一口径 | **已提升为规范**,写进 `.claude/skills/decisive-experiment/SKILL.md` §2.5「度量纪律:一条轨共用一把尺」+ §6 的两条引用资格 |
>
> **修正后没有任何一条实验判决倒塌**:被撤回的四处全是**并排用的修辞**(倍数/量级/跨实验百分比),
> 它们支撑的结论(解释器是 KB 级、stencil 不划算、Declare 有 in-kernel 地板、表驱动斜率为 0)
> **各自都有同口径的直接证据**。真正被降级的只有一件:**单 ISA 体积轴 = 未测定**(E1/U1 已判)。
最危险的一类确实是体积:**全轨存在 5 种互不相通的体积口径 + 一条 std/no_std 正交轴**,
而 Q9↔Q2↔Q10 三条落地路线的体积恰好落在不同口径上。**「执行层判决」的判决方向不倒,
但它的 size 列此前不是一把尺。**
