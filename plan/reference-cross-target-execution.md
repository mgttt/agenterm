# 跨目标执行：技术空间参考

> ⚠️ **不是 AgenTerm 产品范围。** 这是动态核研究轨的**常驻参考资料**，不是一次性报告。
> 服务对象是 [`design-dynamic-core-experiment.md`](design-dynamic-core-experiment.md)、
> [`design-neutral-ir-experiment.md`](design-neutral-ir-experiment.md) 及其后续实验。
> 不进任何版本 plan，不改 `PRD.md` 能力状态。

| 字段 | 值 |
|------|-----|
| **日期** | 2026-08-08 |
| **角色** | 参考（reference），供后续实验反复引用；不设时间盒，随发现增补 |
| **回答的问题** | 「一份产物如何跨 ISA / OS / ABI / 布局跑起来」这个空间里，人类试过哪些手段、哪些死了、**为什么死** |
| **前置** | 上两篇 design 文档的 §0/§1（约束）与已判决结论（2 层、四原语） |
| **⚠ 可信度** | **本次调研 WebSearch / WebFetch 全程不可用**（`selected model (haiku)` 报错），本机亦无直连出口。**文中除个别标注 ✅查证 的条目外，全部来自模型知识，未经本次联网核对。** 见 §11 |

---

## 0. 怎么用这份文档

三种用法，按频率排：

1. **查一个手段**：§3–§8 按簇分节，每条固定五栏（机制 / 轴 / 体积 / 成败与根因 / 对我们的可用性）。
2. **判一个设计冲动**：先看 §2.7（中立 IR 失败根因排序）与 §9（与本轨架构的对照）。
   如果你正想往 IR 或内核里加东西，§2.7 大概率已经记录了这个加法上一次是怎么把项目做死的。
3. **找我们没想到的**：§10 是本文档的主观产出——**调研者判断我们漏掉的手段**，按该补的优先级排。

**纪律**：本文只收「与极小动态核的决策相关」的内容。教科书细节一律砍掉。
一条技术如果既不能用、也不能借鉴、也不构成警告，它就不该出现在这里。

---

## 1. 坐标系：不是「FFI/JIT/AOT」，是「轴 × 推迟时刻」

我们此前的调色板只有 FFI / JIT / AOT 三格，太窄的根因是**分类维度选错了**。
FFI/JIT/AOT 是三种*实现风格*，不是三个*位置*。真正的坐标系是二维的：

### 1.1 五根轴（比我们原来的两根多三根）

上两篇文档只显式承认两根轴：**ISA** 与 **OS**。历史证据表明至少有五根，
而且**死人最多的不是 ISA 轴**：

| 轴 | 具体是什么 | 谁在这根轴上死过 |
|---|---|---|
| **ISA** | 指令编码。CPU 用硬件解码，装入内存后无法「探测后适配」 | 相对最好解决；二进制翻译、AOT、JIT 都能解 |
| **OS** | syscall 号/语义、内核服务、装载器、**平台策略**（W^X、签名、entitlement） | WSL1、Lx86、QEMU-user 的 `linux-user/` |
| **ABI** | 调用约定：参数寄存器、影子空间、红区、被调用者保存集、varargs、返回位置 | **Apple Bitcode 的直接死因**；本轨实验已自曝（sysv64 统一 + 桥接） |
| **布局** | `sizeof` / 对齐 / 结构体字段偏移 / 位域 / 联合体 | ANDF 用 token 解过；eBPF CO-RE 现役解法；Objective-C 非脆弱 ivar、Swift resilience |
| **命名与链接** | 「我要调的那个东西**叫什么**、在**哪个库**里、**哪个版本**」 | **我们完全没有承认过这根轴**。见 §9.2 |

> **第五根轴是本文档的第一个结构性发现。** 原语 ③ 给了 `dlsym`/`GetProcAddress`，
> 但**符号名本身是目标专属事实**（`open` vs `CreateFileW`；`libc.so.6` vs `kernel32.dll`；
> glibc 的 `open@GLIBC_2.2.5` 版本符号）。一份「中立 IR」若在里面写死 `"CreateFileW"`，
> 它在 ISA/ABI/布局三根轴上都中立，却在命名轴上完全不中立——**而这正是我们的载荷现在的样子**。

### 1.2 推迟时刻谱（第二个维度）

每种手段的本质是：**把某根轴上的决定，推迟到哪个时刻**。越往下推得越晚，
中立性越强，代价（时间/体积/信任）越大。

```
① 源码编写时   ── 手写 #ifdef。零机制，最大人力
② 编译时       ── 常规交叉编译。产物 = 目标专属
③ 链接时       ── 静态链接/多目标同产物
④ 分发时       ── fat binary / APK ABI split / App Thinning
⑤ 安装时       ── ANDF installer、PNaCl translator、dex2oat、Rosetta 2 首次启动
⑥ 装载时       ── Slim Binaries、eBPF verifier+JIT、CO-RE 重定位、copy-and-patch
⑦ 首次执行/热点 ── FX!32、HP Aries、JIT 分层、Transmeta CMS
⑧ 每次调用     ── libffi / 原语 ④
⑨ 每条指令     ── 解释器、QEMU TCG（实为每基本块）
```

**我们的方案落在 ⑥ + ⑧**：载荷在装载时降级，OS 函数在每次调用时按签名描述分派。
这个位置是有历史同伴的（Slim Binaries、eBPF、ANDF 偏 ⑤）——**不是没人走过的路，
是走过、有的成了、有的死了**。

### 1.3 由此得到的判读规则

看任何一条历史技术，先问三句：

1. **它把哪根轴推到了哪个时刻？**
2. **它没碰的轴，是它自己解决了，还是它的宿主替它解决了？**
   （§3.7 会证明：几乎所有成功的二进制翻译**都不解 OS 轴，而是让宿主先原生化**。）
3. **它死的时候，死在技术上还是死在经济上？**（§2.7 会证明：**多数死在经济上**，
   而经济根因**不一定命中我们**——这是本文档对本轨最有价值的一条。）

### 1.4 速查索引（一屏）

| 手段 | 节 | 主轴 | 推迟到 | 结局 | 对我们 |
|---|---|---|---|---|---|
| UNCOL | §2.1 | ISA | — | **从未实现** | 张力在 M×N；**我们 M=1** |
| **ANDF / TDF token** | §2.2 | ISA+布局+OS | ⑤安装 | 死（**经济**） | **技术遗产可用，死因不适用** ⭐ |
| **Slim Binaries** | §2.3 | ISA | ⑥装载 | 技术成，商业败 | 与我们最像的成功先例 |
| **Apple Bitcode** | §2.4 | 几乎无 | ⑤安装 | 死（**设计**） | **根因命中我们当前 blob** ⚠ |
| PNaCl | §2.5 | ISA（布局靠钦定） | ⑤安装 | 死（分发政治） | 预答了「失去的表达力」 |
| C + autoconf / JVM / CIL / wasm / SPIR-V / EBC | §2.6 | 各异 | ⑤⑥ | **成功** | **赢家都「内部钦定 + 硬 FFI 边界」** ⭐ |
| QEMU TCG | §3.1 | ISA+OS | ⑨每块 | 成功（基础设施） | `linux-user/` = 「翻译 OS」的成本曲线 |
| Rosetta 2 | §3.2 | ISA | ⑤+⑦ | 成功（有时限） | **降级产物持久化** ⭐ |
| Transmeta CMS | §3.3 | ISA | ⑦热点 | 商业败 | 预热成本会以延迟尖峰暴露 |
| **FX!32** | §3.4 | ISA+ABI | ⑦热点 | 技术成，平台死 | **持久缓存 + 原生库 jacketing** ⭐ |
| Lx86 / QuickTransit | §3.6 | ISA+**OS** | ⑦ | 死 | **「禁止封装」的最强背书** |
| **ARM64EC** | §3.8 | **ABI** | ②编译 | **成功在役** | **我们的 sysv64 桥接被正名 + 明码标价** ⭐ |
| **eBPF ISA / 验证器** | §4.1 | ISA / 安全 | ⑥装载 | 成功 | **交集非并集**；验证器 20k 行不可负担 |
| **eBPF helper 冻结 / kfunc** | §4.1d | 命名 | ⑥ | 成功 | **封顶必须配逃生舱口；靠作用域限界** ⭐ |
| **BTF + CO-RE** | §4.1e | **布局** | ⑥装载 | 成功 | **「把布局挡在载荷外」的现役答案** ⭐⭐ |
| wasm AOT / WAMR / Pulley | §4.2 | ISA | ⑤⑥ | 成功 | **29.4 KB 经验地板**；AOT 比中立大 3–4× |
| fat binary / App Thinning | §5 | ISA | ④分发 | 成功 | **我们已在这一格且做对了**；无更优解 |
| **WSL1 / Wine / Linuxulator** | §6.1 | OS | ⑨ | 混合 | **「发接口、让你自带封装」才活得下来** ⭐ |
| Cosmopolitan / APE | §6.2 | OS+格式 | ⑥运行时探测 | 成功但是**封装** | 三样机制可单借；路线已排除 |
| **W^X / CET / BTI / PAC** | §7.1–7.2 | OS | — | **在收紧** | **原语 ①② 的地基在被侵蚀** ⚠⚠ |
| libffi 类模型 | §7.3 | ABI | ⑧每次调用 | 成功 | **出站不需可执行内存，只有 closure 需要** |
| **copy-and-patch** | §7.4 | ISA | ⑥装载 | 成功（CPython 3.13+） | **不发编译器却能生成原生码** ⭐⭐ |
| 解释 / 线索化 | §7.5 | ISA | ⑨每指令 | 成功 | **三个平台上唯一合法路径** ⭐⭐ |
| Futamura / 部分求值 | §8.1 | — | — | 大运行时 | **copy-and-patch 是它的 KB 级形态** |
| unikernel / exokernel | §8.2 | OS | — | 未统治 | 思想祖先 + 「没东西可跑」的警示 |
| Drawbridge PAL | §8.3 | OS | — | 部分成功 | **三队独立收敛到 30–50 条，不是 4 条** ⚠ |
| CHERI | §8.4 | ABI | — | 未主流 | **签名要能说「这是指针」**，现在免费 |
| PCC / TAL / SFI / StackMapTable | §8.5 | 安全 | ⑥ | 学术 / SFI 可用 | **小 TCB + 不可信代码：无便宜解** |

⭐ = 可直接借鉴；⚠ = 命中我们的风险。**「推迟到」列的编号见 §1.2。**

---

## 2. 中立中间表示：失败史（本文档最重要的一节）

> 可信度：本节全部未经本次联网核对。**年份、人名、比例数字一律按 §11 的等级读**。
> 但**根因分析**是结构性的，不依赖具体数字。

### 2.1 UNCOL（1958）—— 一个有名字的失败

| 栏 | 内容 |
|---|---|
| **机制** | **没有制品。这是最常被搞错的一点：UNCOL 从未被实现**，也从未有一份完成到能支撑 M+N 工具链的规格。它是一个**论证**，不是一个系统。不要写成「UNCOL 部署了然后失败了」 ✅ 高 |
| **谁/何时** | SHARE 通用语言特别委员会。奠基文献：Strong, Wegstein, Tritter, Olsztyn, Mock, Steel, *The Problem of Programming Communication with Changing Machines: A Proposed Solution*, CACM vol.1 (1958)，分两期刊出 ◐中（人名与 1958/CACM 无疑，期号页码需核）。Steel 后来写过 *UNCOL: The Myth and the Fact*（Annual Review in Automatic Programming vol.2, 1961）◐中 |
| **轴** | 志在 ISA 中立；1958 年 OS/ABI 尚未被概念化为可分离的轴。**布局是没说出口的杀手**：那年代的目标机在字长（36/48 位）、按字寻址 vs 按字符寻址、反码/原码/BCD 算术上全不一样——**根本不存在一个可供中立的公共数据模型** ✅ 高 |
| **成败** | 从未建成。名字以「**the UNCOL problem**」的形式活了下来，是编译器教材（Appel、Muchnick）里指代「通用 IR 反复失败」的标准词汇 |

**根因（本文档最想讲清楚的一条）**：

M×N → M+N 的论证**默认 IR 是一个中立的会合点。它不是，它是一个被争夺的点。**

- 前端只能丢弃**没有任何后端会需要**的信息；
- 后端只能消费**每个前端都能提供**的信息。

M 越大，「每个前端都能提供」的事实集越小（Fortran 的别名保证、COBOL 的十进制语义、
Lisp 的动态类型与 GC 根、后来的异常与协程）；N 越大，「够生成好代码」所需的事实集越大
（寄存器类、寻址模式、字长、字节序、陷阱语义）。**两个集合从相反方向逼近一个在 M、N 都大时为空的交集。**

**推论，也是对我们最要紧的那句**：**M+N 的节省只对编译器里*便宜*的那一半成立。**
词法/语法/降级是便宜的一半；**目标级代码质量不被 IR 共享，它无论如何都要复制 N 次**。
所以 UNCOL 的头条收益本来就小——这才是没人把它做完的原因。
**中立在两个端点上是可达的（发源码；发机器码），在中间地带退化。**

> **对我们**：见 §9.3(a)。**我们的 M = 1，UNCOL 张力的一半直接消失**——这是我们与这段历史最大的结构差异。

### 2.2 ANDF / TDF / TenDRA —— **与我们最像的先例**

| 栏 | 内容 |
|---|---|
| **机制** | ISV 发一份 ANDF 文件，各平台的 **installer** 在客户机上把它编成原生码。TDF 是一套带类型的树形二进制 IR。**关键性质：C→TDF 的 producer 不做 ABI 降级** ✅ 高：<br>· `sizeof(int)`、对齐、结构体字段偏移**在分发文件里不是常量**，而是 SHAPE/OFFSET 上的**符号表达式**，安装时求值；<br>· 整数类型是按**所需取值范围**指定的 `VARIETY`，不是按位宽——表示法由 installer 挑；<br>· 一个 **API**（POSIX、X11、厂商头文件）被表达成一组**留作未定义的 TOKEN**（`~stat`、`st_size` 的偏移、`EOF` 的值）。installer 提供一份由**该目标真实头文件与 ABI** 导出的 token 定义库，在安装时解析；<br>· TOKEN 是 TDF 的通用抽象/参数化机制（约等于 TDF sort 上的卫生宏），不是为 API 打的补丁；<br>· 目标条件构造（`#if sizeof(int)==4`）以 TDF 条件式存活到安装期、token 解析后才求值 |
| **谁/何时** | OSF（1988 成立，Unix 战争中的反 AT&T/Sun 阵营）约 1989 发 ANDF 的 RFT ◐中；约 1991 选中英国 DRA Malvern 的 TDF ◐中⚠。TDF 承自 RSRE 的 **Ten15**。OSF 1996 并入 The Open Group；技术以开源 **TenDRA** C/C++ 编译器形式存活 |
| **轴** | 中立了 **ISA + 布局 + （经 API token）一大片 OS 表面**。**没有**中立掉：OS/库表面的**长尾**——厂商扩展、ioctl、内联汇编、信号语义、故意做布局双关的 C（union、强转、线格式结构体）。也完全没解决 ISV 的真实成本：**逐平台 QA** |
| **体积** | TDF 紧凑，大致与同源目标码相当或更小 ○低⚠（无可信比例，宁可不写数字） |
| **成败** | 商业上 90 年代中期即死，ISV 采用率基本为零；技术上以 TenDRA 存活 |

**根因（技术 vs 政治的分野——这条分野对我们至关重要）**：

**技术上它基本是成立的。** TenDRA 编真实的 C，token/API 机制确实work。
**所以：ANDF 不构成「布局推迟行不通」的证据。** ◐中

失败是政治—经济性的，且结构值得写清楚：

1. **必须运行 installer 的那一方，激励是反的。** ANDF 的成本落在平台厂商身上
   （造、发、支持、还要调优一个不能丢自家原生编译器脸的安装期编译器），
   而收益归 ISV、并最终归**想离开该厂商平台的客户**。
   **应用可移植性会把硬件商品化。HP、IBM、DEC 被要求出钱拆自己的护城河。** ✅ 高（作为分析）
2. **问题在他们脚下蒸发了。** ANDF 的价值随 N（可行目标平台数）增长。1991→1996，
   N 塌缩了：COSE 与 Unix 战争终结，随后 Windows NT + 商品化 x86 把盒装市场压到一两个目标。
   **N→2 时「就发两份二进制」直接获胜**，整个 M×N 论证不再付账。
3. **联盟所有制是最弱的所有制。** 没有单一方拥有装载器，就没有单一方能单方面把它变成普遍事实。
4. **残余技术阻力（真实但次要）**：大应用的安装期编译延迟、代码质量不如厂商自己的 `-O`、
   调试故事差、C++ 前端来得晚、以及一长串「不干净」的真实世界 C。

> **必须同时记住的反向教训**：**ANDF 最大、最复杂的机器，恰恰就是给 OS 表面用的 API/token 系统——
> 而即便如此它也没能覆盖长尾。** 推迟是可行的；**预算会烧在长尾上**。
> 见 §9.4——我们打算借用 token 机制时，这条是价签。

### 2.3 Slim Binaries / Juice —— **与我们最像、且成功过的那个**

| 栏 | 内容 |
|---|---|
| **机制** | 分发一棵**压缩的抽象语法树**，装载时**按过程惰性**生成原生码。编码器在**已发射部分**上建一个自适应的子树抽象字典（概念上是 LZW 家族，但作用在带类型的 AST 节点与符号表上而非字节上），解码器同步重建同一字典，于是解码是一趟前向扫描、**解码本身就是 parse**，直接把可用的 IR 交给代码生成器 ◐中⚠（机制类别对，具体方案名须查 Franz 原论文） |
| **谁/何时** | Michael Franz，ETH Zürich 1994 博士论文 *Code-Generation On-the-Fly: A Key to Portable Software*（导师 Wirth）；在 Macintosh 上的 Oberon 系统里部署，**同一份 slim binary 同时跑 68k 与 PowerPC Mac —— 一次真实的 ISA 过渡，这是它的头条战果** ✅ 高。与 Thomas Kistler 在 UC Irvine 做 **Juice**（约 1996–97），浏览器插件形态，直接对标 Java applet |
| **轴** | 真正中立了 **ISA**。布局只在「一个编译器同时拥有两个目标的全部布局决定」这个平凡意义上中立。**完全没有**中立 OS 与 ABI——**它根本不需要**：一种语言（Oberon）、一个运行时、一个 OS、一个编译器厂商，**封闭世界**。✅ 高 **这是「它work了」旁边必须写上的限定** |
| **体积** | 明显小于同源原生目标码；常引 2–3 倍更小、Juice 文件小于等价 Java `.class` ○低⚠（不要印具体比例） |
| **装载代价** | Franz 的论证是**I/O 论证**：在 90 年代中期硬件上，从磁盘/网络读一个大原生二进制的墙钟时间，超过读一个小 AST **加上**为它生成代码；按过程惰性生成又把成本摊到整个会话。外加一个胖二进制永远没有的好处：**代码生成器知道它正跑在哪一款具体 CPU 上** |
| **成败** | **技术上被验证，商业上无关紧要** |

