# ⚠️ 已归档：dynamic-core 已判决实验规格

> **归档于 2026-08-10。** Q0–Q23 研究轨已经封闭，综合结论与重新开启条件由
> `research/dynamic-core/SYNTHESIS.md` 和 `research/dynamic-core/README.md` 拥有。
> 本文件只保存该问题的实验前判据与历史结果，不是活跃版本任务。

# Q8 — 可执行内存地基还剩多少：原语 ①②③ 的实测可用性（历史规格）

> ⚠️ **不是 AgenTerm 产品范围。** 动态核研究轨的一条实验（见
> `research/dynamic-core/README.md` 的 Q 索引）。不进任何版本 plan 的 must-ship，
> 不改 `PRD.md` 能力状态。

| 字段 | 值 |
|------|-----|
| **日期** | 2026-08-08 |
| **目的** | 用**实测**（不是引用综述）回答：四原语里的 ①（内存 RW↔RX）②（跳进那段内存）③（裸 syscall + 符号解析）在今天的主流平台上实际还剩多少可用空间；缺口在哪、是策略可配置的还是硬性的、有没有合法替代路径 |
| **实现位置** | `research/dynamic-core/platform/`（**不挂进根 workspace**） |
| **前置阅读** | `plan/reference-cross-target-execution.md` §7.1–7.2（发现来源）；`design-dynamic-core-experiment.md` §1.1（四原语定义）；`.claude/skills/decisive-experiment/SKILL.md` |
| **来源纪律** | **从零探索。** 不照搬任何既有实现源码；Win32 契约取自公开文档与本机 Windows SDK 头文件 |
| **可信度纪律** | **本机是 Windows Server 2022 / x86_64 真机**——Windows 侧必须是实测。Linux/macOS/iOS/OpenBSD 本机无法实测（无 WSL）：能找到本地一手依据就用，否则明确标为「未验证的转述」，**不把综述里未核实的论断升级成本实验的结论** |

---

## 0. 背景与已确定的事

综述 `reference-cross-target-execution.md` §7.1 指出：整个动态核压在原语 ①②③ 上，
而**平台正在系统性地收回这块地基**（iOS 无合法路径、Windows ACG、systemd
`MemoryDenyWriteExecute`、OpenBSD 从生成代码发 syscall 即杀进程）。一句话：
**「标为可执行」≠「平台会让你执行」。** 但综述作者的 WebSearch/WebFetch 全程失败，
上述论断**未经联网核对**（§11 已标注）。这条轨的方法本来就是**测量而非引用**，
且这些论断**恰好可以在本机直接测**。

### 已确定、不在本实验讨论范围内的事

1. **原语 ③ 在 Windows 上「裸 syscall」半边本就不用**：Q0 内核（`core/kernel.rs`）在
   `#[cfg(windows)]` 下 `raw_syscall` 直接 `return -1`；Windows 侧的「可达」= 符号解析
   （`LoadLibraryA`/`GetProcAddress`）。所以 Windows 实测里 ③ 只测「符号解析」这半边，
   「裸 syscall」半边的实测靶场是 Linux/OpenBSD（本机测不了）。
2. **Q0 的 Windows 内核走的是哪条路，已由读源码确定**：`load_and_run` = `VirtualAlloc(RW)`
   → 写入 → `VirtualProtect(RX)` → 跳入（即本实验的 M1）。不用 RWX 一步到位、不用 section 对象。
3. **本实验不改四原语的实现**，只测它们在不同平台策略下成不成立。

---

## 1. 硬约束（违反则实验无效）

1. **Windows 侧结论必须来自本机运行的可执行文件**，每个数字第三方可复跑，命令写进 `RESULTS.md`。
2. **非 Windows 平台的任何结论必须带可信度标签**：`实测` / `本地一手依据` / `未验证的转述`。
   诚实标注可信度分层**是本实验的交付物之一**，不是免责声明。
3. **病灶探测器**：任何「为了让架构好看而低估缺口」或「因为综述吓人而放大缺口」的冲动，
   都是本实验要**检测**的病。如实测、如实报。若实测与综述矛盾，以实测为准并点名。
4. **时间盒**：Windows 侧把 ①②③ 的可用性、ACG 的默认态与失败方式、至少一条替代路径测出数即停。
   **不为 Linux/macOS 去装 WSL，不跨平台大扫荡。**

---

## 2. 最小实验内容

| 维度 | 选择 | 理由 |
|------|------|------|
| **平台** | Windows/x86_64 实测；其余平台标注可信度 | 本机只有 Windows 能真跑 |
| **策略轴** | `default` vs `ACG`（`ProcessDynamicCodePolicy.ProhibitDynamicCode`） | ACG 是综述点名「把两条候选路一起断掉」的那个 |
| **代码执行机制** | M1 `RW→RX 翻转`（Q0 内核路）、M2 `直接 RWX`、M3 `section 对象映射可执行` | 覆盖「①②」的全部已知申请方式，才能说清 ACG 断的是一条还是全部 |
| **条件路径** | T1 `ACG 前置 RX 是否在 ACG 后仍可执行`、T2 `AllowThreadOptOut + 线程退出` | 测「有条件能」的两条合法窗口及其代价 |

每个机制用同一段机器码桩（`mov eax,42; ret`）验证「真的跳进去执行且返回正确值」，
不是只看 API 返回码。

