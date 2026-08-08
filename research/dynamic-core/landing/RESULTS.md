# Q12 RESULTS — the landing gate: which "second-gate" hazards actually bite (Windows/x86_64)

**规格**：[`plan/design-landing-gate-experiment.md`](../../../plan/design-landing-gate-experiment.md)

Q8 measured the **first gate** — can a page be made executable and jumped into. This
experiment measures the **second gate** the survey (`reference-cross-target-execution.md`
§7.2/§7.4) lists but nobody tested: once you jump in, do the *bytes* run — CET-IBT/ENDBR64
landing pads, I-cache coherence, Windows unwind registration, and the ±2GB placement
truncation. Every number below is **measured on this box**, not cited.

## 度量条件

| 项 | 值 |
|----|----|
| 机器 | Windows Server 2022 Datacenter 10.0.20348 (**真机**) |
| ISA / 目标 | x86_64 / `x86_64-pc-windows-msvc` |
| 编译器 | `rustc 1.97.0 (2d8144b78 2026-07-07)`, `-O` |
| 依赖 | 无外部 crate；`kernel32` FFI + `CPUID.7.0` |
| 错误码 | `1655 = ERROR_DYNAMIC_CODE_BLOCKED`（本机 SDK `winerror.h`，同 Q8） |

## 复跑命令

```sh
P=research/dynamic-core/landing
rustc -O $P/probe_landing.rs         -o $P/out/probe_landing.exe
rustc -O $P/probe_reach.rs           -o $P/out/probe_reach.exe
rustc -O $P/probe_interp_immunity.rs -o $P/out/probe_interp_immunity.exe

$P/out/probe_landing.exe            # ① CET/IBT state + ② our products' patterns
$P/out/probe_reach.exe near         # ③ near (<2GB) control
$P/out/probe_reach.exe far          # ③ far (>2GB) silent truncation
$P/out/probe_interp_immunity.exe    # ⑤ interpreter immunity under ACG
```

Machine-code stubs are hand-assembled; each returns 42 when it truly runs.

---

## ① 本机 CET / IBT 现状 → **硬件不支持，策略未开，IBT 不咬人**（实测）

`probe_landing.exe`:

| 度量 | 值 | 含义 |
|------|----|----|
| `CPUID.7.0 ECX[7]` CET_SS | **0** | CPU **不支持**影子栈 |
| `CPUID.7.0 EDX[20]` CET_IBT | **0** | CPU **不支持**间接分支跟踪（IBT）——本机硅片根本没有 CET |
| `GetProcessMitigationPolicy(ControlFlowGuard)` | `0x0` | 软件 CFG 未启用 |
| `GetProcessMitigationPolicy(UserShadowStack)` bit0 | 0 | 影子栈未启用（`0x100` = `CetDynamicApisOutOfProcOnly`，非 enable 位） |

**关键**：本机 CPU 的 CPUID 直接报告 **CET_SS=0 / CET_IBT=0** —— 不是"支持但没开"，是
**硬件层面就没有 CET**。因此 CET-IBT/ENDBR64 这一关在本机**根本无从触发**。

**双测姿态的诚实说明（对照 Q8）**：Q8 能"默认测一遍、再显式启用 ACG 测一遍"，因为 ACG 有
运行时开关 `SetProcessMitigationPolicy(DynamicCode)`。**前向边 IBT 在 Windows 上没有对应的
运行时开关**——它需要 OS/loader 为用户态兑现 CET-IBT，而当前 Windows 并不对用户态强制前向边
IBT（只有影子栈这条后向边，且需 `/CETCOMPAT` 链接 + CET 硬件）。所以 ② 的**实测**（把无 ENDBR
的目标真跳进去看它跑不跑）就是这条关卡的直接判据，而非查文档。

## ② 我们的产物踩线了吗 → **每一处间接跳转目标都无 ENDBR64，但本机全过**（实测）

**静态**（读 `lowering/lower.rs` + 扫描产物二进制）：