**根因（最深的那层）**：**Franz 解决的是编译问题；市场的问题是信任与分发，而 Java 解决的正是后两者。**
Java 1995 带来的是一份**有规格的 VM**、一个**字节码验证器**、一个沙箱、类 C 语法，加上 Sun 的分发力量。
**一棵来自不可信服务器的 AST 没有验证故事**——你没法便宜地证明任意 AST 是安全的；
而在浏览器语境里，安全（不是代码生成速度）就是全部产品。◐中（作为分析）

> **但「思想赢了，制品输了」**：HotSpot（1999）把「发一份可移植的高层格式、在客户端用运行时信息自适应编译」
> 变成了行业默认——**那就是 Franz 的论文，由一个带验证器的竞争者抵达。** ✅ 高
>
> Franz 本人的后续也说明他吸收了这条教训：**SafeTSA**（Amme, Dalton, von Ronne, Franz, PLDI 2001）——
> 一种基于 SSA 的移动代码格式，**设计成类型/内存安全由编码本身的构造保证，而非由一个独立验证器检查**。◐中
>
> **对我们**：见 §10 —— 「安全由构造保证」是我们唯一负担得起的验证路线。

### 2.4 Apple Bitcode —— **根因直接命中我们**

| 栏 | 内容 |
|---|---|
| **机制** | `-fembed-bitcode` 把 LLVM bitcode 放进 Mach-O 的专用 section；胖二进制里**每个架构 slice 各带一份 bitcode**。Apple 服务器再跑一次 LLVM 后端产出逐设备二进制。开发者实际感受到的后果：重编产生新 UUID，**dSYM 必须从 App Store Connect 重新下载才能符号化**；**依赖图里每一个静态库与三方 framework 也都必须带 bitcode**，于是把工具链版本约束传染给整个生态 ◐中 |
| **谁/何时** | Apple，Xcode 7 / iOS 9（2015）引入：iOS 可选，**watchOS 与 tvOS 强制**。**Xcode 14（2022）弃用** ✅ 高 |
| **轴** | **结构上几乎什么都没中立掉。** 只中立了「固定目标内的后端版本与调优」。**没有**中立 ISA、ABI、布局，**因为 Clang 在生成 IR *之前* 就把 C/Objective-C 的 ABI 降完了** ✅ 高：<br>· module 里带着固定的 `target triple` 与 `target datalayout`（指针宽度、字节序、逐类型对齐）；<br>· 聚合体传递已经降完——`byval`、`sret`、把小结构体强转成 `i64`/`[2 x i32]`/浮点对，全按目标调用约定；<br>· 结构体字段偏移、padding、`sizeof` 已经是常量；<br>· varargs 处理、目标专属 intrinsic 与内联 `asm` 已烘焙进去。<br>**因此 armv7 → arm64 从 bitcode 从来就不可能，Apple 也从未这样宣称** ✅ 高 |
| **体积** | 显著撑大提交时的 archive/IPA（bitcode 不下发到设备）。无可靠倍数 ○低 |
| **实际交付 vs 人们以为的** | **交付了**：用更新的编译器/新缓解措施重优化而无需开发者重新提交；参与 App Thinning。注意：**逐设备切片本来就不需要 bitcode**（切片从胖二进制 + asset catalog 即可）。<br>**唯一一次真实的重定目标战果**：watchOS **armv7k → arm64_32**（Apple Watch S4/S5 时代，约 2019）。技术上它可行而 armv7→arm64 不可行的原因是：**arm64_32 是 AArch64 上的 ILP32 ABI——指针仍是 32 位**，而指针宽度正是烘焙得最死的那一项 ◐中。**⚠ 但「Apple 确实用提交的 bitcode 重新生成了已有手表应用」这一条是广泛流传而未经核实的，本文档不把它当作事实** ○低⚠<br>**从未可能**：任何 ISA 家族或指针宽度变化、任何 OS 变化、任何 ABI 变化 |

**根因（两层）**：

- **表层**：成本/收益崩了。每一个依赖都得配合，符号化坏了，构建时间与 archive 变大，
  而交付的收益（后端代码质量的边际改善）对用户不可见。
- **深层，也是要写进我们墙上的那句**：
  > **编译器的内部 IR 不是分发格式，把它序列化并不能使它成为分发格式。**
  > LLVM IR 是**按设计就目标专属**的——它是 ABI 降级的**产物**，不是其输入；
  > 而 LLVM bitcode 是**有意流动**的格式（LLVM 只保证新读者能读旧 bitcode，**不做前向兼容承诺**）。
  > **Apple 是在一个其维护者明确拒绝作为契约的制品之上，维护一份线格式契约。** ✅ 高

> **一句话版**：*Bitcode 不是「不小心只降了一半」，它是**按设计就完整地做完了 ABI 降级**，
> 因为 Clang 的工作就是实现 C ABI。留着没降的只有指令选择——最便宜、最没价值的那部分。*
>
> **对我们**：见 §9.3(b)。**这条根因直接命中我们当前的载荷 blob。**

### 2.5 PNaCl / NaCl / PPAPI

| 栏 | 内容 |
|---|---|
| **机制** | NaCl：让不可信的**原生机器码**在浏览器里跑在一个 SFI（软件故障隔离）沙箱中。PNaCl：改发一个**冻结的 LLVM bitcode 子集**，由 Chrome 内的翻译器在安装时 AOT 编到 x86/x86-64/ARM。<br>**冻结子集**：`pnacl-freeze`/`pnacl-thaw` 加一串 **ABI 简化 pass**，把通用 LLVM IR 变成 "PNaCl bitcode"，再由一个 **ABI 验证器**拒绝子集外的东西。这些 pass 正是最有信息量的部分，且**与 TDF 恰好相反**：把 `byval`/`sret` 展开成显式指针传递 + memcpy；把 varargs 展开成显式缓冲；抹平/擦除具名结构体类型使聚合体变成字节数组 + 显式地址算术；legalize 到一个小的固定类型集（i1/i8/i16/i32/i64, float, double）；展开 `switch`、常量表达式与复杂链接；白名单一小组 intrinsic ◐中⚠<br>**关键：PNaCl 的中立是靠*规定*一个数据布局达成的，不是靠*推迟*。** 冻结 ABI 定死了小端 **ILP32** 模型（`le32-unknown-nacl`）——x86-64 与 ARM 上一律 32 位指针 ◐中。**这与 TDF 正相反**，是「定义一台抽象机」学派披着 LLVM 的外衣 |
| **SFI，逐 ISA** | x86-32 用**分段**（LDT limit），几乎免费——最初那个优雅的把戏。x86-64 与 ARM 没有可用分段，隔离改为**守卫区 + 显式掩码**：保留 4 GiB+守卫区并在 x86-64 上截断指针，ARM 上对 load/store 与间接跳转目标做掩码，外加保留寄存器。所有目标强制 **32 字节指令 bundle**——指令不得跨界、间接跳转目标必须对齐 bundle——这要付代码体积与流水线代价。常引开销 ~5%(x86-32) / ~10–25%(x86-64, ARM) ○低⚠（**形状**——x86-32 最便宜、其余明显更贵——是可信的） |
| **翻译器** | Chrome 内一个裁剪版 LLVM 后端，数十 MB 量级 ○低⚠；首次加载有翻译延迟，带磁盘缓存。Google 的应对是 **Subzero**，一个专门做快而简单翻译以压低 `-O0` 启动延迟的翻译器 ✅ 高（存在与动机） |
| **轴** | 中立了 **ISA**。布局靠**钦定**（处处 ILP32）中立，代价是放弃 64 位性能与 >4 GiB 地址空间。OS 靠**替换**中立：没有 syscall，一切走 **PPAPI/Pepper**。**它没有中立掉平台——它造了一个新平台** ✅ 高 |
| **成败** | 死。被 WebAssembly 取代（2015 宣布，2017 年 3 月四大引擎均出 MVP）。PPAPI 另有其死因：Chrome 独有的插件 API，无人愿实现，最大消费者是 Flash（2020-12 EOL），随后被移除 |

**根因排序（针对本条）**：**不是 IR，也不是沙箱。是分发政治，而驱动它的是 PNaCl 索要的平台表面之大。**

1. **单厂商平台 + 索要一整套平行 API 表面。** PNaCl 不只是一个 IR，它是**IR + PPAPI**——
   一整个第二 Web 平台（图形、音频、输入、文件），Mozilla / Apple / 微软得重新实现一遍，
   而换不来任何他们想要的东西。Mozilla 有 asm.js 并明确说不。
   **一个只有一个运行时接受的中立格式，不是中立分发，是多走了几步的私有格式。** ✅ 高
2. **WebAssembly 因为*复用宿主*而占位更好。** wasm 没有 PPAPI：它调出去用的是每家浏览器
   **已经在发、已经在维护**的 JS/Web API 表面。它对厂商的边际成本是一个代码生成后端，不是一个平台。
   **这就是为什么四个竞争者能达成一致。** ✅ 高
3. **IR 只排第三。** 冻结 LLVM IR 的一个子集是一条永久跑步机——你钉住的格式，其上游所有者保留改动权。
   wasm 决定做一个**独立格式并采用结构化控制流**（而非基本块 CFG），正是对这条的直接否定，
   也使验证变得便宜且单趟可完成。
4. **沙箱反而是成功的那部分。** 基于机器码的 SFI work 了，验证成本可接受；
   wasm 用**由构造保证**替代它，更便宜——但那是对一个已解决问题的精炼，不是否定。

### 2.6 成功的中立表示：它们凭什么

| | 装载器所有者 | 布局策略 | 格式里含 OS/ABI 吗 |
|---|---|---|---|
| **JVM** | Sun/Oracle | **钦定**（规格定死基本类型尺寸；偏移是运行时内部事） | 否 —— JNI 边界 |
| **CIL/.NET** | Microsoft | **钦定 + 推迟**（JIT 计算偏移；泛型是 reified 而非擦除） | 否 —— P/Invoke，且**显式钉住布局** |
| **wasm** | 浏览器厂商 | **钦定**（小端、线性内存、严格 IEEE754） | 否 —— imports / canonical ABI |
| **SPIR-V** | GPU 驱动厂商 | **声明**（接口处显式 `Offset`/`ArrayStride` 装饰） | 不适用 —— 域内没有 OS |
| **EBC** | 固件 | **推迟**（natural + constant 索引编码，见下） | 极小的固定协议表面 |
| **Slim binaries** | Franz 自己的 OS | 单编译器拥有全部目标 | 否 —— 封闭世界 |
| **ANDF** | **没有人**（要求对手来当） | **推迟**（token / 符号偏移） | **是 —— 试图吸收它** |
| **Bitcode** | Apple | **烘焙** | 是 —— 烘焙 |
| **PNaCl** | 仅 Chrome | **钦定**（le32） | 是 —— 用 PPAPI 替换了它 |

三条格外值得记的：

- **ANSI C 是历史上最成功的中立分发格式**（f2c、cfront、GHC 的 C 后端、Chicken/Gambit、
  Vala、Nim、Cython…）；**更广地说：发源码 + `configure` + `make` 才是 Unix 上真正赢了的
  架构中立分发格式——autoconf 就是那个打败了 ANDF 的 token 解析系统**。✅ 高（作为分析）
  它赢的公式：① 它像 TDF 一样推迟布局（`sizeof` 与偏移是*目标*编译器的活）；
  ② **每台机器上本来就有一个 installer，而且是因为与你的 IR 无关的理由**；
  ③ 由标准组织而非厂商拥有，谁也撤不掉它。
  作为 IR 它的败处：没有保证的尾调用、不能跳进作用域、没有一等的栈操作，
  于是异常/GC/协程要靠 trampoline、`setjmp`/`longjmp` 或 GHC 那个著名的巨型 switch。
- **GCC RTL / GIMPLE 是独立的对照实验** ◐中：RTL 显式按目标参数化（machine mode、硬寄存器号、
  machine description），一份 RTL dump 在其目标之外毫无意义；GIMPLE 结构上更中立，
  但类型早已带上前端给的目标尺寸，且 **GCC 的 LTO 流是版本锁定的、不是可移植制品**。
  **与 bitcode 同一教训，来自一个完全独立的代码库。**
- **EBC（EFI Byte Code）—— 名单上最被低估的一项，也是我们最近的活亲戚** ◐中（设计）/○低⚠（采用）：
  UEFI 定义了一台由固件解释的栈式 VM，好让**一份 PCI option-ROM 驱动**同时跑在 IA-32、x86-64、Itanium 上。
  **值得偷的是它的布局机制**：EBC 有一个等于宿主指针宽度的 "**natural**" 操作数尺寸，
  结构体偏移与数组下标编码成 **(natural 单位数, 常量字节数) 二元组**——
  一个偏移字面上就是 "*a* × sizeof(void\*) + *b* 字节"，由解释器在真实宿主上求值。
  **那就是 TDF 的符号偏移思想，压进了指令编码里，并且跑在过数亿台机器的规格中。**
  结局：规格里有、实践中薄——纯解释慢，厂商少发，Itanium（其主要动机）死了，
  ARM64 UEFI 从未把它放到中心，部分固件干脆去掉了解释器。
  **教训**：它work是因为域**极窄**（驱动 init 路径，性能无关）且装载器所有者（固件）**真有**多 ISA 问题。两个条件都很窄。
- **SPIR-V 是成功案例，而它的前身是punchline** ◐中：SPIR 1.x/2.0（2012–14，给 OpenCL）
  **字面上就是 LLVM IR + metadata**，它失败的原因与 Apple bitcode 一模一样——格式在标准脚下移动。
  SPIR-V 的定义性决定就是做一个**不依赖 LLVM 的独立 SSA 二进制格式**。
  它赢的原因，按重要性：① **消费者想要它**——GPU 厂商刻意不暴露稳定原生 ISA，
  每个驱动里本来就带一个编译器，中立 IR 对他们零额外成本且保护了他们改硬件的自由。
  **这与 ANDF 的处境正好相反：那边是要求 installer 所有者把自己商品化。**
  ② 域受限：没有 OS、没有 libc、没有 syscall、没有任意堆指针——**那条杀死了所有人的 OS/ABI 轴，在这个问题里根本不存在**。
  ③ 布局是**声明**的而非假定或推迟的。④ 规格 + 一致性测试 + 参考工具链单一所有者，且第一天就有版本与能力位。

### 2.7 横切根因：排序（本节的结论）

**没有单一根因。有两种截然不同的死法，把它们混为一谈是本主题最大的分析错误。**

- **A 类 ——「它从来就不中立」（设计失败）**：UNCOL、Apple Bitcode、SPIR 1.x。
  制品携带了下游任何一步都无法撤销的目标承诺，**所承诺的重定目标能力从第一天起在算术上就不可能**。
- **B 类 ——「它是中立的，但没人愿意扛」（经济失败）**：ANDF、PNaCl。**两者都work。**
  死是因为**被要求执行最终降级的那一方，没有持久的理由继续这样做**。

**交叉证据**（这条最能约束分析）：**ANDF 与 PNaCl 证明「中立可达但不充分」；
Bitcode 则证明「拥有装载器同样不充分」**——Apple 从上到下拥有链条上每一环，仍然砍了它，
因为该格式的中立太浅，不值那笔生态税。

#### 四个候选根因的排序

**#1 —— 「没有厂商有经济理由维持它」。** 四例皆有，在 ANDF 与 PNaCl 上是决定性的。
但这么说太被动。更准确的形式是一条候选清单上没有的规律：

> **(e) 一个中立分发格式能存活，当且仅当**：
> **执行最终降级的那一方，从执行这件事本身获得持续的、自利的收益，
> 且该收益不依赖于这个格式的使用者。**

逐例验证：GPU 厂商**无论如何都需要**驱动端编译器（SPIR-V ✓）；浏览器**无论如何都需要** JIT
（wasm ✓；PNaCl ✗，因为它另外索要一个平台）；JVM **就是** Sun 的产品（✓）；
固件**真有**多 ISA 问题（EBC ✓，弱）；Franz 拥有自己的 OS 与装载器（✓）；
Unix 厂商被要求出钱把自己商品化（ANDF ✗）；Apple 的收益在 armv7 消失后就很小了，
而成本由数千个三方库维护者承担（Bitcode ✗）。**这条规律能预测名单上每一个结局，
包括「只看所有权」会判错的那两个。**

**#2 —— 「真成本是 OS/ABI/库表面，不是 ISA」。最被低估、对我们最可操作。**
ISA 中立是简单的那 10%。证据：**赢家全都把 OS/ABI 从格式里彻底移走了**——
JVM（原生只经 JNI）、wasm（只经 imports/WASI/component-model canonical ABI）、
CIL（只经 P/Invoke，且要显式钉布局）、SPIR-V（没有 OS）、EBC（一小撮固件协议调用）。
**每一个赢家都把原生边界做成显式、狭窄、且有意难跨的。**
而 ANDF 是唯一正面进攻 OS 表面的项目，**把最大的一笔复杂度预算花在 token/API 系统上，仍然没能覆盖长尾**。

> ⚠ 需要修正我们此前的措辞：说「中立 IR 对 OS 轴什么都没做」是不对的——**ANDF 做了很多，还是输了。**
> 这个说法比原来的更强，也更让人清醒。

**#3 —— 「IR 从来不中立，它是为某人做完一半的降级」。** 不是最常见，但在它适用的地方最深，
且**对「复用现有编译器 IR」这个具体做法是致命的**。Bitcode、SPIR 1.x、GCC 的 LTO 流是同一个错误的三个独立实例。
一般化：
> **编译器的内部 IR 是一个程序的两半之间传递的数据结构，这两半对目标达成了共识。
> 分发格式是不曾达成共识的各方之间的契约。序列化前者不产生后者。**
> 而且「只要别用目标专属的部分」修不好它，因为目标专属性长在**类型系统与调用约定**里，不在一个可选附录里。