---

## 3. 判据（动手前钉死，事后不得改）

| # | 判据 | 度量 | 性质 |
|---|------|------|------|
| **J1** | ①② 在 **default 策略**下可用？ | M1/M2/M3 是否都跑到「返回 42」 | 布尔门 |
| **J2** | ①② 在 **ACG** 下的失败点与错误码 | 每个机制在哪个 API 调用失败、`GetLastError` 值 | 清单 |
| **J3** | ACG 是**默认开**还是**opt-in** | 进程启动时 `GetProcessMitigationPolicy` 读到的值 | 布尔门 |
| **J4** | 缺口是**策略可配置**还是**硬性** | ACG 能否被关掉/绕过；是不是要显式启用 | 清单（架构含义相反） |
| **J5** | 合法替代路径**存在性 + 代价** | T1/T2 是否成立；各自的字节/复杂度/权限代价 | 清单 |
| **J6** | ③「符号解析」半边在 Windows 是否受这些策略影响 | ACG 下 `GetProcAddress` 路径是否仍可达 | 布尔门 |

### 度量纪律

- Windows 产物用 `rustc -O`（release），本机 `x86_64-pc-windows-msvc`，rustc 版本记进 `RESULTS.md`。
- 每条结论标机制（M/T 编号）与错误码；错误码名以本机 Windows SDK `winerror.h` 为准。
- 非 Windows 结论一律带可信度标签（约束 1.2）。

---

## 4. 判决规则与 kill criterion

这不是「A vs B 选一个」的实验，主产出是**一张缺口清单 + 一层可信度标注**，
但仍有一个决定架构走向的布尔判决：

```
1. 主判据 = J3（ACG 是否默认开）。
   - 若 ACG 默认开且不可关 → 原语 ①② 在 Windows 默认部署下即不可用
     → 「JIT 进 RWX」从主路径降级为「有条件快路径」，解释执行必须是一等层。判「地基动摇」。
   - 若 ACG 是 opt-in 且我们可以不开 → 降级为「部署前提」：默认能跑，硬化场景需替代路径。
2. J1 若在 default 下不过（连默认都不让 RW→RX）→ 立即「地基动摇」，不看其它。
3. J2/J4 给缺口清单：每条缺口标「策略可配置 vs 硬性」——两者架构含义相反。
4. J5 给替代路径与代价。若某平台**一条合法替代都没有**（综述称 iOS 如此）→ 该平台上
   原语 ①② 判「硬性缺失」。
kill criterion: 若 default 策略下 M1（Q0 内核路）都跑不通 → Q0 的 Windows 实测结论与本机矛盾，
   先查环境再下结论。
时间盒: 做到 J1–J5 在 Windows 出数 + iOS/OpenBSD/Linux 缺口按可信度标注为止。
```

---

## 5. 目录结构

```
research/dynamic-core/platform/
├─ probe_win.rs    ← default vs ACG × {M1 RW→RX, M2 RWX, M3 section}
├─ probe_win2.rs   ← T1 前置RX存活 / T2 AllowThreadOptOut 线程退出
├─ RESULTS.md      ← 逐判据数字 + 复跑命令（最重要的产出）
└─ out/            ← 构建产物（git-ignored）
```

---

## 6. 已排除的选项

| 选项 | 为什么排除 |
|------|-----------|
| **为测 Linux/macOS 装 WSL/虚拟机** | 超时间盒；综述里 Linux/OpenBSD 论断可在别处的真机测，本机只诚实标注 |
| **手搓一个签名 PE 走 SEC_IMAGE 测 CIG** | CIG 是另一条策略（签名，非本实验的 ①②）；构造签名 PE 成本远超时间盒 |
| **完整实现跨进程 JIT** | 是替代路径之一，但重；本实验测「存在性 + 代价形状」，不做产品级实现 |
| **把结论写成「综述说 iOS 无路」** | 违反约束 1.2；iOS 本机测不了，只能标「未验证的转述」 |

---

## 7. 本实验不回答的问题

- Linux `MemoryDenyWriteExecute` / `MFD_NOEXEC_SEAL`、macOS `MAP_JIT` entitlement、
  OpenBSD `msyscall`/pinsyscalls 的**实测**（本机无对应 OS）。
- 跨进程 JIT 的产品级实现与性能。
- 解释执行退路的体积/性能（那是「若地基动摇成立」之后的下一个实验）。
- I-cache 跨线程一致性、CET-IBT/BTI/PAC 落地指令、x64 unwind 注册的**执行后正确性**
  （综述 §7.2；本实验只测「能不能跳进去」，不测「跳进去之后行为是否正常」）。

---

## 8. 结论回填

见 `research/dynamic-core/platform/RESULTS.md`（第三方可复跑形态）与本轨 `README.md`
Q8 行。要点：**ACG 在本机是 opt-in（默认 0x0），一旦启用把 M1/M2/M3 三条路一起断
（`ERROR_DYNAMIC_CODE_BLOCKED` 1655）；存在两条有条件的合法窗口（前置 RX 存活、
AllowThreadOptOut 线程退出）。结论是「地基是部署前提，不是默认动摇」——但硬化进程与
iOS 是硬性缺口。** 完整判决 trace 与可信度分层见 RESULTS.md §8。

---

*研究轨投影。不承诺版本归属，不改 PRD 能力状态。*