- 扫描 `f3 0f 1e fa`（ENDBR64）在 `A_lower_pure_windows.exe`(Q2)、`interp/out/driver.exe`(Q9)、
  `platform/out/probe_win.exe`(Q8) 中 = **0 / 0 / 0**。rustc-msvc 默认**不发**落地指令；我们
  手写发射的机器码里也**一条 ENDBR64 都没有**。
- Q2 `lower.rs` 的间接控制转移只有两类，都无 ENDBR 目标：
  - **入口**：`lower_and_run` = `transmute(code); entry(env)` —— **间接 CALL** 落在 `e_prologue`
    首字节（`push rbx = 0x53`）。
  - **回调**：`e_call_env` = `call qword [r15+idx*8]` —— **间接 CALL** 经内存到 rustc shim。
  - **内部跳转**：`e_jmp`/`e_jcc` = `E9`/`0F 8x` **直接 rel32**，IBT 不管直接分支。
  - 计数：每个降级载荷 = **1 入口间接跳 + N 回调间接跳**（N = OP_CALL 数），ENDBR64 = **0**。

**动态**（`probe_landing.exe`，逐条真跳进去）：

| 模式 | 机制 | 结果 |
|------|------|------|
| P0 | 间接 CALL → 无 ENDBR `mov eax,42;ret` | **返回 42**，IBT 不咬 |
| P0b | 间接 CALL → `ENDBR64`+stub | **返回 42**（IBT 关时 ENDBR 是 NOP） |
| **P1** | **Q2 入口**：间接 CALL → 无 ENDBR 的 `e_prologue` | **返回 42**，Q2 入口不踩线 |
| **P2** | **Q2 回调**：`call qword [mem]` → 无 ENDBR shim | **返回 42**，Q2 OS 回调不踩线 |

- **unwind**：对 P1 生成的代码调 `RtlLookupFunctionEntry` = **NULL** —— 生成帧**没有**注册
  unwind 表。本机结论：这是**潜伏**缺口，不是**当前**缺口——只有当 SEH/C++ 异常**穿过**生成帧
  展开时才 UB；我们的载荷要么直接退出进程、要么正常返回，不触发穿帧展开，故本机不咬。
- **I-cache**：所有 stub 都在同线程写入后立即执行、无显式 `IC/DSB/ISB` 就返回正确值——
  x86_64 硬件自维护 I/D 一致，**这一关在 x86 上不咬**（ARM 才咬，属转述）。

→ **本机上，四原语产物的第二关全部不咬**：IBT 无硬件、无策略；I-cache x86 免费；unwind 潜伏未触发。

## ③ 放置约束的实际边界 → **确认是静默截断，跳到错地方，不报错**（实测，最有价值）

`probe_reach.exe`，back-patch 用 Q2 `lower.rs::patch()` **逐字相同**的
`let rel = (target - (site+4)) as i32`（Rust `as` = 静默截断）：

| 模式 | 全 64 位 delta | 装得下 i32？ | 发射 rel32 | 执行结果 |
|------|----------------|-------------|-----------|---------|
| **near** | −65,541 | 是 | −65,541 | **返回 42**，正确到达 |
| **far** | 3,221,159,931 (~3GB) | **否** | −1,073,807,365（0xbffefffb） | **返回 99** |

far 案例逐字：
- 目标 `0x020eb3180000`，源 S `0x020df3190000`，真实距离 ~3GB **>±2GB**。
- **发射/回填阶段没有任何 API 报错**——`as i32` 悄悄把 3GB 截成 −1,073,807,365。
- 截断后的**有效跳转目标** = `0x020db3180000`，恰好在意图目标**下方 4GB（0x100000000）**。
- 我们在那个错误地址种了个 `mov eax,99;ret` 诱饵 → far 调用**返回 99**：
  **没有崩溃、没有报错、静默跳到了错误的被调方**。