**#4 —— 「冻结在错误的抽象层级、无法演化」。** 真实，但通常是 #1 或 (e) 缺失的症状。
有受益所有者的格式演化得很好（wasm 加了 SIMD、线程、GC、尾调用、异常；SPIR-V 有能力位与版本；
CIL 在 v2 把泛型加进了格式）。PNaCl 的冻结是最清楚的真实实例，而**即使在那里，冻结之所以脆弱，
也是因为底层 IR 是别人的移动靶——即 (c) 归约为 (a)**。
设计含义小而便宜：**第一个字节起就把版本与能力协商建进去，(c) 基本就消失了。**

#### 从成功者表里掉出来的三条模式

1. **「单一所有者且同时拥有装载器」是必要而不充分的。** Apple 是反例。充分条件是 (e)。
2. **赢家压倒性地**在**钦定一台抽象机**，而不是**推迟给宿主的那台**。
   推迟（ANDF、EBC、CIL 的 JIT 那一半）要求你去建模**所有宿主变化的并集**——一个无界义务；
   钦定要求宿主来适应你——一个有界义务，**由宿主付，每宿主付一次**。
   > **钦定的价签是一条硬 FFI 边界，而每一个赢家都显式、可见地付了这笔钱。**
   > **这是本节对 `design-neutral-ir-experiment.md` 最有决策价值的一条。** 见 §9.4 与 §10.1。
3. **两个靠「推迟布局」成功的中立格式（EBC、CIL 的托管内部），都处在一个原生 ABI 从不需要被满足的域里**
   （固件 init 路径；托管到托管的调用）。**推迟只在围墙花园里存活。**

---

## 3. 二进制翻译

**共同形状**：把目标 ISA 的字节，在某个时刻翻译成宿主 ISA 的字节。
差别全在「哪个时刻」和「翻译结果存不存」。

> 可信度：本节全部来自模型知识，**未经本次联网核对**。年份与体积数字尤其需要复核。

### 3.1 QEMU TCG

| 栏 | 内容 |
|---|---|
| **机制** | 按基本块把客户机指令降到一套精简 IR（TCG ops），每个宿主一个后端发射原生码进代码缓存，块间直接链接；复杂操作（FP、MMU、部分 SIMD）回调编译好的 C helper |
| **轴** | user 模式：ISA + OS(syscall) + ABI + 布局；system 模式：ISA + 整机，OS 轴靠「跑真正的客户机内核」绕开 |
| **体积** | TCG 核心 + 一个宿主后端约 10^5 字节量级；`qemu-system-*` 的 10–30 MB 绝大部分是设备模型不是 TCG。每进程另有数十 MB 代码缓存 ○低 |
| **成败** | **成功**，但要看清成功的形状：它从不追求快，只追求可移植且免费；它活着是因为它是**别的东西的基础设施**（KVM 设备模型、CI 交叉测试、发行版 bootstrap），不是一个必须自证价值的产品 |
| **对我们** | **明确不适用**作为方案（体积差 4 个数量级）。但 `linux-user/syscall.c` 是**最有价值的反面证据**：它是 QEMU 最大、变更最频繁、bug 最多的文件之一，永远在追内核变化。**这就是「翻译 OS 语义」的真实成本曲线**，直接支撑我们 §1.2「禁止封装」的纪律 |

### 3.2 Apple Rosetta 1 / Rosetta 2

| 栏 | 内容 |
|---|---|
| **机制** | R1：授权 Transitive QuickTransit，纯动态 JIT + 代码缓存，不持久化。**R2：首次启动整体 AOT 翻译**并把结果缓存到磁盘，之后启动直接用；对动态生成的代码（JS/Java JIT、自修改码）保留 JIT 退路 |
| **轴** | **只有 ISA**。macOS 本体、dyld、全部框架都先原生移植了——翻译器从不碰 OS/ABI 表面 |
| **体积** | 翻译器本体 MB 量级；每个被翻译的 App 在 `/var/db/oah/` 下有一份等量级的原生副本 ◐中 |
| **成败** | **成功**，且是**有意时限的成功**。R1：10.4.4(2006) 上市，10.7 Lion(2011) 移除。R2：Apple 已宣告收窄（**macOS 26/27 之后仅保留窄兼容模式**——⚠此条是本次最需要联网核实的一项，**未经查证不得当作事实引用**） |
| **对我们** | **思路可借鉴，前提不成立**。R2 能赢的**四个条件我们一个都没有**：① 同一家控制硅片+OS+工具链+分发（Apple 为翻译器**在 CPU 里加了 per-thread TSO 内存序模式**，消掉了强→弱翻译最大的一笔屏障税）；② 只需解 ISA 轴，因为 OS 已原生化；③ AOT+持久缓存把翻译成本摊到零；④ 敢砍长尾（不支持 AVX，禁止混 ISA 进程）。**真正可迁移的只有第 ③ 条：把降级结果持久化，别每次重做**——这对我们的「装载时降级」是直接可用的优化 |

### 3.3 Transmeta Code Morphing Software

| 栏 | 内容 |
|---|---|
| **机制** | CPU 原生 ISA 是一套**没有任何软件面向它编译**的 VLIW；开机时把 CMS 加载进对 OS 不可见的一块 DRAM，解释 x86、profile、把热区翻译成调度好的 VLIW 存进翻译缓存。硬件配套：影子寄存器 + **commit/rollback**、**gated store buffer**、**内存别名检测**，使激进重排在 x86 精确异常语义下仍安全 |
| **轴** | **只有 ISA**。Windows/Linux 完全不改 |
| **体积** | CMS 数 MB 固件；占用约 16 MB 系统 DRAM ○低（两数字都需核实） |
| **成败** | **商业失败**（约 2005 退出芯片业务，2009 被 Novafora 收购后瓦解）。**根因不是翻译不work——它 work 了**：① 它的价值主张是**性能功耗比**，而 Intel 用常规原生 x86（Pentium M / Banias, 2003）直接在这个指标上打赢了，公司存在的理由当场蒸发；② 绝对性能平庸，且**不可预测**（冷码慢、跑分测的是预热、交互出现翻译尖峰）——「有时候快」比「一直还行」难卖得多；③ 无自有制程，永远落后一代 |
| **对我们** | **不适用**（需要自有硅片）。但 ② 是一条给我们的直接警告：**「装载时降级」也是一种预热成本，它会以延迟尖峰的形式暴露给用户**。R2 的持久缓存正是对这一条的解药 |

### 3.4 DEC FX!32（x86 Win32 应用跑在 Alpha NT 上）

| 栏 | 内容 |
|---|---|
| **机制** | **三件套**：① 解释器首次执行时跑 x86 并**同时采集 profile**——尤其是**间接跳转/调用的实际目标**，这是静态翻译永远发现不了的；② 空闲时后台优化翻译器把热区转成真正的 Alpha 码；③ **按映像持久化的翻译数据库**，越用越快。加上 **jacketing**：应用发出的 Win32 调用被重定向到**原生 Alpha 系统 DLL** |
| **轴** | ISA + 边界处的 ABI 桥接。OS 轴免费：NT 本来就原生跑在 Alpha 上，Win32 API 同一套 |
| **体积** | 未获可靠数字 ○低 |
| **成败** | **技术成功，平台死亡**。Compaq 1998 收购 DEC 后转投 Itanium，2001 把 Alpha IP 卖给 Intel；微软取消了 NT/Alpha（Win2000 的 Alpha 移植在发布前被砍）。**桥修好了，对岸没了** |
| **对我们** | **最值得学的一条**。它是现代一切方案的祖先：profile 引导 + 持久翻译缓存（→ Rosetta 2、Windows XTA cache）、空闲时翻译、**原生库 jacketing**（→ CHPE、ARM64EC）。可直接借鉴的两点：**(a) 把降级产物持久化并复用**；**(b) 「自己的代码翻译，平台的代码原生调用」——这正是我们原语 ③④ 的设计，FX!32 是它的历史背书**。经典引用：Chernoff et al., *FX!32: A Profile-Directed Binary Translator*, IEEE Micro 18(2), 1998 |

### 3.5 HP Aries（PA-RISC → IA-64, HP-UX）

| 栏 | 内容 |
|---|---|
| **机制** | 两层动态翻译：冷码走**快速解释器**并采 profile，热轨迹编译进代码缓存并再优化。完全透明——HP-UX 内核装载器识别 PA-RISC 可执行文件即静默套上 Aries |
| **轴** | ISA + 同一 OS 家族内的 syscall/ABI 垫片。OS 轴同样由「HP-UX 自己先移植了」解决 |
| **体积/性能** | 无可靠数字，**不要引用厂商的「接近原生」宣称** ○低 |
| **成败** | **窄角色内的技术成功**，随 Itanium 崩塌与 HP-UX 退场而终。与 FX!32 同一死法 |
| **对我们** | 唯一增量信息：**「解释冷码 + 只编译热码」是一个正当的省体积策略**。我们目前默认「全量降级」，但如果降级器是体积瓶颈，「解释 + 热点降级」是一个我们没考虑过的折中（见 §10） |

### 3.6 Transitive QuickTransit / IBM PowerVM Lx86 —— **最有分析价值的失败**

| 栏 | 内容 |
|---|---|
| **机制** | 唯一严肃的**通用多对多**动态翻译框架：可插拔 ISA 前端 + 后端，**外加一层 OS 调用翻译**，因此能跨 OS。落地实例：PPC/Mac→x86（即 Rosetta 1）、SPARC/Solaris→x86 Linux、MIPS/IRIX→Itanium、x86/Linux→POWER Linux。IBM 2008 收购 Transitive。**Lx86** 让未修改的 x86 Linux 应用跑在 POWER Linux 上，**必须随附一整套 x86 Linux 用户态（glibc 等）** |
| **轴** | ISA + **OS**（是这份名单里唯一真的去扛外来 OS 表面的） |
| **体积** | 翻译器 + **一整个外来发行版用户态** |
| **成败** | **失败（早期退市）。三条根因，第一条对我们最重要**：<br>① **它是唯一真去背外来 OS 表面的，而那个表面是无界的**——glibc 版本、`/proc` 内容、`ioctl` 结构体布局、`/sys`、LSB 打包、发行版认证。维护成本随**别人的**发布节奏增长，而你不控制那个节奏。<br>② **收购后的战略利益冲突**：Lx86 主要帮客户**避免**移植到 POWER，削弱 IBM 自己想要的原生生态。**由目的地平台厂商拥有的翻译器，结构上劣于原生移植**。<br>③ 性能天花板（常引 60–70% ○低）导致没有 ISV 愿意认证，它永远是迁移拐杖而非目标 |
| **对我们** | **这是「禁止封装」纪律的最强历史背书**。Lx86 之死 = 「谁去扛 OS 语义表面，谁就承担一条自己不控制的无界增长曲线」。我们的 §1.2 恰好禁止了这件事。**把这条写进任何一次想给内核加 POSIX 抽象的讨论里** |

### 3.7 Intel Houdini / Intel Bridge

| 栏 | 内容 |
|---|---|
| **机制** | 闭源 ARM→x86 动态翻译（`libhoudini.so` + 挂进 Android linker 的 ARM ELF 装载器），让只有 ARM `.so` 的 NDK 应用在 x86 Atom 安卓设备上跑；翻译后的 ARM 码经桩层 **thunk 进原生 x86 的 Bionic/libc/ART**——又是 jacketing。后续以 **Intel Bridge Technology** 形态用于 WSA（Windows Subsystem for Android, 2021）与 ChromeOS ARC |
| **轴** | ISA + ABI thunk。OS 轴免费（两边都是 Android/Linux） |
| **成败** | **两次死于平台经济**：Intel 退出手机 SoC（Broxton/SoFIA 2016 取消）；微软停掉 WSA（2024 宣布，2025 结束）。次要因素：更慢更费电；反作弊/DRM/证明系统会检测到翻译并拒绝 |
| **对我们** | 增量信息一条：**「运行环境会被检测并被拒绝」是一种真实存在的失败模式**。对一个「拿到二进制包就执行」的引擎，这类完整性证明机制（attestation）天然敌对。低优先级，但记下 |

### 3.8 Microsoft x86-on-ARM64、CHPE、**ARM64EC** —— ABI 轴的解法

**ARM64EC 是这份名单里唯一一条不在 ISA 轴上做文章的，因此对我们最有启发。**

| 栏 | 内容 |
|---|---|
| **机制** | ARM64EC = **真正的 ARM64 机器码，但遵守一套与 x64 二进制兼容的 ABI**。不是仿真，不是 ISA 特性，**是一套调用约定 + 寄存器状态映射**：<br>· 前四个整型参数映射到与 x64 的 RCX/RDX/R8/R9 一一对应的 ARM64 寄存器（x0–x3），浮点映到对应 XMM0–XMM3 的低 SIMD 寄存器，**同样的 32 字节 home space、同样的栈布局、同样的 varargs 规则**；<br>· **故意弃用一部分 ARM64 寄存器**（常引 x13/x14/x23/x24/x28 与 v16–v31 ○低，须核实），使 ARM64EC 的寄存器状态能干净地映射到仿真器能物化的 x64 状态；<br>· **结构体布局、对齐、类型尺寸走 x64 规则而非标准 ARM64 Windows 规则**，于是数据结构在 x64 与 ARM64EC 之间传递**完全不需要 marshalling**。<br>边界靠编译器生成的 **entry thunk / exit thunk** 双向穿越，函数指针与回调也走得通。**ARM64X** 则是一个 PE 文件同时含纯 ARM64 视图与 ARM64EC 视图，由装载器经 hybrid 重定位表选择。PE 头标 **AMD64** machine type + hybrid 元数据，好让 x64 时代的工具链仍然接受它 |
| **轴** | **ABI（主）+ 布局**。ISA 轴不解——它本来就是原生 ARM64 码 |
| **体积** | 弃用寄存器 + 每函数 thunk 的稳定开销；外加一整套并行工具链（微软的一次性投资） |
| **成败** | **成功且在役**（Win11 系统 DLL 即 ARM64X）。设计目标不是「过渡」而是**让部分迁移成为稳定状态** |
| **对我们** | **可直接借鉴，且是我们已有做法的正名**。本轨实验里内核做 `sysv64→win64` 桥接、载荷统一用 sysv64——那**不是 hack，是 ARM64EC 同款策略**：*选一个公共 ABI，把差异压到边界 thunk*。ARM64EC 同时给出了这条路的**明码标价**：① 你要放弃一部分寄存器（≈ 我们放弃一部分性能）；② 你要在每个跨界函数上付 thunk；③ **布局也必须跟着统一**（ARM64EC 连结构体布局都用 x64 规则——这直接说明**ABI 与布局两根轴无法分开推迟**，见 §9.3）。<br>另一条：**微软之所以需要 ARM64EC 而 Apple 不需要，是因为微软没有生态强制力**。Apple 能禁止混 ISA 进程并给两年死线；微软不能，所以只好把「部分迁移」写进 ABI。**我们更像微软**——我们的载荷要和一堆不由我们编译的平台代码在同一进程里共处 |

### 3.9 二进制翻译死因排行（跨案例归纳）

按「实际是死因」的频次排：

1. **平台经济，不是技术——遥遥领先。** FX!32、Aries、Lx86、Houdini、Intel Bridge
   **全都能work**，全都死于目的地平台或出资业务线消失。**翻译器是桥，桥只在有人想过河时才有人出钱。**
   这也解释了成功者的形状：Rosetta 1/2 明确设了时限，因为**活得比渡河更久的桥是纯成本**。
2. **ISA 是简单的那根轴；OS/ABI/库表面才是真成本。** 注意**每一个幸存者都是绕开而非解决**：
   FX!32 jacket 进原生 Win32 DLL；Rosetta 2 坐在已原生移植的 macOS 上；Houdini thunk 进原生 Bionic；
   CHPE/ARM64EC 让 OS 侧原生。真去吃 OS 表面的都付了代价（QEMU 的 `linux-user/`、Lx86 的整套外来用户态）。
   **指令解码是有界且可规格化的；syscall/库表面是无界的，且版本由别人定。**
3. **内存模型不匹配是头号技术杀手**（强→弱：x86 → ARM/POWER/Itanium）。
   解决它的都是**在硬件里解的**（Apple 的 per-thread TSO、Transmeta 的 gated store buffer + 别名检测 + commit/rollback）。
   这条硬约束决定了谁能赢：**你基本得拥有硅片**。
4. **自修改与动态生成的代码打穿一切基于缓存的设计**——JIT、加壳、安装器、反作弊、DRM。
   于是每个 AOT 设计都被迫保留 JIT 退路 + 写检测/页保护机制。这与 W^X 和代码签名策略叠加：
   **翻译器自己需要平台正在收回的 JIT 权限**（因此 iOS 上不存在 Rosetta 类层）。
5. **语义长尾**：EFLAGS、精确异常、非对齐访问、x87 80 位、以及 SIMD 扩展覆盖度。
   到「能跑我的应用」很快，到「逐 bug 一致」是进度表死的地方。
6. **成功是自我终结的。** 生态一旦重编译完，翻译器价值归零而维护与攻击面成本长存。
   反例是 ARM64EC（把部分迁移设计成稳定态）与 QEMU（目的永不完成）。

---

## 4. 验证型字节码 + 装载时 JIT

> **可信度例外**：本节与 §6 的绝大部分**已于 2026-08-08 对活的一手来源查证**
> （内核源码与文档、GitHub API、厂商文档）。这是全文档可信度最高的两节。
> 少数未查证项已单独标注。

### 4.1 eBPF —— 现役系统里与我们约束最接近的一个

#### (a) 指令集：**它不是「恰好好 JIT 的中立 IR」，它是两套硬件 ABI 的交集被提升成了 IR**

**这是本节最重要的洞察，而且内核文档里是明写的。** 引 `Documentation/bpf/classic_vs_extended.rst`（✅ 查证，逐字）：

> "Natively, x86_64 passes first 6 arguments in registers, aarch64/sparcv9/mips64 have 7 - 8
> registers for arguments; x86_64 has 6 callee saved registers, and aarch64/sparcv9/mips64
> have 11 or more callee saved registers.
> **Thus, all eBPF registers map one to one to HW registers on x86_64, aarch64, etc, and
> eBPF calling convention maps directly to ABIs used by the kernel on 64-bit architectures.**"

**11 个寄存器的算术**：5 个调用参数 + 1 个返回值 + 4 个被调用者保存 + 1 个只读帧指针。
**这个数字是从真实 64 位 ABI 的交集反推出来的，不是为优雅挑的。** 映射是真的（✅ 查证于当前 master）：

```
x86-64:  R0=RAX R1=RDI R2=RSI R3=RDX R4=RCX R5=R8 R6=RBX R7=R13 R8=R14 R9=R15 FP=RBP(ro)
arm64:   R0=x8  R1..R5=x0..x4   R6..R9=x19..x22   FP=x25
```

同一设计动作的第二个实例：**32 位子寄存器写入零扩展到 64 位**，因为
"that behavior maps directly to x86_64 and arm64 subregister definition,
**but makes other JITs more difficult**"。**32 位架构上 eBPF 只能走解释器。**

> **对我们的结论（改变判断的一条）**：
> **eBPF 故意在两个点上牺牲了中立性（11 寄存器、零扩展），换来的是「JIT 是一次查表而不是一个寄存器分配器」。**
> 我们的中立 IR 若坚持对所有目标一视同仁，将得不到这个好处。
> **「选定两三个主流 64 位 ABI 的交集作为 IR 的寄存器/调用模型，让边缘架构走解释器」是一条经过验证的、
> 我们没考虑过的路线**，且与 §9.4「内部钦定」完全一致。

#### (b) 验证器：它证明什么、代价多大

**机制**：① DAG 检查拒绝环与不可达指令；② 抽象解释遍历所有路径，
每寄存器/每栈槽维护 `bpf_reg_state`（类型 + 值域 `umin/umax/smin/smax` + tnum `var_off`）；
load/store 只允许在 `PTR_TO_CTX`/`PTR_TO_MAP`/`PTR_TO_STACK` 上并做边界与对齐检查；
路径爆炸靠**状态剪枝**（`states_equal()`）与活跃性跟踪控制。

**它不证明终止性——它是在*模拟*，靠预算兜底。** 当前常量（✅ 查证于 master）：

| 常量 | 值 |
|---|---|
| `BPF_COMPLEXITY_LIMIT_INSNS` | **1,000,000**（注释：`/* yes. 1M insns */`） |
| `BPF_COMPLEXITY_LIMIT_JMP_SEQ` | 8192 |
| `BPF_COMPLEXITY_LIMIT_STATES` | 64 |
| `MAX_TAIL_CALL_CNT` | 33 |

**4096 → 1M 的历史**（✅ 查证到 commit）：4096 是最初的程序尺寸上限；128k 是 `8e17c1b16277`（2017-08-07）；
**1M 是 `c04c0d2b968a`（2019-04-02，Linux 5.2）**；**有界循环是 `2589726d12a1`（2019-06-15，5.3）**，在此之前大家用 `#pragma unroll`。
1M 那条 commit message 值得整段记住：

> "**4k limit was confusing to users**, since small programs with hundreds of insns could be hitting
> BPF_COMPLEXITY_LIMIT_INSNS limit. Sometimes **adding more insns and bpf_trace_printk debug statements
> would make the verifier accept the program while removing code would make the verifier reject it.**"

用户为此发明的迷信解法：二分查找一个 `#define MAX_FOO` 直到能装载；把一个程序拆到 32 个 tail-call 槽里买预算。

> **设计教训**：**静态预算是一个用户可见的、非组合的性质。**
> 程序会因为与自身正确性无关的理由被拒。**我们若给动态核加任何验证预算，它一定会以同样的方式
> 渗进载荷作者的心智模型。**

#### (c) JIT 与解释器的体积（✅ 查证）

- x86-64 JIT：`arch/x86/net/bpf_jit_comp.c` = **4,223 行 / 119 KB 源码**；arm64 = 3,249 行 / 90 KB。
  **量级：一个 1:1 映射的 ISA 的逐架构 JIT 是约 4k 行，不是约 40k 行。这就是 (a) 的红利。**
- 解释器：`kernel/bpf/core.c` = 3,593 行；在 `CONFIG_BPF_JIT_ALWAYS_ON` 下**整个被编译掉**。
- **关键耦合**：`jit_needed = IS_ENABLED(CONFIG_BPF_JIT_ALWAYS_ON) || bpf_prog_has_kfunc_call(fp)`
  ——**一个调用 kfunc 的程序根本无法被解释执行**。
  > **对我们**：**「可达机制」会静默约束「执行引擎」。** 如果我们把某些能力做成只能走原生调用，
  > 就等于宣布这些载荷不能走解释退路——而 §7.1 说明解释退路在 iOS/ACG/OpenBSD 上是唯一的路。

#### (d) helper / kfunc：**这是不是封装膨胀的失败模式？**

**本节最具决策价值的发现，而且答案毫不含糊。** `include/uapi/linux/bpf.h` 里 helper #211 之后紧跟着（✅ 逐字查证）：

```c
FN(cgrp_storage_delete, 211, ##ctx)                             \
/* This helper list is effectively frozen. If you are trying to  \
 * add a new helper, you should add a kfunc instead which has    \
 * less stability guarantees. See Documentation/bpf/kfuncs.rst   \
 */
```

- **211 个 helper，已封顶。** 最后两个落在 Linux 6.2。**有 UAPI 稳定性保证的可达表面停止增长了——
  是被写进头文件的政策明令停下的。**
- **kfunc**："do not have a stable interface and can change from one kernel release to another…
  BPF programs need to be updated in response to changes in the kernel"，可见性**按程序类型作用域化**。
- **数量**：docs.ebpf.io 站点地图今日为 **319 个 kfunc 页 vs 212 个 helper 页**——**kfunc 已经是 helper 的约 1.5 倍且在自由增长。**

> **判决：eBPF 并没有避免接口的无界增长，它把增长*重新分类*了。**
> 增长被从「有稳定性保证的表面」（冻结在 211）驱逐，重新安置到一个
> **显式不稳定、按上下文作用域、带版本**的表面（319+ kfunc）上，
> 而其代价由**装载时的 BTF/CO-RE 重定位**支付，而不是由一个永久 UAPI 义务支付。
>
> **三条直接转移到我们身上的推论**：
> 1. **封顶一个稳定接口是可行的，但只能在同时提供一个不稳定逃生舱口的前提下。历史记录里没有第三条路。**
> 2. **没有任何一个载荷看得见整个表面。** 按类型作用域化使任一载荷面对的**有效**接口保持很小。
>    **界限靠作用域达成，不靠计数达成。**
> 3. **不稳定表面的自由是用可解释性换来的**（kfunc ⇒ 必须 JIT）。**晚绑定的可达性不是免费的。**

#### (e) BTF + CO-RE —— **「把布局挡在载荷之外」的现役答案**

**这是我们最强的先例。** 问题陈述（Nakryiko, *BPF Portability and CO-RE*, 2020，✅ 查证逐字）：

> "**BPF programs do not control memory layout of a surrounding kernel environment. They have to
> work with what they get from independently developed, compiled, and deployed kernels.**"

**机制四段，其中只有三段在载荷路径上**：

1. **BTF** —— 一种紧凑的 C 类型描述格式，经 dedup 相对 DWARF **最多缩小约 100×**。
   便宜到内核可以在运行时自带自己的类型信息：`CONFIG_DEBUG_INFO_BTF=y` → `/sys/kernel/btf/vmlinux`。
   **这就是布局神谕，而它住在宿主上，不在载荷里。**
2. **Clang 内建**发出**记录意图而非偏移**的重定位：
   "If you were going to access `task_struct->pid` field, Clang would record that it was exactly a
   field named `"pid"` of type `pid_t` residing within a `struct task_struct`."
   重定位种类：字段偏移、字段存在性、字段尺寸、位域，后来又加了类型与枚举值。
3. **libbpf 在装载时**（**用户态，不是内核**）把程序的 BTF+重定位与运行中内核的 BTF 匹配，
   把偏移/立即数**修补进指令流**。
4. **内核对 CO-RE 一无所知**："to kernel it looks like any other valid BPF program code.
   It is indistinguishable from a BPF program compiled right there on the host."

**它替代了什么**：BCC 的做法是**把编译器发出去**——把 C 源码作为字符串嵌入、随身带 Clang/LLVM、
拉本地内核头文件、在目标上编译。公开列出的缺点："big fat binaries"、
"on a busy host, compiling a small BPF program might take minutes"、必须装 `kernel-devel`、错误只在运行时暴露。

> **可转移的规则（一句话）** ✅ 查证：
> **载荷携带「名字 + 类型描述 + 重定位记录」；装载器携带「权威的布局神谕」；偏移在装载时计算，从不随载荷发出。**
>
> **诚实的局限**：BTF 记录**类型，不记录 `#define` 宏**——**常量仍然从另一个机制漏出去**。
> （Cosmopolitan 撞上了同一堵墙，§6.3。）

#### (f) 用户态 eBPF 运行时：**「安全」与「小」只能二选一**

✅ 查证（源码尺寸；**编译后二进制尺寸未查证**）：
`ubpf` —— `ubpf_vm.c` 93 KB、`ubpf_jit_x86_64.c` 94 KB、`ubpf_jit_arm64.c` 69 KB。
`rbpf`（Rust）—— `interpreter.rs` 26 KB、`jit.rs` 45 KB、**`verifier.rs` 只有 13 KB**。

> **`verifier.rs` 只有 13 KB 就是那个 tell**：用户态 eBPF 运行时**根本不重新实现内核验证器**，只做最低限度的 sanity check。
> 内核 `kernel/bpf/verifier.c` = **20,065 行**（✅ 查证），不可移植、不可复用、不是一个几 KB 的核能背的东西。
> **想要 eBPF 的*安全*，就得不到 eBPF 的*体积*。只能选一个。**（与 §8.5 同结论，两条独立路径。）

### 4.2 WebAssembly 的 AOT 路线 —— **量到了这个空间的经验地板**

| 项 | 数字 | 可信度 |
|---|---|---|
| **WAMR AOT 运行时** | **约 29.4 KB**（cortex-m4f，bloaty 量 text） | ✅ 查证（README） |
| WAMR fast interp / classic interp | 约 58.9 KB / 56.3 KB | ✅ 查证 |
| WAMR libc-wasi / libc-builtin | 约 21.4 KB / 3.7 KB | ✅ 查证 |
| Wasmtime 最小嵌入（Wasefire） | **256 KB RAM + 约 300 KB flash** | ✅ 查证 |
| **Component Model canonical ABI 规格** | `CanonicalABI.md` = **5,411 行** | ✅ 查证 |

**Wasmtime 的 AOT（`.cwasm`）给出一个必须记住的反直觉事实**（✅ 查证，Rust hello world）：

| 构建 | `.wasm` | `.cwasm` |
|---|--:|--:|
| 默认（含 DWARF） | 2.5 M | 284 K |
| `-Cstrip=debuginfo` | 78 K | 284 K |
| 全优化 | 64 K | 219 K |
| 极简 + `--target pulley64` | 50 K | 90 K |

官方原话："**compiled `*.cwasm` files are often larger than their corresponding `*.wasm` file.
This is expected and generally always going to be the case.**"

> **两条对我们的结论**：
> 1. **AOT 产物比中立产物大约 3–4×，而且 `.cwasm` 要带 `--target`。中立性在编译期就花光了，
>    你又回到「发 N 份变体」。** 这与 §5 的结论一致，也是对「降级产物持久化」这条优化的价签。
> 2. **Wasmtime 自己造了 Pulley——一个可作为编译目标的可移植解释器**，而且它的产物**更小**。
>    **他们在 Cranelift *下面* 重新引入了一个中立字节码，因为原生码不总是对的权衡。**
>    这是对 §7.5「解释是调色板里缺的那一格」的独立背书。
> 3. **`CanonicalABI.md` 5,411 行，就是「把一个真正语言中立的 ABI 老实写下来」的价格**，
>    外加每次跨组件调用的**复制**。对照 eBPF：它**直接宣布平台 C ABI 就是 ABI**，绕开了这整件事。
>    **§9.4「内部钦定」的代价对比，这就是最锋利的两个数据点。**

### 4.3 Dalvik / ART / APK —— 一句话的反例

○低⚠（本条**未查证**，相关站点被网络屏蔽）：
Dalvik JIT（Android 2.2）→ **ART 安装期 AOT `dex2oat`（5.0, 2014）→ 解释器 + JIT + 空闲时 profile 引导 AOT 的混合（7.0, 2016）**。
**为什么退回来**：全程序安装期 AOT 让安装、尤其 OTA 后的重编译慢到难以忍受且撑爆存储；
profile 引导只编译可证明为热的部分。
> **根因：AOT 的成本正比于*发出的*代码量，JIT 的成本正比于*执行的*代码量，而两者之比是巨大的。**
> 对我们：**「装载时全量降级」是 ART 走过并退回来的那一步。** §3.5 的「解释冷码 + 只编译热码」在这里第二次出现。

APK 的 `lib/<abi>/` 多 ABI 分包 + App Bundle 逐设备下发：
**业界在每个尺度上对「ISA 中立」的实际答案都是「发 N 份变体、在下载/安装时选」，而不是「发一份中立的东西」**（§5）。

---

## 5. 胖/瘦二进制与分发时选择（谱位 ④）

**这一节短，因为结论短：这条路没有魔法，但它是最被低估的枯燥正确答案。**

| 手段 | 机制 | 轴 | 体积代价 | 成败 |
|---|---|---|---|---|
| **Mach-O universal / NeXT fat** | 一个文件里并列 N 份完整的目标码，头部有 fat header，装载器按当前 ISA 选一份 | ISA | **字面上 N×** 代码（资源可共享） | ✅ 成功，且被反复复用（68k/PPC、PPC/i386、i386/x86_64、x86_64/arm64、arm64e）。**成功的形态是「用完就砍」——Apple 每次过渡后都删掉旧架构** |
| **Android APK ABI split + Play 逐设备下发** | 构建期切成多个 ABI 变体，商店按设备下发一份 | ISA | 服务端 N×，**设备侧 1×** | ✅ 成功 |
| **Apple App Thinning / slicing** | 上传胖包，商店切片后逐设备下发 | ISA + 资源 | 同上。**注意：切片不需要 bitcode**（§2.4） | ✅ 成功 |
| **Debian multiarch** | 同机并存多架构的库树，路径带 triplet | ISA + 命名 | N× 磁盘 | ✅ 成功，但是**包管理层的解法，不是产物层的** |

**有没有比 N× 更聪明的做法？**

- **没有。** 跨 ISA 的机器码之间**没有可用的去重**（不同 ISA 的字节流之间不存在有意义的公共子结构）。
  据我所知从未有系统做到过 ◐中。
- 真实的缓解只有两条，**都不是压缩，是移动决定的时刻**：
  1. **把选择推到分发时**（App Thinning、Play 下发）——设备侧回到 1×，服务端仍 N×；
  2. **砍掉旧架构**——Apple 的实际做法。

> **对我们**：**「N 份极小内核，每份自认所有 OS」正是这一格，而且我们做对了。**
> 我们的内核不随 ISA 数增长（只是构建 N 份），这在体积上等价于「分发时选择」，
> 是这条轴上已知的最优解，没有更聪明的东西可抢。**这一节的价值是让我们别再在这里找灵感。**
>
> 唯一的增量提醒：**Apple 的模式是「胖 → 用完就砍」。** 我们应当预先写下
> **内核的架构退役规则**，否则 N 只增不减是这条路上唯一的体积增长源。

---

## 6. OS 轴的手段

> 本节大部分**已于 2026-08-08 对一手来源查证**（厂商文档、仓库、GitHub API）。

### 6.1 syscall 翻译层：一条统一的死因

#### WSL1 —— 根因，用微软自己的话

**机制**：`lxcore.sys`，一个在 pico process 上实现 Linux syscall 接口的 **Windows 内核驱动**。无 VM。

引 `learn.microsoft.com/windows/wsl/compare-versions`（✅ 查证，逐字）：

> "Whereas **WSL 1 used a translation layer that was built by the WSL team**, WSL 2 includes its own
> Linux kernel with full system call compatibility."
>
> "**Any updates to the Linux kernel are immediately ready for use (you don't have to wait for the
> WSL team to implement updates and add the changes).**"

能力表：WSL1 在**完整 syscall 兼容性、完整 Linux 内核、systemd** 上是 ❌；
在**跨 OS 文件系统性能**上是 ✅（WSL2 反而 ❌）。

**两条根因，都是厂商自己说的**：
1. **兼容表面是无界的，且由别人的移动实现定义。**「你不必等 WSL 团队实现」是在承认
   该团队是一条他们永远追不完的跑步机上的永久瓶颈。
2. **模拟被卡在外来原语上。** NTFS 上的 Linux 文件系统语义太慢；Docker 与 systemd 要的是内核内部，不是一张 syscall 表。

> **不可错过的微妙之处：WSL1 从未被删除。** 它仍在发货，微软仍在**恰好一个场景**下推荐它——
> 当项目文件必须放在 Windows 文件系统上时。
> **翻译层赢的地方，正是它停止重新实现、改为直通宿主的地方。**

#### Wine —— 表面相同的策略，相反的结局

**机制**：重新实现 **Win32 API**（一个比 syscall 高得多的边界）。
**体积**（✅ 查证）：仓库 **1.26 GB**，`dlls/` 下 **741 个条目**，2026-08-06 仍在推送。
（**不要引用「20,000 个 Win32 API」这类数字**——未查证；741 个 DLL 是可辩护的代理指标。）

> **差别在哪，这才是有意思的部分**（分析，✅ 高）：
> **Win32 是一个有文档、有稳定性承诺的 API**——微软的向后兼容文化意味着旧入口点实际上永不消失，
> 于是 Wine 的靶子**只会以增生方式移动**。
> 而 Linux 的 syscall ABI 虽稳定，**其周边契约（procfs、sysfs、netlink、cgroups、文件系统语义、
> ptrace、信号时序）是未规格化的且持续移动**。
> **Wine 选了一个契约被写下来的边界；WSL1 选了一个没被写下来的。**

#### FreeBSD Linuxulator —— **本节最重要的一句话**

引 FreeBSD Handbook 第 12 章（✅ 查证，逐字）：

> "**Linux software requires more than just an ABI to work. In order to run Linux software a
> Linux userland must be installed first.**"

你要 `debootstrap` 一个真正的 Debian/Ubuntu rootfs 到 `/compat/linux`。**状态：活着且在维护。**

> **它存活的根因：它从不声称 ABI 是充分的。它发接口，让你自己带封装。**
> **这就是我们 §1.2「不封装」纪律的正面现役背书**，而且是唯一一个在这条路上长期存活的项目。