→ 综述 §7.4「失败模式是静默截断的重定位，不是一个错误」**实测确认**，且是最坏形态
（跳到有效但错误的代码，连崩溃都没有）。**但要点是下一条。**

### ③的反转发现：Q2 的降级器**结构上免疫这条关**

综述的 ±2GB 隐患针对的是 **copy-and-patch / 发 rel32-CALL 到运行时符号**的设计。**Q2 不是**：
- Q2 的 OS 回调走 `call qword [r15+idx*8]`——**绝对间接（经内存），无距离限制**，能到任意地址。
- Q2 唯一的 rel32 是**同一 <8KB 代码缓冲内部**的 `jmp`/`jcc`，target−site 永远是几百字节，
  **不可能越 ±2GB**。
- Q2 **从不发射一条指向外部符号的 PC 相对引用**。

→ 放置约束是真隐患，但**恰好不咬我们现有产物**：Q2 用"绝对间接调用 + 缓冲内相对跳"的组合天然
避开了它。这是 Q2 设计的一个**未被点名的正确性**。谁要是改成 copy-and-patch（发 rel32 到 helper），
就会一头撞上这条。

## ④ 修补代价（估算，时间盒内不做实现）

以本机 x86_64、对照 Q2 的 X=3003 B / Q9 的 1908 B：

| 关卡 | 给 Q2 降级器加什么 | 估算代价 |
|------|-------------------|---------|
| **CET-IBT/ENDBR64** | `e_prologue` 首发 `F3 0F 1E FA`（每个"被间接调用的生成入口"4 字节） | **+4 字节/入口，+1 行编码器**。内部直接跳无需。外部回调目标的 ENDBR 由 OS 库自带（/CETCOMPAT），非我方成本 |
| **I-cache 一致性** | x86：无。ARM：emit 后发 `DC CVAU/DSB/IC IVAU/DSB/ISB` + 执行线程 `ISB` | **x86 = 0**；ARM ≈ 每 ISA 10–20 行（本实验不做 ARM） |
| **Windows unwind 注册** | 构造 `RUNTIME_FUNCTION`(12B)+`UNWIND_INFO`(~8–20B) 描述 prologue，调 `RtlAddFunctionTable` | **~40 行 + ~30 字节/函数数据 + 1 次注册调用**；**且当前载荷不触发穿帧展开，可暂缓**（潜伏项）。或走 §7.2 便宜法：把生成帧做成叶帧（不动 RSP）——但 Q2 prologue push 5 个寄存器，非叶，需重构 vreg 分配 |
| **放置 ±2GB** | **无**——Q2 已结构免疫（见 ③反转发现） | **0** |

**净估算**：本机若只上 IBT，Q2 降级器加 **~1 行 + 4 字节**（<X 的 0.2%）；若连 unwind 一起硬化，
**~40 行 + ~34 字节**（≈X 的 1%）。第二关的硬化代价对 codegen 路是**小的**，远不及 X 本身。

## ⑤ 对解释路线的影响 → **解释器天然免疫全部四道第二关**（实测 + 结构证明）

**结构证明**（读 `interp/interp.rs`）：解释器 `run` 用 `match` 遍历 IR，**从不** `VirtualAlloc(RX)`、
**从不** `VirtualProtect(→RX)`、**从不** `transmute` 成函数指针跳进缓冲、**从不**发射重定位。
它唯一的"控制转移"是 Rust `match` 分派与普通编译好的 FFI 调用。因此：

| 第二关 | 需要它的前提 | 解释器有该前提吗 |
|--------|-------------|-----------------|
| CET-IBT/ENDBR64 落地 | 有"间接跳进生成字节"这一动作 | **无**——不生成字节，无处可跳 |
| I-cache 一致性刷新 | 有"写了要执行的字节" | **无**——不写可执行字节 |
| unwind 注册 | 有"OS unwinder 读不懂的合成帧" | **无**——只有 rustc 正常帧 |
| 放置 ±2GB 截断 | 有"要应用的重定位" | **无**——不做任何重定位 |