#### illumos LX brand zone —— 一条查证得到的更正

○→✅：`illumos/illumos-gate` 的 `usr/src/uts/common/brand` 下**只有 `sn1` 与 `solaris10`——上游根本没有 `lx`**。
LX 只存在于 Joyent 的 fork（`TritonDataCenter/illumos-joyent`），
含 `autofs, cgroups, devfs, dtrace, io, os, procfs, sys, syscall, sysfs`，
`syscall/` 下 **53 个文件**，`lx_brand.c` 75 KB、`lx_ptrace.c` 72 KB、`lx_syscall.c` 45 KB。

**两条根因就写在那个文件清单里**：(a) 工作从未合入上游——太大、与单一厂商产品纠缠太深，
基础 OS 不愿承担，维护负担集中在一家公司；(b) **看他们不得不建的东西：procfs、sysfs、cgroups、autofs、ptrace
——又一次，不只是 syscall。**

#### 6.1.x 统一根因

> **syscall 表不是那个接口。** 真正的接口是
> **syscall + procfs + sysfs + netlink + cgroups + 文件系统语义 + ptrace + 信号/时序边角**——
> 一个 (a) 无界、(b) 由一个移动的**实现**而非一份**规格**定义、(c) 只有在逐位一致时才算「做完」的表面。
>
> **每一个试图*封装*一个 OS 的项目都撞上了这堵墙。**
> 幸存者靠**拒绝声称完备**而幸存：Wine 挑了一个有文档、有稳定承诺的 API 边界；
> FreeBSD 要求你自备一个真正的 Linux 用户态；WSL1 只在它直通而非翻译的窄缝里存活。
> 被替代的那个，是被**发真正的实现**（VM 里一个真 Linux 内核）替代的。
> **没有任何人靠把翻译层朝完备方向做大而赢过。**

**对一个几 KB 动态核的直接含义**：不要封装 OS。要么
(a) 挑一个**小的、有文档的、有稳定承诺的契约并拒绝让它增长**（eBPF 冻结的 211），要么
(b) 让可达性**晚绑定、显式不稳定、在装载时对宿主解析**（kfunc + CO-RE）。
**eBPF 两条同时做了，而这个组合是唯一一个匹配我们约束的现役先例。**

### 6.2 Cosmopolitan libc / APE —— 机制可借鉴，定位与我们相反

**机制**（✅ 查证，justine.lol/ape.html）：一个文件的开头字节**同时**是合法的 Thompson shell 脚本、
PE 的 `MZ` 头、以及 BIOS 引导扇区。shell 序言**就地重写文件自己的头**
（`exec 7<> $(command -v $0); printf '\177ELF…' >&7; exec "$0" "$@"`）然后重新 exec，
于是 Unix 内核此时看到的是 ELF。ELF/Mach-O/PE/PKZIP 结构由链接脚本在链接期合成；
PKZIP 的中央目录在文件尾，于是同一个文件也是合法 zip。ASCII 同时是 x86 代码
（`"MZqFpD"` 解码为 `pop %r10 ; jno ; jo`）。**轴：OS + 格式（不是 ISA）。**

**体积**（部分查证）：2020 年、仅 x86-64、`-nostdlib` 时 `hello.com` = **16 KB** ✅；
**今天（v3+/v4，AMD64+ARM64 × 6 个 OS 的 fat 形态）大得多，具体数字未能查到——
⚠ 不要再引用 16 KB 这个数**。可用代理：`cosmocc-4.0.2.zip` = 422 MB；
Cosmo 的 6-OS/2-arch fat `dash` 比 Alpine 的（只支持 x86-Linux 且动态链接单独的 600 KB musl）大 30%，
但运行时 RSS 更小（544 KB vs 688 KB）——靠惰性 4 KB 分页加 `apelink` 刻意的 section 布局
（Windows 专用代码放在 Unix 上永不被换入的位置）。**这个「按 OS 分段、惰性换入」的布局技巧本身值得偷。**

**当前状态**（✅ 查证）：最新 release **4.0.2**（2025-01-06），仓库 2026-07-20 仍在推送。
支持底线：Linux 2.6.18 / Windows 8 / **macOS Darwin 23.1.0+（2023!）** / OpenBSD 7.3 / FreeBSD 13
——**注意 macOS 的底线有多新，Apple 一直在打破它。**

**已知毛边**（✅ 查证于 README 的 Platform Notes，**对我们全是警告**）：

- zsh <5.9、旧 fish、Python `subprocess` 会处理错 Thompson-shell polyglot。
- **"Some Linux systems are configured to launch MZ executables under WINE."**
  其它发行版会打印 `run-detectors: unable to find an interpreter`。
  修法是注册 `binfmt_misc`——**要 root**。
- **"It's normally unsafe to use APE in a WSL environment"**，因为 WSL 会把 MZ 当 Win32 二进制跑。
  修法要 root 关掉 `WSLInterop`。
- **自修改头意味着二进制首次运行时改写自己**——需要对自己文件的写权限；
  `ape` 加载器与 `assimilate` 就是为逃离这一点而存在。

> **它放弃了什么——对我们最要紧的一点**：
> **APE 是封装，不是接口。** Cosmopolitan Libc **就是**一份 POSIX 实现：一个按 OS 在运行时分支的完整 libc
> 加上 Win32 UTF-8 polyfill。作者自己的说法是明确的封装口径——
> "gluing together the binary interfaces that've already achieved a decades-long consensus, **and ignoring the APIs**"。
> 而她 2020 年对 ISA 轴的答案是**内嵌一个 x86-64 模拟器**，并诚实标价：
> "binaries will only be 10x smaller than Go's Hello World, instead of 100x"；APE v3 改选了 fat binary。
> 它还需要 "a minor ABI change, where C preprocessor macros relating to system interfaces need to be symbolic"
> ——**即常量不能烘焙进去，与 BTF 的 `#define` 缺口是同一个问题（§4.1e）。**
>
> **净判断**：我们的 `design-dynamic-core-experiment.md` §6 已经以「它给你一个 POSIX」为由排除了这条路线，
> **这个排除是对的，本次调研只是补上了机制细节与价签。可借鉴的是三样具体东西**：
> ① 运行时 OS 探测 + 查表（不带 POSIX 那部分）；② `apelink` 的按 OS 分段惰性换入布局；
> ③ **「常量必须符号化」这条 ABI 要求——它对我们的中立 IR 同样成立，且我们还没写进任何文档。**

### 6.3 polyglot 可执行文件的真实边界

**能做到的**：各格式的 magic 在**不同位置**寻找，所以可以重叠（PKZIP 从文件尾反向扫描；
Thompson shell 不要求 shebang，所以任意前导字节都是合法「脚本」）。
POSIX 规范**确实被修订过**以允许 shell 脚本里含二进制（Justine 归功于 FreeBSD 的 Jilles Tjoelker）。

**硬边界四条**（✅ 查证/推论）：

1. **只有一种格式能在不做运行时重写的前提下拥有偏移 0。** ELF(`\x7fELF`)、PE(`MZ`)、Mach-O 都想要字节 0。
   **这个冲突就是 APE 必须自我改写的原因**——代价是对自身文件的写权限、一次文件系统往返、
   在只读/noexec 介质上失效、以及被任何嗅探改写前字节的启动器打断（上面的 WINE 误启动与 `run-detectors` 就是这条的实测）。
2. **polyglot 解决的是*格式*，不是 ISA。** 没有任何头部把戏能让 x86-64 机器码在 arm64 上跑。
   只有两个答案：fat binary（N 份）或内嵌模拟器（约 10× 体积）。
3. **polyglot 解决的是格式，不是 ABI 也不是布局。** 装载之后你仍然面对逐 OS 的 syscall 约定、
   逐 OS 的结构体布局、不同的 errno/常量值。
4. **宿主启动器是一个敌对的、无人治理的表面**：各种 shell、binfmt_misc、发行版 run-detectors、
   WSL interop、macOS codesign/公证/Gatekeeper、Windows SmartScreen。
   每一个都是一次独立集成，都能在你毫无改动的情况下打断你的文件。
   **这是持续的运营成本，不是一次性的工程成本。**

> **对我们**：我们不做 polyglot（我们按 ISA/OS 发 N 份内核，§5），**这条决定被本节强化了**。
> 但第 4 条值得单列进风险表：**任何「拿到二进制包就执行」的引擎都要面对宿主启动器与完整性证明这个敌对表面**（另见 §3.7）。

---

## 7. 装载与执行机制 —— **原语 ①②④ 的真实约束在这里**

> 本节是全文档**对我们现有实现改动最大**的一节。前面几节讲的是别人怎么死的；
> 这一节讲的是**我们的四条原语，其契约写得不够**。

### 7.1 可执行内存政策（2026）—— 对原语 ②「跳转」的挤压

**结论先行**：「申请 → 写 → 翻成 RX → 跳进去」在 **Linux、Windows（默认配置）、macOS（带 entitlement）**
上仍是一等能力。在 **iOS/iPadOS**（无特权 entitlement 时）、**开了 ACG 的 Windows 进程**、
**未 `wxallowed` 的 OpenBSD** 上**不可用**。
**iOS 上连「落盘再走装载器」的退路也不通**（文件必须由 Apple 代签），**是唯一一条没有合法路径的平台**。

| 平台 | 现状 | 关键细节 | 可信度 |
|---|---|---|---|
| **Linux** | 一等，但**假设策略层随时可撤销** | 内核不全局强制 W^X。真正会打断的是：**systemd `MemoryDenyWriteExecute=yes`**（seccomp 拒绝 `mprotect` *添加* `PROT_EXEC`——**它专杀 RW→RX 翻转**，硬化 unit 里越来越常见）；SELinux `execmem`；`PR_SET_MDWE` prctl（约 6.3）；`memfd` 的 `MFD_NOEXEC_SEAL` + `vm.memfd_noexec` sysctl（6.3，=2 时封死 memfd 双映射）；`/tmp`、`/dev/shm` 的 `noexec` 挂载 | ◐中（版本号⚠） |
| **macOS** | 一等，**但要 entitlement** | `MAP_JIT` + `com.apple.security.cs.allow-jit`；Apple Silicon 上映射常驻 RWX，**按线程**用 `pthread_jit_write_protect_np()` 翻转（硬件寄存器，无 syscall，便宜）；**执行前必须 `sys_icache_invalidate()`**。公证不拒绝 `allow-jit` | ◐中 |
| **iOS / iPadOS** | **不可用** | 内核原语是私有 entitlement `dynamic-codesigning`，实际只有 Apple 自己的 WebContent(JavaScriptCore) 持有。例外只有两条：EU DMA 下的 `BrowserEngineKit`（17.4+，须是获批浏览器厂商）、以及 `CS_DEBUGGED` 调试态（非商店合法）。**`dlopen` 未签名 dylib 也失败** | ✅ 高 |
| **Windows** | 默认**完全放行**；硬化后**两条路一起断** | **ACG**（`ProcessDynamicCodePolicy.ProhibitDynamicCode`）下 `VirtualAlloc` 带任何 `PAGE_EXECUTE_*` **失败**、`VirtualProtect` 加 execute **失败**——**是的，ACG 把我们两条候选路径都断了**。**CIG** 则封死「写个 .dll 再 `LoadLibrary`」的退路。ACG 是逐进程 opt-in，不是系统默认。业界解法是**跨进程 JIT**（另起一个非 ACG 进程编译，把可执行 section 句柄共享进来；Edge/Chakra 就这么干），代价是 IPC + 第二个进程 | ✅ 高 |
| **OpenBSD** | **按策略敌对** | 严格 W^X（`PROT_WRITE|PROT_EXEC` 需文件系统 `wxallowed`）。**而且更致命的一条打在原语 ③ 上**：OpenBSD 强制 **syscall 来源限制**（`msyscall`/pinsyscalls，7.3+），**从 libc text 区之外的任何地址执行 `syscall` 指令一律杀进程**。**我们的「裸 syscall 原语 + 生成到 JIT 内存里」在 OpenBSD 上按设计就是致命的** | ✅ 高 |
| **Android** | 匿名 `PROT_EXEC` 可以（ART 需要）；但**从应用可写路径 `dlopen` 被封**（Android 10 起） | 落盘退路受限，进程内 JIT 没问题 | ◐中 |

> **对我们（必须落进设计）**：
> 1. **原语 ② 有三种实现，不是一种**：直接 RX / 跨进程共享 section / **解释**。
>    「事后给一个 JIT 形状的设计补一个解释层」比反过来痛苦得多——**解释层必须是一等层，不是补丁**。
> 2. **原语 ③ 的裸 syscall 在 OpenBSD 上从生成代码里发出即死。** 这条我们完全没考虑过。
> 3. **启动时探测，不要假设。**

### 7.2 跳进去之后：**「标为可执行」≠「平台会让你执行且事后行为正常」**

这是本节最重要的一段。以下每一条都是**原语 ②「把控制权交给一段内存里的字节」隐含假设、
但在真实平台上不成立**的事：

| 事项 | 不做会怎样 | 可信度 |
|---|---|---|
| **指令缓存一致性** | x86 上免费（硬件维护 I/D 一致）。**AArch64 / RISC-V 等弱序 ISA 上强制且不显然**：写者要 `DC CVAU` → `DSB` → `IC IVAU` → `DSB` → `ISB`；**而且将要执行新代码的*其它*线程需要它自己的 `ISB`，写者无法代劳**。这就是 V8/JSC 有显式跨线程代码发布协议的原因。可移植写法：`__builtin___clear_cache` / `sys_icache_invalidate` / `FlushInstructionCache` | ✅ 高 |
| **CFI 落地指令** | **Intel CET-IBT**：间接跳转的目标处必须有 `ENDBR64`，否则**faults**。**ARM BTI**：`PROT_BTI` 页上要 `BTI c/j/jc` 落地垫。**arm64e PAC**（全部 Apple Silicon 已发货）：函数指针与返回地址被**签名**，JIT 不正确签名即 fault。**这些今天就在发货**，不是未来问题 | ✅ 高 |
| **Windows x64 unwind 注册** | x64 Windows 是**表驱动**异常处理。非叶函数必须有 `RUNTIME_FUNCTION`（3×DWORD=12 字节）指向 `.xdata` 里的 `UNWIND_INFO`（最小约 8 字节，典型小帧 12–20 字节）。生成代码要用 `RtlAddFunctionTable` / `RtlInstallFunctionTableCallback` / `RtlAddGrowableFunctionTable`（Win8+，JIT 追加场景）注册。**不注册时** unwinder 会套用**叶假设**（「返回地址在 `[RSP]`、没保存寄存器」），对任何动过 RSP 的帧都产出垃圾 RIP，于是**要么进程死、要么走进乱码**。丢掉的能力：C++/SEH 穿越你的帧、`RtlCaptureStackBackTrace`、调试器调用栈、ETW/采样 profiler、崩溃转储栈。<br>**叶函数可以合法跳过**：不动 RSP、不 call、不 throw、不存非易失寄存器（纯 `jmp` 的 trampoline/thunk 就是）。**注意这是 RSP 增量的性质，不是「在栈顶」的性质** | ✅ 高 |
| **Linux/macOS 的对应物** | DWARF CFI（`.eh_frame`：CIE 约 24–32 B 共享 + 每函数 FDE 约 32–48 B），生成代码不在任何 ELF 对象里，须调 `__register_frame`/`__deregister_frame`。**经典可移植陷阱**：libgcc 的 `__register_frame` 收**单个 FDE** 指针，LLVM libunwind 收**整块以零长度结尾的 `.eh_frame``**——两者不通用。<br>**便宜的替代、且现在重新成为主流**：**总是发 `push rbp; mov rbp,rsp`**。Fedora 38、Ubuntu 24.04 全发行版重新启用了帧指针，就是为了 `perf -g fp` 能work。代价一个寄存器 + 约 1–2%，**对 KB 级内核这比发 DWARF 好得多** | ✅ 高 |
| **符号化与 profiler 可见性** | `perf` 要 `/tmp/perf-<pid>.map` 或 jitdump；GDB 要 JIT 接口（`__jit_debug_register_code`）。**各约 50–200 行，便宜；但一旦载荷格式丢掉了源位置映射就无法补救**——**从第一天起在载荷里保留位置信息，哪怕暂不消费** | ✅ 高 |
| **代码退役** | 「reserve/commit/protect」没有对偶：「这块代码死了，但可能有别的线程正在里面执行」。需要 safepoint / epoch / RCU 回收，或者干脆不释放。`__deregister_frame`、`RtlDeleteFunctionTable` 必须配对，否则泄漏并最终污染 unwinder 表 | ✅ 高 |
| **陷阱与故障处理** | 载荷 fault 了怎么办？需要故障边界（`sigaction` / `AddVectoredExceptionHandler`）、把出错 PC 映回载荷级位置、恢复路径。很多 JIT **依赖** fault 求正确（隐式空检查、栈溢出守卫页、wasm 边界检查靠 4 GB 守卫区）。**Windows 上故障传播还依赖上面的 unwind 信息** | ✅ 高 |
| **TLS** | 生成代码想访问线程局部量不能直接发一条 load。访问模型（initial-exec / local-dynamic / global-dynamic、`__tls_get_addr`、`%fs:` 偏移、macOS `tlv_get_addr` thunk、Windows TEB + `_tls_index`）是 ABI 专属，**不是「调一个符号」能表达的**。JIT 的常规解法是**在固定寄存器里钉一个运行时提供的 per-thread 上下文指针**——这是一个**必须在写第一条 stencil 之前定下的约定** | ✅ 高 |

### 7.3 原语 ④「按签名描述调用」的真实边界

**libffi 的机制与关键因式分解**：`ffi_prep_cif` 对 `ffi_type` 描述符跑目标 ABI 的**分类算法**并把方案缓存进 `ffi_cif`；
`ffi_call` 按方案把参数拷进栈帧，跳进一个**预编译好的固定汇编 trampoline**（`ffi_call_unix64` / `ffi_call_win64`）。
x86-64 Linux 上共享库约 40–60 KB ◐中⚠；每 ABI 的核心（分类器 + trampoline）约 1500–3000 行 C + 约 200 行汇编。

> **一个对我们非常有用的因式分解** ✅ 高：
> **出站调用（原语 ④）不需要可执行内存；只有*入站回调*（closure）才需要。**
> 也就是说**原语 ④ 独立于 ①②**。推论：**一个「什么都能调」的核可以部署在 iOS 上，
> 而一个「能生成代码」的核不能。** libffi 3.4+ 用**静态 trampoline 表**（一页预编译的相同 trampoline
> 经 memfd 双映射，各自从平行数据页的固定偏移取回 closure 指针）连 closure 也不再需要 W^X 窗口。

**libffi 类模型处理不好/不可移植的地方**（= 我们判据「失去的表达力」的预答案）：

| 缺口 | 原因 |
|---|---|
| **结构体按值** | `ffi_type` 能表达尺寸/对齐/成员，**但不能表达位域、不能表达联合体**（只能按最大成员建模——在某些 ABI 上是错的），不支持 C++ 非平凡类型。AArch64 HFA/HVA 与 SysV eightbyte 分类是递归的，历史上边角一直有 bug（结构体里的数组、`_Complex`、超对齐成员） |
| **varargs** | `ffi_prep_cif_var` 存在正因为各 ABI 对可变槽处理不同：**SysV x86-64 要求 `AL` = 使用的向量寄存器数**；float 提升为 double；**Apple arm64 把全部可变参数放栈上**，偏离 AAPCS64 并需要 libffi 里一份单独的 ABI 代码。仍「不完整」是因为调用方必须知道 `nfixed`，而很多前端不知道 |
| **long double / `_Float128`** | 一个标签三种不兼容含义：x87 80 位 / IEEE binary128 / double-double。**中立载荷根本无法可移植地*构造*这个值** |
| **软浮点 ABI** | ARM `FFI_SYSV` vs `FFI_VFP`；MIPS o32/n32。选错了是静默的 |
| **C++** | 无 mangling、无 `this` 约定（Itanium 恰好等于「第一个参数」；MSVC 把 `this` 放 RCX，而 sret 指针会**挤位**）、无虚分派、异常不能穿 `ffi_call`。实质不支持 |
| **sret / 返回走内存** | 支持，但隐藏指针约定不同：SysV 走 RDI（**占掉一个整型参数寄存器**）并在 RAX 返回；MSVC 走 RCX；某些 32 位 ABI 由**被调用方**弹出 |

**SysV vs Win64 —— 真会咬人的差异**（✅ 高）：

| | x86-64 SysV | Win64 |
|---|---|---|
| 整型参数寄存器 | RDI,RSI,RDX,RCX,R8,R9（**6**） | RCX,RDX,R8,R9（**4**） |
| 浮点参数寄存器 | XMM0–7，**独立计数** | XMM0–3，**与 GPR 槽位置耦合**（同一槽用其一，绝不同时——varargs 例外，那时 float **两边都放**） |
| 影子/home 空间 | 无 | **恒定 32 字节**，零参调用也要 |
| 红区 | **128 字节** | **无** |
| varargs 标记 | **AL = 用到的向量寄存器数**（不设则被调用方的寄存器保存区序言行为错乱） | 无 |
| 结构体按值 | 分类成 eightbyte，≤16 B 可跨 INTEGER/SSE 拆分；>16 B 拷到栈 | **仅当**尺寸 ∈ {1,2,4,8} 才按值；否则调用方分配临时并传指针 |
| >8 字节返回 | 隐藏指针在 RDI（挤位），RAX 返回 | 隐藏指针在 RCX（挤位），RAX 返回 |
| 被调用者保存 | RBX,RBP,R12–R15。**没有任何 XMM 是被调用者保存的** | RBX,RBP,RDI,RSI,R12–R15，**外加 XMM6–XMM15** |
| unwind 元数据 | 实践上可选 | **强制**（§7.2） |

> **手写桥的两个最常见 bug**：SysV varargs 忘了设 `AL`（`printf("%f")` 打出垃圾）、Win64 上踩坏 XMM6–15。
>
> **手写桥的诚实体积**：Win64 分类器约 50 行 C；**SysV 分类器约 200–400 行（成本全在这里）**；
> 汇编 trampoline 约 100–200 字节。整数+指针+浮点、两个 ABI 约 **1–2 KB**；加 ≤16 字节结构体约 **3–6 KB** ○低（估算）。
> **最诚实的最小答案**：若能约束被调用方「无结构体按值、无 varargs、无 long double」，
> **每 ABI 一个固定 trampoline + 每参数一个 32 位类型掩码 ≈ 300 字节，覆盖 90% 的真实 FFI 用途。这个约束值得接受。**

### 7.4 **不带编译器地生成原生码：copy-and-patch** ⭐

**这是本文档认为我们最该拿走的一条技术。**

**机制（精确）** ✅ 高：
1. **构建期**：为「每个 IR 操作 × 每种操作数变体」写一个小 C 函数（**stencil**）。
   stencil 里引用未定义的 `extern` 符号作为**孔（hole）**——立即数的孔、运行时 helper 地址的孔，
   以及关键的**下一个 stencil 地址**的孔。用**真正的优化编译器**（clang -O2）把它们编成可重定位对象。
2. 抽出每个 stencil 的 `.text` 字节**外加它的重定位条目**，冻进产物里的一张表。
3. **装载/运行期**，「编译器」就是：按 IR 操作选变体 → `memcpy` 字节进代码缓冲 → 走一遍记录的重定位把具体值写进孔。
   **没有指令选择、没有寄存器分配器、没有汇编器、没有链接器。**

**两个使它不只是小、而且快的技巧**：
- **续延传递形式**：每个 stencil 以**尾调用**下一个 stencil 结束，地址是个孔。
  **分派循环完全消失**——「解释器」变成一条直线的尾调用链，分支预测器看到的是直接跳转。
  LLVM 的 `ghccc`（以及后来的 `preserve_none`）调用约定的存在就是为了让几乎整个寄存器堆能跨界携带 VM 状态。
- **用变体爆炸代替寄存器分配**：每个操作发多个 stencil（操作数在寄存器 vs 在栈槽、常量 vs 动态、按类型特化），
  于是「寄存器分配」退化成 **stencil 选择**，一次查表。

**成绩**（Xu & Kjolstad, PLDI 2021）：编译速度约比 LLVM `-O0` **快两个数量级**，
代码质量**略好于 `-O0`**、远不及 `-O2`；对 WebAssembly 而言比 V8 的 Liftoff 基线层编得更快且代码更好
◐中⚠（**形状可信，数字待核**）。

**落地**：**CPython 的 JIT（PEP 744, 3.13+）** —— `Tools/jit/` 在 **CPython 构建期**用 clang
把 `template.c` 逐 micro-op 编译，读取目标文件，产出 `jit_stencils.h`。**终端用户不需要编译器。**
运行时部分（`jit.c`）小，约 1–2 kLOC；stencil 数据每架构数十到低百 KB ◐中⚠。

**它是否消除了产物里的代码生成器？是——这是对我们最关键的结论。**
你只发一个 `memcpy` 循环加一个重定位应用器。但要诚实记两笔它**转移**而非消除的成本：

1. **stencil 表是逐 ISA 且逐 ABI 的。** 你把胖二进制的 N× 又请回来了——
   **但只落在表上、不落在整个运行时上，而且表恰好是「分发时选择」能剥掉的那个东西（§5）。
   这正是这笔成本该落的地方。**
2. **重定位应用器不是架构中立的。** `R_X86_64_PC32` 是 32 位有符号写；
   `R_AARCH64_CALL26` 是 26 位字段、**±128 MB 可达范围**；
   `ADR_PREL_PG_HI21` + `ADD_ABS_LO12_NC` 是必须一致修补的两指令对。
   每 ISA 大约 8–15 种重定位、各约 200–400 字节代码。小，但是复数。

> **会打穿朴素实现的那条约束（我们的原语 ① 缺了它）** ✅ 高：
> `PC32` 要求代码缓冲落在**距离它调用的每个运行时符号 ±2 GB 内**；AArch64 `CALL26` 要求 **±128 MB**。
> **一个只会说「给我 N 字节」的内存原语，会给你一块够不到自家运行时符号的缓冲，
> 而失败模式是静默截断的重定位，不是一个错误。**
> **原语 ① 需要一个「靠近地址 X / 在 ±R 内」的放置约束参数**，外加 veneer/thunk island 策略。
> **每个人在第一次移植到 AArch64 时都会撞上这条。**

**这条路会坏在哪**（必须写清）：
1. **跨 stencil 没有寄存器分配**：值在每个边界必须落在固定寄存器或栈槽，每个边界付一次 store+load
   （除非变体 stencil 吸收掉）。**这是代码质量损失的大头。**
2. **每个边界都要一个调用约定**：需要一个「几乎什么都不是被调用者保存、几乎什么都能带参数」的约定
   ——这正是 LLVM 加 `ghccc`、后来加 `preserve_none` 的原因。
   **于是构建期工具链要求变硬：你需要 clang/LLVM，不是「任意 C 编译器」。** ◐中（CPython 接受了这个供应链约束）
3. 跨 stencil 无内联、无 CSE、无常量折叠：**代码体积约 1.5–3×**、运行时约 2–5× ◐中。
4. 重定位可达范围（上）与 I-cache 压力。
5. **调试/unwind 元数据必须另行合成**——stencil 自带的 CFI 在链起来之后就没意义了（§7.2）。

### 7.5 解释：调色板里缺的那一格

| 技法 | 机制 | 体积 | 速度 |
|---|---|---|---|
| `switch` 分派 | 可移植 C | 最小，核心约 2–8 KB | 基准 |
| 直接线索化 | 操作数组存**标号地址**，`goto *ip++`（GCC computed goto） | +≈0 | **历史传说是快 20–50%** |
| 子程序线索化 | 发一串真实 `call` 指令 | 需代码缓冲 | 好——吃到**返回地址栈预测器** |
| 上下文线索化 | 把 VM 分支变成真机器分支，让硬件预测器看见 | 中 | 相当程度上追平基线 JIT |

> **对民间传说的重要更正** ✅ 高：在 Haswell 之后（约 2013+）带 ITTAGE 级间接分支预测器的 CPU 上，
> **直接线索化相对 `switch` 的优势已缩到约 5–15%，不是 90 年代传说里的 2×**
> （Rohou et al., *Branch prediction and the performance of interpreters — don't trust folklore*, CGO 2015）。
> **不要为这个预留大收益。**

量级 ◐中：**解释器比优化原生慢 5–50×**（取决于操作粒度）；**基线/模板 JIT 慢 2–5×**；
**copy-and-patch 落在基线 JIT 的档位**。

基线 JIT 能做到多小：**V8 Sparkplug（2021）是最有信息量的数据点** ✅ 高——
一个**完全没有 IR** 的基线编译器，对字节码单趟线性扫描、每操作发一段固定序列，量级**几千行**；
它的决定性设计技巧是**保持解释器的栈帧布局完全一致**，于是 OSR 进出几乎免费、且不需要 deopt 元数据。
**LuaJIT** 则是另一条路：解释器是**逐架构手写汇编**，经 **DynASM** 生成——
即「模板写成汇编 + 发一个编码器」，是 copy-and-patch 的对偶。整个 LuaJIT 约 400–600 KB ◐中。

---

## 8. 相邻理论与系统

### 8.1 Futamura 投影 / 部分求值 —— **重要的重构**

第一投影：把解释器对某程序特化 → 得到编译后的程序。真实系统：**Truffle/GraalVM**（运行时做第一投影）、
**PyPy/RPython**（元追踪等效物）。

**判决：本质上是大运行时的想法，KB 级不可达，而且原因是结构性的**——
特化器需要 (a) 能符号执行的解释器语义、(b) 一个激进优化器来消掉残余的解释开销
（特化输出在优化之前**很糟**）、(c) 去优化机制。GraalVM 的编译器是数十 MB。

> **但值得给我们的重构是** ◐中（判断）：
> **copy-and-patch 就是 KB 级的 Futamura 第一投影。**
> 一个 stencil 精确地就是「解释器对一个 opcode 特化、操作数留成孔」的结果，
> **而这次特化是在构建期由一个真正的优化编译器离线完成的**。
> 你拿到第一投影的收益、不付它的运行时代价，代价是一张逐 ISA 的 stencil 表。
> **如果本文档只带走一条架构思想，就是这一条。**

### 8.2 Unikernel / Exokernel —— 我们的思想祖先，与它的警示

- **Exokernel**（MIT Aegis/ExOS、Xok/ExOS，1995–97）：内核只导出硬件的**名字**与**保护**，
  抽象由应用内的 library OS 提供。**这字面上就是我们原语 ③ 的「给可达性、不给语义」，
  作为一个 OS 设计命题。它是本项目最近的思想祖先。** ✅ 高
- **Unikernel**：MirageOS（约 100 KB–数 MB）、IncludeOS（约 1 MB）、Unikraft（约 100 KB–2 MB）、
  Nanos/OPS、OSv（10–20 MB）◐中。
- **为什么没能统治**，按重要性：① **运维**——没有 shell、`ps`、`strace`、`gdb`、sidecar、`kubectl exec`，
  既有全部运维实践失效；② **没有东西可以跑在上面**——世界上的软件是 POSIX 二进制，移植是逐应用且无界的；
  ③ 容器以约 0 移植成本吃掉了用例，Firecracker/gVisor/Kata 补上了隔离差距；
  ④ 单地址空间无内部特权分离。
- **对我们的相关性：高，作为关于第 ② 条的警示。**
  **一个四原语的核有完全相同的「没有东西可以跑在上面」问题，除非载荷生态先被解决。**

### 8.3 Drawbridge / picoprocess / WSL1 —— **对「四条够不够」最强的经验证据**

Drawbridge（MSR, ASPLOS 2011）把 Windows 7 重构成进程内的 **Library OS**，跑在 **picoprocess** 里，
**对宿主的全部接口是一层 PAL，约 45 个下调用** ◐中⚠。真实后代：**WSL1**（`lxcore.sys`，
真正的 Windows 内核驱动实现 Linux syscall；被 WSL2 取代，根因是 syscall/ioctl 保真度缺口
与决定性的 NTFS 上文件系统性能）、**SQL Server on Linux 的 SQLPAL**（这条路上最大的真实产品）、
**Haven（Drawbridge+SGX）→ Graphene/Gramine**（PAL 约 30–40 个调用）。

> **这是对「完备性」问题最强的经验数据** ◐中：
> **三个独立团队在「一个通用程序需要的最小宿主接口是什么」这个问题上，各自收敛到 30–50 条原语，不是 4 条。**
> 见 §9.2 与 §10.5 的处理。

### 8.4 CHERI / 能力硬件

- **机制**：指针变成 128 位能力 + 一个带外 **tag 位**，携带 base/bounds/permissions/object type。
  溯源不可伪造：只能从已有能力**派生**且单调收窄；任何「从整数造出能力」的算术会清掉 tag，
  解引用无 tag 的字会 trap。轴：**ABI（根本上）** ✅ 高
- **会打断原语 ④ 吗？会，而且正是我们担心的那种方式**：在 CHERI purecap 上，
  一个把每个参数当整数字的 FFI **会丢掉 tag**，产出不可用的指针。CheriBSD 的 libffi 移植不得不
  在签名描述里引入一个独立的指针类型 ◐中。
  > **但这在没有 CHERI 时也是好的设计约束** ✅ 高：你在四条与 CHERI 无关的理由上也需要指针/整数之分——
  > **精确 GC 根扫描、ILP32-on-LP64 目标、wasm 的 64 位宿主上 32 位指针、Android 的 MTE 带标签指针。**
  > **一份不能说出「这个字是指针」的签名描述，在四条独立理由上都是欠规格的。**
- **会打断原语 ② 吗？部分会**：你需要一个从 PCC 或映射派生的**可执行能力**，不能从地址伪造；
  `mmap` 在 CheriBSD 上返回能力所以流程走得通，但「跳到这个整数地址」不行。
- **2026 部署现实**：Arm **Morello** 是研究原型板（2022，数百块量级）；**CHERIoT**（微软的 32 位 RISC-V
  嵌入式 profile）有真硅片；CHERI Alliance 2024 成立。**没有主流应用处理器发货 CHERI。** ◐中⚠
- **紧迫性判决：部署上低，设计卫生上高。** 保险费——在签名格式里把指针参数单独标记——几乎免费，
  而且在上面四条非 CHERI 理由上**立刻**回本。

**而 2026 已经部署、我们必须处理的版本**（见 §7.2）：**Intel CET-IBT（`ENDBR64`）与影子栈、
ARM BTI、arm64e PAC、Android MTE。**

### 8.5 携带证明的代码（PCC）与小验证器 —— 「小 TCB + 不可信代码」有便宜解吗

**PCC**（Necula & Lee, OSDI'96 / PLDI'98）：生产者发原生码 + 一份用 LF 编码的**证明**；
消费者跑 VCgen 从机器码导出验证条件，再用一个**小 LF 证明检查器**验证。
可信基 = VCgen + 检查器 + 公理，量级几千行。

**为什么停在学术界**（按重要性升序）◐中：
1. **证明体积**：早期证明是代码的 1–3 倍，有时更糟。Oracle-based PCC 压到约 10–20%，代价是检查器变大变复杂。
2. **可信基其实并不小**：VCgen 编码了机器语义与类型系统，本身就有 bug。Foundational PCC 把 TCB 缩到逻辑 + 机器语义模型，
   代价是证明与生产者工作量都爆炸。**这个权衡把这个领域自己吃掉了。**
3. **只有 certifying compiler 才产得出证明**——所以你**无论如何都得控制生产者**；
   **而到了那一步，给输出签名便宜得多，且给出同等的实用保证。**
4. **决定性的一条：人们真正想要的性质（「这段代码不是恶意的」）不是一个形式化的安全性质。**
   编译器生成代码的内存安全，从来不是任何人害怕的东西。

**真正落地的替代**：
- **Java 的 `StackMapTable`**（classfile v50，Java 6 引入；v51/Java 7 起强制）
  **是发货在数十亿台设备上的 PCC，只是没人这么叫它**：class 文件携带**验证器在每个汇合点的类型状态**，
  于是验证从迭代数据流不动点退化成**单趟线性检查**。
  **那正是 PCC 的交易——生产者干活、消费者检查——被应用在唯一一个便宜到可行的性质上。**
  ✅ 高。**这是「小验证器」现有的最佳模板。**
- **eBPF 的验证器是反例**：`kernel/bpf/verifier.c` 约 **20 kLOC** ◐中⚠，做路径敏感探索 + 状态剪枝，
  CVE 不断。**它的体积就是「验证未加注解的代码」的经验价格。**
  **PREVAIL**（eBPF-for-Windows 用）明显更小（几千行），因为用了原则化的抽象域（zone/octagon）而非临时的范围跟踪
  ——**说明 eBPF 验证器的体积部分是偶然的，不全是本质的** ◐中。

**五个真实选项与诚实代价**：

| 选项 | TCB 体积 | 运行时代价 | 诚实评价 |
|---|---|---|---|
| **① 离线签名载荷** | Ed25519 验证路径约 **2–3 KB**；带完整 crypto 库约 10–20 KB | ≈0 | **转移信任，不减少信任。** 但这是每一个发货平台的做法（Apple codesign、Secure Boot、APK v2/v3、Linux 模块签名）。**便宜一个数量级** |
| **② SFI**（Wahbe 1993；PittSFIeld；NaCl） | 验证器约 **2–5 kLOC** | 约 5%（NaCl x86-32 分段）到 10–25%（x86-64/ARM 掩码）◐中 | **唯一真正的「小验证器」技术——而它的把戏不是聪明的验证，是*约束代码形状***（定长对齐 bundle、所有 store 与间接跳转掩码进沙箱），于是验证退化成线性扫描。**技术本身是好的，可直接借鉴**，前提是你也控制代码生成器 |
| **③ 硬件/OS 隔离** | OS（巨大，但**已经**被信任） | 进程 + IPC | 独立进程 + seccomp/pledge/Job Object/AppContainer。**对 KB 级核这是正确答案，因为你在*复用*一个既有 TCB 而不是自建** |
| **④ W^X + CFI/CET/BTI/PAC** | ≈0 | 约 1–5% | **是缓解，不是隔离。** 而且它们对你的生成代码提出**义务**（§7.2） |
| **⑤ 解释它** | 解释器 **2–20 KB** | 5–50× | **解释器由构造即是它自己的验证器。** 这就是 ACPI AML、cBPF、EBC、FCode、Java Card 全都是解释执行的原因。**在 KB 尺度上，这是唯一同时做到「小、安全、可移植」的选项** |

> **判决**：对一个几 KB 的核，可达的安全故事是 **① + ③ + ⑤**——签名、把不可信载荷放解释器里跑、
> 对降级成原生的东西用 OS 隔离。**自建验证器是一个以年计、持续产 CVE 的承诺；
> SFI 是唯一划算的中间选项，且前提是我们也控制代码生成器使其发出受约束的形状。**

---

## 9. 与本轨已定架构的对照

**这一节是本文档存在的理由。** 前面是别人的历史，这一节是把历史对准我们自己。

### 9.1 我们在坐标系里的位置

| 轴 | 我们目前的处理 | 历史评价 |
|---|---|---|
| **ISA** | 每个 ISA 构建一份内核（分发时选择，谱位 ④）；载荷在装载时降级（谱位 ⑥） | 分发时选择是**枯燥但有效**的（§5）；装载时降级有 Slim Binaries 与 eBPF 两个成功先例（§2.3、§4.1） |
| **OS** | 每份内核自认所有 OS，原语 ③ 给可达性、不给语义；**明令禁止封装** | **这是全文档最被历史背书的一条决定**。§3.9 死因 #2 与 §6 的 syscall 翻译层史都指向：谁扛 OS 语义谁死 |
| **ABI** | 载荷统一 sysv64，内核在边界做 `sysv64→win64` 桥接 | **不是 hack，是 ARM64EC 同款策略**（§3.8）；但 ARM64EC 同时开出了价码，见 9.3 |
| **布局** | 中立 IR 实验的 §1.1/§1.2 选择了**禁止**（不许 `sizeof` 求值成数、不许 `offsetof`） | **这是唯一一处我们选了历史上较弱的解法。** ANDF 用 unbound token、eBPF CO-RE 用装载时重定位——两者都是**推迟**而非**禁止**。见 9.4 |
| **命名与链接** | **未被承认为一根轴。** 载荷里写着 `"CreateFileW"` 这样的字面量喂给原语 ③ | **未解。** 见 9.2 |

### 9.2 四原语够不够？逐条压力测试

> **结论先行（本次调研改变了此前的判断）**：本轨实测（`RESULTS.md` §偏离 6）没出现加第五类的冲动，
> 但**那是因为实验只跑了两个 OS、一个 ISA、三个短叶子载荷**。
> 历史与平台现状指出：**①②④ 的契约都写得不够**，而且**存在一整类被漏掉的机制（元数据双向流动）**。

| 原语 | 历史/平台给出的压力 | 判断 |
|---|---|---|
| **① 内存 (RW↔RX)** | (a) 平台正在收回「把内存变成可执行」的权利（§7.1）——iOS 无路、Windows ACG 两条路全断、OpenBSD 需文件系统 opt-in、systemd `MemoryDenyWriteExecute` 专杀 RW→RX；(b) 重定位可达范围：`PC32` ±2 GB、`CALL26` ±128 MB，**给错位置的失败模式是静默截断** | **契约没写全，缺两样**：① 需要**放置约束参数**（§10.3）；② 需要承认「RW→RX 恒可用」这个假设在多个平台上已不成立 |
| **② 跳转** | 「标为可执行」**不等于**「平台会让你执行且事后行为正常」：I-cache 一致性（ARM/RISC-V 强制，且**其它线程要自己的 `ISB`，写者代劳不了**）、CET-IBT `ENDBR64` / ARM BTI 落地指令 / arm64e PAC 签名（**今天就在发货**）、Windows x64 unwind 注册、代码退役、故障边界、TLS 访问模型（§7.2） | **契约缺口最大的一条。** ② 目前只承诺「控制权交出去」。**而且它有三种实现（直接 RX / 跨进程共享 section / 解释），不是一种**（§10.2） |
| **③ 可达 (syscall + dlsym)** | (a) **命名轴**：`dlsym` 需要一个名字，名字是目标专属事实（ANDF 的 TDF token 正为此发明）；(b) **OpenBSD 从生成代码里发 `syscall` 指令一律杀进程**（§7.1）；(c) eBPF 的教训：**可达机制会静默约束执行引擎**（kfunc ⇒ 必须 JIT，§4.1c） | **原语 ③ 本身没错，但它*回答不了布局问题*。** `dlsym` 解析符号到地址，回答不了 `offsetof(struct stat, st_mtime)`——**再多 `dlsym` 也不行**。见 ⑤ |
| **④ 调用 (按签名描述)** | 本轨已自曝 arity 天花板（7→11）。历史进一步给出真实边界：**struct-by-value（位域/联合体表达不了）、varargs（SysV 的 `AL`、Apple arm64 全走栈）、long double 一标签三义、sret 隐藏指针挤位、软浮点、C++**（§7.3）；ARM64EC 证明 **ABI 与布局无法分开推迟**（§3.8） | **④ 是承重墙。三条修订**：(a) 想真正通用必须连**结构体布局规则**一起选定；(b) **必须能区分指针与整数**（§10.6）；(c) 好消息——**出站调用不需要可执行内存，只有入站 closure 才需要**，故 ④ 独立于 ①②，**一个「什么都能调」的核可以部署在 iOS 上**（§7.3） |
| **⑤ Declare（缺失）** | 元数据在核与平台之间**双向**流动：向平台**发布**生成代码的 unwind/符号/落地指令/一致性；向平台**询问** ABI 变体与**类型布局**。CO-RE 之所以work，仅因为内核发布了 BTF | **这是被漏掉的一整类机制**，且在「只有做」的四原语模型里结构性不可见。详见 **§10.5** |

**净判断（修订）**：
1. **①②④ 的契约必须补全**——这不是加原语种类，符合 §1.1。
2. **⑤ 应当被单列为第五类**，因为它在性质上不是「做」而是「元数据流动」，藏进①②④会让它继续不可见。
   §1.1 要求把「加第五类的冲动」记为发现而非偷偷满足——**本文档就是在履行这条**：如实记录，交决策者判断。
3. 经验旁证：Drawbridge / Gramine / WASI **三个独立团队都收敛到 30–50 条宿主原语，不是 4 条**（§8.3）。
   **4 条只有在核是「一个带 FFI 的解释器」时才可达**——而那恰好也是唯一小 TCB 安全的形状（§8.5、§10.2）。

### 9.3 我们的中立 IR 假设，在这些历史面前站得住吗？

分四条，**两条站得住、一条命中我们、一条我们比历史更有利**。

#### (a) UNCOL 的深层张力：**大部分不命中我们，因为我们的 M = 1**

（先记住 §2.1 的事实更正：**UNCOL 从未被实现**，它是一个论证不是一个系统；
「有名字的失败」指的是这个论证被反复重演并反复失败。）

UNCOL 之所以是一个**有名字的失败**，深层原因是：一个通用 IR 必须同时**高到能承接所有源语言**、
**低到能落到所有目标**，而这两个要求朝相反方向拉。历史上化解这个张力的唯一办法是**把 IR 做大**
（LLVM 的答案）——而做大正是本命题明令拒绝的。

**但 UNCOL 的张力是 M×N 的，其中 M 是源语言数。我们的 M = 1**（载荷由我们自己的工具产出，
不需要承接任何第三方语言）。**M=1 时，UNCOL 张力的「高到能承接所有源语言」那一半直接消失**，
只剩「低到能落到所有目标」——那是一个普通的编译器后端问题，不是一个有名字的失败。

> **应写死为不变量**：*一旦出现第二种独立的、不由我们控制的源语言想产出这份 IR，UNCOL 风险立即回归。*
> 任何「让 IR 更通用一点好让别人也能用」的提议，都是在把 M 从 1 抬起来，必须按这条驳回。

#### (b) Apple Bitcode 的根因：**直接命中我们当前的实现**

Bitcode 之死的根因不是「LLVM IR 不好」，而是 **LLVM IR 不是目标中立的，它是「为某个目标做完一半降级」的产物**：
target datalayout、指针/整型宽度、对齐、结构体字段偏移、聚合体的 ABI lowering 都已烘焙进去
——因为 **Clang 在生成 IR *之前* 就把 C 的 ABI 降完了**。

**这条根因命中我们，而且已经在数据里了。** 本轨层数实验的变体 B，其「载荷 blob」是
**用 Rust 按 ELF 目标编译出的机器码展平**——编译器早已做完全部 ABI 与布局决定，
内核只是在边界打补丁（`sysv64→win64` 桥接、红区警告）。`design-neutral-ir-experiment.md` §0
已经诚实地写了这一点（「那不是中立，是选一个 ABI 然后打补丁」），历史只是确认它：
**这正是 Bitcode 的失败形态，一字不差。**

> **给中立 IR 实验的硬要求**：判据 ① 的「中立性」必须从**一个从未经过 ABI 决定的表示**出发。
> 若 IR 的生产路径上有任何一个环节是「面向某目标的编译器前端」，实验测的就不是中立性，
> 而是「补丁打得好不好」。这是能让整个实验失效的方法学缺陷。

#### (c) ANDF 的根因：**技术上它解决了我们的问题；它死于经济，而那个经济根因不成立于我们**

ANDF/TDF 是**与我们最像的先例**：它把 `sizeof`、对齐、结构体偏移、以及 API 实体
都留成**未绑定 token**，由目标机上的 installer 依据该目标的 API 定义绑定。
**这就是「把布局与命名决定推迟给降级」，而且它技术上是work的。**

它死于：Unix 阵营的厂商没有动机去削平自己平台的差异；Unix 战争结束后 ISV 直接转投
Windows/x86，「一份产物跑遍所有 Unix」这个价值主张的需求方消失了。
（本条的政治/技术之分需联网核实，见 §11。）

> **这个经济根因不成立于我们**：我们的载荷**生产者与消费者是同一方**。
> 没有需要被说服的第三方厂商，没有需要被证明商业价值的 ISV。
> **ANDF 的技术遗产（unbound token）可用；ANDF 的死因不适用。这是本文档对本轨最有价值的单条结论。**

#### (d) PNaCl 已经替我们答了判据 ④「失去的表达力」

PNaCl 为了把 LLVM IR 冻结成一个稳定中立子集，必须跑一串 **ABI 简化 pass**：
消除 `byval`、把结构体参数拆成标量、legalize 类型、去掉不稳定的 intrinsic。
换句话说，**「让 IR 中立」在工程上等价于「从 IR 里删掉聚合体按值传递与 varargs」**。

这**预先回答**了 `design-neutral-ir-experiment.md` §3 判据 ④ 要测的东西：
结论大概率是「结构体按值传参/返回、varargs 会被禁掉，替代形态是**强制过内存**
（调用方分配缓冲，传指针，由降级决定实际放法）」。实验仍值得跑（要量代价），
但**不应把这个发现当作意外**。

### 9.4 中立性的形状：**内部钦定，边界声明**——不是「全面推迟」

§2.7 模式 2 是本文档对本轨最锋利的一刀：

> **赢家压倒性地在钦定一台抽象机，而不是推迟给宿主的那台。**
> 推迟要求你建模「所有宿主变化的并集」（无界义务）；钦定要求宿主适应你（有界义务，每宿主付一次）。
> **钦定的价签是一条硬 FFI 边界，每个赢家都显式付了。**

`design-neutral-ir-experiment.md` §1.1 目前的写法是**纯推迟派**：IR 不得编码任何 ABI/布局事实，
一切交给降级。**按这张表，纯推迟派的历史战绩是 ANDF（死）+ EBC/CIL 托管内部（只在
「原生 ABI 从不需要被满足」的围墙花园里活）。** 这不是说实验白做——而是说**实验的边界条件应当改一下**：

| 区域 | 应当采取 | 理由 |
|---|---|---|
| **载荷内部**（载荷函数之间、载荷自己的数据结构） | **钦定一台抽象机**：定死字长模型、定死载荷内部调用约定、定死自有类型的布局规则 | 有界义务。历史上全部赢家的做法。**中立性不需要在这里花钱**——反正只有我们的降级器会看它 |
| **原生边界**（原语 ③④ 所在处） | **声明式、狭窄、有意难跨**——即一份显式的签名/布局描述符 | 这正是 JVM 的 JNI、wasm 的 imports、CIL 的 P/Invoke、SPIR-V 的 layout decoration 所在的位置。**我们的原语 ④ 已经是这个东西了，只是我们没把它当作「中立性的定价点」来对待** |
| **仅在边界上** | 才需要「按目标描述解析」（token / 重定位，见 9.5） | ANDF 的失败恰恰在于**把这套机制推广到整个 OS 表面**，最大一笔复杂度花在这里、仍然没覆盖长尾（§2.2） |

> **给中立 IR 实验的具体建议**：把判据 ① 的问题从「IR 能否对 SysV 与 Win64 都中立」
> 改成「**IR 内部钦定一套约定后，边界描述符能否被独立降级到两个 ABI 且都正确**」。
> 前者是在测一个历史上输过的命题，后者是在测赢家的命题，而且**更贴近我们实际已有的实现**
> （内核已经在做 `sysv64→win64` 桥接——那就是边界 thunk，就是 ARM64EC 的做法）。

### 9.5 我们漏掉的一种手段：**按目标描述做装载时绑定**（限用于边界）

我们的调色板是 FFI / JIT / AOT。历史给出的另外两格是**解释**（§10）与：

> **载荷不携带数值，携带查询；装载器拿目标描述把查询解成数值。**

三个实证：**ANDF 的 unbound token**（§2.2）、**EBC 的 (natural, constant) 偏移编码**（§2.6）、
**eBPF 的 CO-RE/BTF**（§4）。它**恰好落在我们目前二选一的中间**：

| 做法 | 载荷里有什么 | 我们的态度 |
|---|---|---|
| IR 编码布局 | `offsetof(S,f) == 24` | 已禁止（= Bitcode 的病，§2.4） |
| **按描述做装载时绑定** | 「S 的字段 f 的偏移」这一条**查询/重定位** | **我们没考虑过** |
| IR 禁止观察 | 根本不许提 `offsetof` | 我们现在选的（中立 IR §1.2） |

**在 9.4 的框架下限用于边界，它改善两处**：

1. **原生类型的布局**：我们必须**满足**平台结构体（`STARTUPINFOA`、`struct stat`）时，
   目前只能在适配器里硬编码偏移——那是 Bitcode 的病的小型版。改成查询式绑定就消失了。
   现役实例与其价签：**Objective-C 非脆弱 ivar**（`objc_ivar_offset` 间接层，解决脆弱基类问题）、
   **Swift 的 library evolution / resilience**（字段偏移全局量 + value witness table），
   代价是**每次字段访问多一次取偏移** ◐中。
2. **命名轴（9.2 的缺口）**：同一机制直接适用。载荷不写 `"CreateFileW"` 字面量，
   写一个抽象能力 token；由**核外的**绑定表按目标解成具体符号 + 具体库。
   **这与「封装」的区别必须写清**：封装是内核提供 `open()` **语义**；token 绑定是**载荷说出它要什么**，
   **绑定表在核外、可缺席、可替换**，内核仍然只有 ③ 的裸可达性。**中立 IR §1.2 的禁令没有被违反。**

**必须同时记住的价签**（§2.2 的反向教训）：**ANDF 把最大一笔复杂度花在这套 token 系统上，
仍然没能覆盖 OS 表面的长尾。** 所以：**用它，但只用在边界上，并且事先接受「长尾不覆盖」**——
长尾的正确处理是**由载荷自己承担**（这本来就是 §1.2 的分工），不是把 token 系统做大。

### 9.6 我们相对历史的真实优劣势

**优势（历史上的失败者都没有的）：**

1. **M = 1**（9.3a）——UNCOL 张力大半消解。
2. **生产者 = 消费者**（9.3c）——ANDF/Bitcode/PNaCl 的经济死因全部不适用。
3. **载荷可再生。** 这是**我们自己也没意识到的最大自由度**：ANDF 要档案级长期兼容，
   PNaCl 必须**冻结** LLVM IR 子集（并为此付出巨大工程量），Bitcode 死因之一是格式跨版本不稳。
   **而我们的载荷随时可以从源重新降级。** 于是**我们不需要 IR 的前向/后向兼容，
   也不需要冻结格式**——历史上压垮这条路的一大笔成本，我们可以直接不付。
   > 推论：不要给 IR 加版本号协商、不要设计兼容性策略。**要的是「重新生成」而不是「兼容」。**
4. **不需要生态强制力**——不必说服任何 ISV，因此可以像 Apple 一样「敢砍长尾」，
   却不需要 Apple 的市场地位。

**劣势（必须承认的）：**

1. **没有硅片。** §3.9 死因 #3：内存模型只能在硬件里解。我们若跨内存模型强度移动载荷，
   要么付满屏障，要么错。**建议把「载荷的内存序契约」显式写进 IR，而不是留给推断。**
2. **没有生态强制力（对平台而言）。** 我们更像微软而非 Apple：载荷要与不由我们编译的
   平台代码在同一进程共处 → ARM64EC 式的边界 thunk 是必经之路，不是可选优化。
3. **平台在收回 JIT 权限**（§7）。原语 ①② 的地基在被侵蚀。
4. **TCB 与验证不可兼得。** eBPF 证明「装载时验证不可信字节」可行，但
   `kernel/bpf/verifier.c` = **20,065 行**（✅ 查证），比我们整个内核大三个数量级；
   **用户态 eBPF 运行时干脆不做**（rbpf 的 `verifier.rs` 只有 13 KB）。
   ⑤（TCB）与「执行 agent 自产代码」之间没有便宜的桥。可达的组合是
   **签名 + OS 隔离 + 解释**（§8.5、§10.2、§10.9）。
5. **体积目标低于这个空间里每一个已发货的数据点。** ✅ 查证：
   **WAMR 的 AOT 运行时约 29.4 KB** 是「真正带模块装载器 + 内存模型 + 调用 ABI」的经验地板；
   Wasmtime 最小嵌入约 300 KB flash；Cosmopolitan 2020 年的 16 KB hello world 是靠
   「只支持 x86-64 + `-nostdlib`」买来的。
   > **我们实测的 2.5–4.6 KB 之所以能低于这条地板，正是因为我们*不*带验证器、*不*带 libc、
   > 且**尚未**承担 §7.2 的 publish 义务与 §10.5 的 describe 义务。**
   > **这不是反驳，是记账：那条地板告诉我们「补齐契约之后」体积会往哪个方向走。**
   > 补齐 ⑤ 的成本应当被单独量，而不是被惊讶。

---

## 10. 我们没考虑过、但应该考虑的手段

**排序判据：这件事有多大概率在设计冻结之后才被发现。** 不是「多有趣」，是「多晚才咬人」。

### 10.1 ⭐ copy-and-patch stencil —— 「不发编译器却能生成原生码」

见 §7.4。**为什么它排第一**：它同时解决我们两个未解问题——
「降级器放核内还是核外」（放核内也只有几百字节的 memcpy + 重定位应用器）与
「降级器体积会不会随目标数线性增长」（增长落在 **stencil 表**上，而表恰好是**分发时能剥掉的**那个东西，§5）。
而且 §8.1 给出的重构是：**它就是 KB 级的 Futamura 第一投影**——特化在构建期由真编译器离线做掉了。

**要付的三笔**：① 构建期需要 clang/LLVM（不是任意 C 编译器），因为需要 `ghccc`/`preserve_none` 类调用约定；
② 每 ISA 一个约 8–15 种重定位的应用器（各约 200–400 字节）；
③ **原语 ① 必须增加「放置约束」**（见 10.3）。

### 10.2 ⭐ 解释，作为**一等执行层**而非退路

见 §7.5、§7.1、§8.5。**为什么它排第二**：它是**三个平台上唯一合法的路径**
（iOS、ACG 硬化的 Windows 进程、非 `wxallowed` 的 OpenBSD），
**同时**是唯一同时做到「小（2–20 KB）、安全（解释器由构造即是验证器）、可移植」的选项。

**证据链**：Wasmtime 自己在 Cranelift 下面重新引入了 Pulley 解释器且产物更小（§4.2）；
ACPI AML、cBPF、EBC、FCode、Java Card 全是解释执行（§8.5）；
eBPF 的 kfunc ⇒ 必须 JIT 这条耦合说明「可解释性」是会被可达机制悄悄毁掉的资产（§4.1c）。

> **给我们的具体动作**：**载荷格式必须能被降级到三种实现**（直接 RX / 跨进程共享 section / 解释）。
> **事后给一个 JIT 形状的设计补解释层，比反过来痛苦得多。**

### 10.3 ⭐ 原语 ① 缺一个**放置约束**参数

`R_X86_64_PC32` 要 ±2 GB；`R_AARCH64_CALL26` 要 ±128 MB。
一个只会说「给我 N 字节」的内存原语会给你一块**够不到自家运行时符号**的缓冲，
**而失败模式是静默截断的重定位，不是错误**（§7.4）。
**这是每个人第一次移植到 AArch64 时都会撞上的坑**，而我们目前的原语 ① 签名里没有它。
修法很小：`mem_reserve(size, near: Option<addr>, reach: Option<u64>)` + veneer/thunk island 策略。

### 10.4 ⭐ **按目标描述做装载时绑定**（CO-RE / TDF token / EBC 自然索引）

见 §9.5、§4.1e、§2.2、§2.6。规则一句话：
**载荷携带「名字 + 类型描述 + 重定位记录」；装载器携带「布局神谕」；偏移在装载时计算，从不随载荷发出。**

**关键的成本规律（§7 综合，本文档最强的单条规律之一）**：

> **推迟布局的代价为零，当且仅当下游有东西把解析结果*常量折叠*掉。**
> Java/.NET/eBPF-CO-RE 代价为零，因为 JIT 或装载器把数字烘焙进去了；
> Objective-C 非脆弱 ivar 付一次 load；**Swift resilience 付真实的百分比，因为它必须支持
> 在布局存在之前就编译好的调用方、无法再特化。**
>
> **对我们**：**把布局解析放在*降级*步骤里（CO-RE 式修补进生成的代码），永远不要放在*执行*路径里（Swift 式间接）。**
> **copy-and-patch 与 CO-RE 在这里完美复合——一个布局孔就是又一个孔。**
> 并且**从第一天就发 `@frozen` 的等价物**，Swift 的经验说你一定会需要那个逃生舱口。

### 10.5 ⭐⭐ **第五种机制类：「Declare」（发布 + 询问）**

**这是对任务书「有没有我们漏掉的第五种手段」最直接的回答，也是本文档的主观核心结论。**

四条原语全部关于**做**：拿内存、跑代码、找东西、调东西。
而 §7.2 里每一条、以及 §4.1e 的 CO-RE，都是关于**元数据在核与平台之间双向流动**
——**这在一个「只有做」的模型里是不可见的，这正是它被漏掉的原因。**

第五类是双向的：

**5a. Publish —— 把你造的代码告诉平台。**
unwind 表（`RtlAddFunctionTable` / `__register_frame`）、调试与符号信息（jitdump、GDB JIT 接口、ETW）、
CFI 落地指令（`ENDBR64` / `BTI` / PAC 签名）、**指令缓存一致性**（`clear_cache` / 跨线程 `ISB`）、
CFG 目标注册，以及以上全部的**生命周期/退役对偶**。
**不发布，你的代码能跑，但是不可穿越展开、不可 profile、不可调试，在某些平台上根本不可执行。**

**5b. Describe —— 向平台询问它自己。**
ISA 与特性位、页尺寸与分配粒度、ABI 变体（SysV vs Win64 vs Apple-arm64-varargs；硬浮点 vs 软浮点）、
字节序、指针宽度、`long double` 表示，以及——CO-RE 的教训——**你打算与之互操作的那些东西的类型布局**。
> **我们的原语 ③「可达」把*符号解析成地址*。它回答不了「这台机器上 `offsetof(struct stat, st_mtime)` 是多少」，
> 而且再多的 `dlsym` 也回答不了。**
> **CO-RE 之所以work，仅仅因为内核发布了 BTF。若目标没有机器可读的类型描述，
> 我们的载荷只能猜、或者自带逐目标逻辑——没有第三条路。**
> **这是本次调研最深的一条约束，而四条原语一条都没碰它。**

**两条支持论证**：

- **经验的**：Drawbridge 的 PAL、Gramine 的 PAL、wasm/WASI 的宿主接口，
  **三个独立团队在「一个通用程序需要的最小宿主接口」上各自收敛到约 30–50 条，不是 4 条**（§8.3）。
  **4 条只有在你甘心做「一个带 FFI 的解释器」时才可达**——而按 §8.5，那也正是唯一小 TCB 安全的形状。
  **一个 JIT 形状的核需要 publish 集合。**
- **结构的**：原语 ①② 的写法暗示 `protect(RX)` ⇒ 「可执行且可见」。
  **在 ARM/RISC-V 上不成立（一致性）、在 CET/BTI 下不成立（落地指令）、
  在 CHERI 下不成立（能力派生）、在 Windows 上对非叶函数不成立（unwind 注册）。
  「字节被标为可执行」与「平台会让你执行、且事后行为正常」之间的那个缺口，就是第五条原语。**

**修订后的原语集（建议）**：

| # | 原语 | 相对现状的修订 |
|---|---|---|
| ① | **内存** | reserve/commit/protect **+ 放置约束**（近 X、±R 内） |
| ② | **执行** | 交出控制权，**且 publish-before-execute 是强制配对步骤**（一致性 + 落地指令 + unwind 注册）；**并承认它有三种实现**（直接 / 跨进程 / 解释） |
| ③ | **可达** | 符号与 syscall（**注意 OpenBSD 禁止从生成代码发 syscall**，§7.1） |
| ④ | **调用** | 数据描述的签名，**必须能区分指针与整数**（10.6） |
| ⑤ | **Declare** | **新增**：向平台发布生成代码的元数据；向平台询问 ISA/ABI/布局描述 |

> **这不违反 §1.1「不许为方便加原语」**：⑤ 不是为了方便，是因为 ①② 的契约在真实平台上不成立。
> 但**必须按 §1.1 的要求如实记录为「实验检测到的病」**，交由决策者判断是补契约还是拆成第五类。
> **本文档的立场：写成第五类更诚实**——因为它是双向的元数据流动，与①②③④的「做」在性质上不同。

### 10.6 签名描述必须能说「这个字是指针」

一个参数一位。**现在免费。** 需要它的四条独立理由（§8.4）：
**CHERI 溯源、精确 GC 根扫描、Android MTE 带标签指针、ILP32-on-LP64 / wasm 的 32 位指针。**
**一份说不出「这是指针」的签名描述，在四条与 CHERI 无关的理由上都是欠规格的。**
**而这是载荷存在之后就撤不回来的决定。**

### 10.7 「选交集，别选并集」—— eBPF 寄存器模型的可迁移做法

见 §4.1a。eBPF 的 11 寄存器是**真实 64 位 ABI 的交集**，不是中立选择；
它故意在两处牺牲中立（11 寄存器、32 位零扩展）并让边缘架构走解释器，
**换来的是「JIT 是一次查表而非一个寄存器分配器」，逐架构 JIT 约 4k 行**。
这与 §9.4「内部钦定」是同一条原则的两个表述，且是**现役的、可测量的**。

### 10.8 降级产物持久化（FX!32 → XTA cache → Rosetta 2 的三次独立收敛）

§3.9 的「最高杠杆的便宜把戏」。1997 年 FX!32、Windows XTA cache、Rosetta 2
**三次独立收敛到同一件事**：把翻译/降级结果按映像持久化到磁盘。
我们目前是「每次装载都降级」。**Transmeta 的死因之一正是预热成本以延迟尖峰的形式暴露给用户**（§3.3）。
代价见 §4.2：**AOT 产物比中立产物大 3–4×，且要带 target**——所以这是缓存，不是分发格式。

### 10.9 值得知道但**明确不适用**的（防止有人重新提出）

| 手段 | 为什么不适用 |
|---|---|
| **自建验证器** | 内核 eBPF verifier 20,065 行、持续产 CVE；用户态 eBPF 运行时**根本不做**（rbpf 的 `verifier.rs` 只有 13 KB）。**要 eBPF 的安全就得不到 eBPF 的体积。**（§4.1f、§8.5） |
| **PCC / 携带证明的代码** | 证明体积、TCB 其实不小、只有 certifying compiler 产得出证明（**那你已经控制生产者了，签名便宜得多**）、而且人们真正怕的东西不是形式安全性质。（§8.5） |
| **Futamura 投影 / 部分求值（直接用）** | 结构性地是大运行时的想法。**但 copy-and-patch 是它的 KB 级形态，见 10.1。**（§8.1） |
| **Cosmopolitan 路线** | 已被 `design-dynamic-core-experiment.md` §6 排除（它给你一个 POSIX）。**本次调研确认该排除正确**，并补上了三样可单独借鉴的机制（§6.2） |
| **polyglot 可执行文件** | 解决格式，不解决 ISA/ABI/布局；只有一种格式能拥有偏移 0；宿主启动器是敌对表面（§6.3） |
| **跨 ISA 代码去重以打败 fat 的 N×** | **从未有系统做到，且在字节层面不可能。** 唯一真实的缓解是「把选择推到分发时」与「砍掉旧架构」（§5） |
| **CHERI 适配（现在就做）** | 无主流应用处理器发货。**但 10.6 的保险费几乎免费，现在就付。**（§8.4） |

---

## 11. 可信度与本次调研的方法学缺陷

### 11.1 必须先说的缺陷

**`WebSearch` / `WebFetch` 全程不可用**（每次调用都返回
`There's an issue with the selected model (haiku)`）。多数调研路径上直连出口也不通
（`curl` 超时、DNS 返回被污染结果、无代理）。