**实测**（`probe_interp_immunity.exe`，在 Q8 最狠的第一关 ACG 下）：

- **[A]** 净室 match 解释器在 **ACG 开启**下算 `((7*191)^0xABCD)<<3` = **358304（正确）**，
  **零可执行页**、零间接跳生成码、零 unwind 表、零重定位。
- **[B]** 同进程里 Q2 式 codegen 路（`VirtualAlloc RW → VirtualProtect RX`）在 ACG 下
  **被挡，err=1655**——codegen 后端连第一关都过不去，更谈不上第二关；解释器两关全过。

→ **解释路线对第二关的免疫是结构性的、不是配置性的**：因为没有生成代码可供落地/刷新/展开/重定位。
这比 Q9 量到的体积/ISA 优势**更硬**——那些是"更省"，这条是"整类隐患从存在变为不存在"。
④ 里 codegen 路要加的 ENDBR/unwind/放置代码，解释器**一行都不用加**（= 0）。

---

## 判决 trace 与哪些关卡"真的咬人"

| 第二关 | 本机是否咬 | 依据 | 可信度 |
|--------|-----------|------|--------|
| **CET-IBT / ENDBR64** | **不咬** | CPUID CET_IBT=0（无硬件）+ P0–P2 无 ENDBR 目标全过 | **实测** |
| **CET 影子栈（后向边）** | **不咬** | CPUID CET_SS=0 + 策略 bit0=0 | **实测** |
| **I-cache 一致性** | **不咬**（x86） | 同线程写后即执行、无屏障、结果正确 | **实测** |
| **Windows unwind 注册** | **潜伏，未触发** | `RtlLookupFunctionEntry`=NULL（帧未注册），但当前载荷不穿帧展开 | **实测** |
| **放置 ±2GB 静默截断** | **真隐患，但不咬 Q2** | far 案例静默返回 99；而 Q2 结构免疫（绝对间接 + 缓冲内相对） | **实测** |
| **ARM BTI / PAC / IC-DSB-ISB** | 本机测不了 | 无 ARM 硬件 | **未验证的转述** |

**净结论**：综述点名的第二关里，**在本机 x86_64 上今天没有一条真的咬我们的产物**——
CET 硬件缺席、I-cache x86 免费、unwind 潜伏未触发、放置约束被 Q2 设计结构性避开。
这把第二关从"悬着的真隐患"**降级为"未来硬化平台（开 CET-IBT 的 Win / ARM 真机）的部署前提"**，
与 Q8 对第一关的结论同构。**但两条必须写进设计**：
1. **放置约束是最坏的一类（静默、跳到错地方、不报错）——已实测确认**。Q2 现在躲开是因为它不发
   PC 相对外部引用；**任何转向 copy-and-patch 的改动都会当场引爆**，且不会有报错提示你。
2. **解释路线对全部四道第二关结构免疫**——这是继 Q9 体积/ISA 之后解释路线的**第三条**结构性优势，
   且是最硬的一条。硬化平台上，"能生成代码"的核要逐条补 ENDBR/unwind/放置；"解释"的核零负担。

## 哪些是实测、哪些是转述

- **实测（本机）**：① CET 硬件/策略状态；② 四种间接跳转模式全过 + ENDBR64 扫描=0 + unwind 表=NULL；
  ③ near 42 / far 静默返回 99 的截断；⑤ 解释器 ACG 下 358304、codegen 路 1655。
- **结构证明（读源码）**：Q2 只有入口+回调两类间接跳、内部为直接 rel32；Q2 免疫放置约束；
  解释器不碰任何可执行内存 API。
- **转述（未验，本机无硬件）**：ARM BTI/PAC 落地垫、ARM 的 `IC/DSB/ISB` 强制一致性、
  arm64e PAC 签名。综述称"今天在发货"，本机 x86_64 无法证伪。

---

*研究轨投影（Q12）。不承诺版本归属，不改 PRD 能力状态。*