**但覆盖度不是均匀的，引用时必须按节区分**：

| 节 | 查证状态 |
|---|---|
| **§4（eBPF / wasm）、§6（OS 轴 / Cosmopolitan / polyglot）** | ✅ **绝大部分已于 2026-08-08 对活的一手来源查证**（内核源码与 kernel.org 文档、GitHub API 取的文件尺寸与 commit、Microsoft Learn、FreeBSD Handbook、Wasmtime/WAMR 文档、justine.lol）。**这是全文档可信度最高的两节。** 例外：§4.3（ART/APK，相关站点被屏蔽）、用户态 eBPF 的**编译后**体积、当前 `cosmocc` hello-world 体积 |
| **§2（中立 IR 失败史）、§3（二进制翻译）、§5、§7、§8** | ○ **未经本次联网查证**，来自模型知识（截止约 2026-05）。**结构性分析（根因、模式、对照）不依赖具体数字，可信度较高；年份、人名、版本号、体积/百分比可信度较低**，文中已刻意避免印出多数具体数字 |
| **§9、§10** | 我方判断与推论，其可信度继承所引各节 |

任务书要求「用 WebSearch/WebFetch 真去查」——**这条只在 §4/§6 上被实质满足**（且是靠绕路），
其余各节未满足，不是因为省事，是因为不可用。**这一点必须随文档一起被引用。**

### 11.2 可信度等级

| 标记 | 含义 |
|---|---|
| ✅ 高 | 广为记载、多处独立佐证的事实，或不依赖数字的结构性推理 |
| ◐ 中 | 实质正确，但某个日期/版本/命名可能有偏差 |
| ○ 低 ⚠ | 合理重建，**用于决策前必须查证**；文中已尽量避免印出具体数字 |

### 11.3 联网恢复后的**查证清单**（按对本轨的杠杆排序）

**高杠杆（会改变设计判断）：**

1. **EBC 的 (natural, constant) 偏移编码** —— 查 UEFI 规格的 EBC 章节。
   本清单上最容易查、价值最高的一条：它是 §9.5 与 §10.4 建议的直接工程范本。
2. **各平台 2026 年的 W^X / JIT 权限现状** —— 原语 ①② 的地基（§7.1）。
   尤其：Windows ACG 是否阻止 `VirtualProtect` 到 RX；macOS `MAP_JIT` +
   `com.apple.security.cs.allow-jit` 的当前要求；Linux `MFD_NOEXEC_SEAL` / `MemoryDenyWriteExecute`
   在主流发行版与容器运行时里的默认值；**OpenBSD 的 syscall 来源限制**（这条若成立，
   我们的原语 ③ 在该平台上从生成代码里发 syscall 即死）。
3. **copy-and-patch 的实测编译速度与代码质量倍数**，以及 CPython 3.13+ 的 stencil 表体积与当前默认状态（§7.4、§10.1）。
4. **ANDF 的技术 vs 政治之分** —— 最佳桥梁文献是 Stavros Macrakis,
   *From UNCOL to ANDF: Progress in Standard Intermediate Languages*（OSF Research Institute, 约 1993）◐中。
   §9.3(c) 的整条结论压在这个分野上。
5. **Windows x64 unwind 注册的最小合法集**（哪些生成函数真能按叶跳过）与
   `RtlAddGrowableFunctionTable` 的使用条件（§7.2）。

> **已经查证、不必重查的**（省时间）：eBPF 的 11 寄存器映射与 helper 冻结在 211 / kfunc 319+、
> 验证器常量（1M / 8192 / 64 / 33）与 4096→1M 的 commit、`verifier.c` 20,065 行、
> CO-RE 的机制与 BTF 相对 DWARF 约 100× 的压缩、WAMR AOT 约 29.4 KB、
> Wasmtime `.cwasm` 比 `.wasm` 大、`CanonicalABI.md` 5,411 行、WSL1/Wine/Linuxulator 的厂商原话、
> illumos LX 不在上游、APE 的机制与毛边。**以上均于 2026-08-08 查证。**

**中杠杆（会改变文中措辞，不改变判断）：**

6. SPIR 1.x「就是 LLVM IR + metadata」—— §2.7 #3 的独立佐证，须确认。
7. Apple bitcode 与 watchOS armv7k→arm64_32 的关系 —— **广泛流传、未经证实**，
   本文档已按未证实处理，查证后可确认或删。
8. ARM64EC 被保留/弃用的寄存器清单（常引 x13/x14/x23/x24/x28、v16–v31）—— 查 Microsoft Learn 的 ARM64EC ABI 页。
9. Rosetta 2 在 macOS 26/27 之后的收窄条款 —— 新闻依赖，**未经查证不得引用**。
10. Franz 的 slim binary 压缩方案名称与全部体积比例；PNaCl 的 SFI 开销百分比与翻译器体积；
    QEMU TCG 与各翻译器的体积数字。**文中已刻意不印这些数字。**

**低杠杆（学术性）：**

11. CACM 1958 的期号页码与六位作者的机构；Conway 的独立提案是否单列。
12. **任何 Steel 的引语** —— 本文档**刻意一句未引**，不要让一句凭记忆的引语混进来。
13. OSF 发 RFT（1989）与选中 TDF（1991）的确切年份。

### 11.4 本文档的维护约定

- 这是**参考**不是任务单：**不设时间盒，允许长期增补**；但每次增补必须带可信度标记。
- **查证过的条目请改标 ✅ 并注明来源名**（不要只写 URL，URL 会烂）。
- **不要把本文档做成教科书。** 判据同 §0：一条技术若既不能用、也不能借鉴、也不构成警告，删掉它。

---

*研究轨参考资料。不承诺版本归属，不改 PRD 能力状态。*
