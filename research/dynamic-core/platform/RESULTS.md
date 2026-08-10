# Q8 RESULTS — 可执行内存地基的实测（Windows/x86_64）

**规格**：[`plan/archive/design-executable-memory-floor-experiment.md`](../../../plan/archive/design-executable-memory-floor-experiment.md)

## 度量条件

| 项 | 值 |
|----|----|
| 机器 | Windows Server 2022 Datacenter 10.0.20348（**真机**，非 VM/WSL） |
| ISA / 目标 | x86_64 / `x86_64-pc-windows-msvc` |
| 编译器 | `rustc 1.97.0 (2d8144b78 2026-07-07)`，LLVM 22.1.6，`-O` |
| 错误码来源 | 本机 Windows SDK `.../10.0.22000.0/shared/winerror.h` |
| 依赖 | 无外部 crate；仅 `kernel32` FFI |

## 复跑命令

```sh
P=research/dynamic-core/platform
rustc -O $P/probe_win.rs  -o $P/out/probe_win.exe
rustc -O $P/probe_win2.rs -o $P/out/probe_win2.exe

$P/out/probe_win.exe  default   # J1
$P/out/probe_win.exe  acg       # J2 J3 J4
$P/out/probe_win2.exe t1        # J5 (前置 RX 存活)
$P/out/probe_win2.exe t2        # J5 (AllowThreadOptOut)
```

每个机制用同一段机器码桩 `mov eax,42 ; ret`（`B8 2A 00 00 00 C3`）验证「真的跳进去且返回 42」，
不是只看 API 返回码。

---

## 逐判据数字（实测）

### J1 — default 策略下 ①② 可用？ → **是（全过）**

`probe_win.exe default`，进程启动时 `GetProcessMitigationPolicy(DynamicCode) = 0x0`：

| 机制 | 结果 |
|------|------|
| M1 `VirtualAlloc(RW)` → 写 → `VirtualProtect(RX)` → 跳（**Q0 内核路**） | **OK，返回 42** |
| M2 `VirtualAlloc(RWX)` → 写 → 跳 | **OK，返回 42** |
| M3 `CreateFileMapping(EXECUTE_RW, pagefile)` → `MapViewOfFile(FILE_MAP_EXECUTE)` → 写 → 跳 | **OK，返回 42** |

→ 默认策略下原语 ①② 是一等能力，三种申请可执行内存的方式都通。**Q0 的 Windows 实测结论在本机复现，走的是 M1。**

### J2 — ACG 下 ①② 的失败点与错误码 → **三条路全断**

`probe_win.exe acg`，运行时 `SetProcessMitigationPolicy(DynamicCode, ProhibitDynamicCode=1)` **成功**，
`GetProcessMitigationPolicy` 读回 `0x1`：

| 机制 | 失败于哪个 API | 错误码 |
|------|---------------|--------|
| M1 | `VirtualProtect(→RX)` 返回 FALSE | **1655 = `ERROR_DYNAMIC_CODE_BLOCKED`** |
| M2 | `VirtualAlloc(RWX)` 返回 NULL | **1655** |
| M3 | `MapViewOfFile(FILE_MAP_EXECUTE)` 返回 NULL | **1655** |

→ 综述 §7.1「ACG 把两条候选路径都断掉」**实测确认，且更强**：连第三条 section-object 路（M3）也断。
三条都返回同一个 `ERROR_DYNAMIC_CODE_BLOCKED`。`winerror.h` 原文：
`// The operation was blocked as the process prohibits dynamic code generation.`

### J3 — ACG 默认开还是 opt-in？ → **opt-in（默认关）**

进程启动时读到的 `DynamicCode` 策略 = **`0x0`**（未启用）。ACG 需**显式**启用
（进程创建时的缓解策略属性，或本机实测：运行时 `SetProcessMitigationPolicy` 亦可，且一旦开不可关）。
**默认部署下 ①② 正常。**

### J4 — 缺口是策略可配置还是硬性？ → **Windows 上是策略可配置**

- ACG 是**逐进程 opt-in**，我们自己的进程可以**不开**。
- 但：ACG 可由**外部**强加（EXE 头缓解标志 / WDAC / AppLocker / 父进程用
  `UpdateProcThreadAttribute(PROC_THREAD_ATTRIBUTE_MITIGATION_POLICY)` 创建子进程时钉死）。
  被外部强加且未带 `AllowThreadOptOut` 时，进程内**无合法自救**（见 J5-T2 的前提）。
- 一旦启用**不可撤销**（`SetProcessMitigationPolicy` 只能开不能关）。

→ 对**我们自己发布的进程**：缺口是策略可配置的，我们控制。对**别人以 ACG 策略托管我们的代码**：
在该进程里是硬性的。

### J5 — 合法替代路径与代价 → **两条有条件窗口**

`probe_win2.exe t1` / `t2`：

| 替代路径 | 实测 | 代价 |
|---------|------|------|
| **T1 前置 RX 存活**：ACG 启用**前**把内存翻成 RX，启用**后**仍跳进去执行 | **成立**（before=42，enable ACG，after=42） | 一次性 AOT：ACG 后**不能再生成新代码**（不能再 JIT、不能改已有 RX）。等于「上锁前把全部代码铺好」，牺牲运行时可扩展性 |
| **T2 AllowThreadOptOut + 线程退出**：ACG 带 `AllowThreadOptOut=1`，`SetThreadInformation(ThreadDynamicCodePolicy, ALLOW)` 让当前线程退出，之后该线程 RW→RX 翻转恢复 | **成立**（退出前翻转被 1655 挡；退出后翻转+跳返回 42） | **前提是启用 ACG 时就带了 `AllowThreadOptOut`**——外部强加的纯 ACG（不带此位）无法这样自救。且被退出的线程是安全策略上的一个洞，等于局部放弃 ACG |
| **跨进程 JIT**（综述所述 Edge/Chakra 方案）：另起非 ACG 进程编译，把可执行 section 句柄共享回来 | **未实现**（超时间盒） | IPC + 第二个进程 + 句柄传递/section 复制；复杂度最高，但唯一能在「进程被外部强加纯 ACG」下继续 JIT 的路 |

### J6 — ③「符号解析」半边在 Windows 是否受影响 → **不受影响**

ACG（`ProcessDynamicCodePolicy`）只管**可执行内存的生成/翻转/映射**，不碰
`LoadLibraryA`/`GetProcAddress`。probe 里 ACG 开启后进程继续正常调用 kernel32 导出函数、正常 `println!`。
→ 原语 ③ 在 Windows 的活半边（符号解析）与原语 ④（按签名描述调已存在的原生地址）**不依赖可执行内存**，
不受 ACG 影响。**综述 §7.3 的因式分解「出站调用不需要可执行内存」在本机侧面成立。**

---

## 8. 判决 trace 与可信度分层

### 判决（按规格 §4 的树走）

1. **主判据 J3**：ACG **opt-in、默认 0x0** → 不落「默认动摇」分支，落「部署前提」分支。
2. **J1** 在 default 下**全过** → 不触发 kill criterion（Q0 内核路本机复现）。
3. **J2/J4 缺口清单**：ACG 下 ①② 三条路全断（硬失败 1655），但缺口对**我方自有进程**是
   **策略可配置**（可不开）；对**外部以纯 ACG 托管我方代码**是**该进程内硬性**。
4. **J5 替代路径**：本机测到两条**有条件**合法窗口（前置 RX / 线程退出），各有明确代价；
   跨进程 JIT 存在但未实现。
5. **J6**：③ 符号解析半边 + ④ 不受 ACG 影响。

**净结论**：在 **Windows 默认策略**下，原语 ①②③ **全部可用**——这把综述的「地基动摇」
在 Windows/x86_64 上**降级为「部署前提」**：默认能跑，硬化（ACG）场景需要替代路径。
**但**：① ACG 一旦被外部强加且不带 `AllowThreadOptOut`，进程内无自救（跨进程 JIT 除外）；
② iOS 综述称连落盘退路都无——若属实是**硬性缺口**，本机无法证伪。

### ①②③ 各自的可用性一句话（Windows/x86_64 实测）

| 原语 | default | ACG | 判定 |
|------|---------|-----|------|
| **① 内存 RW↔RX** | 能 | **不能**（1655） | **有条件能**：默认能；ACG 下不能，除非前置 RX 或线程退出或跨进程 |
| **② 跳进那段内存** | 能 | **不能**（无新可执行内存可跳） | 同 ①（② 依赖 ① 产出可执行页） |
| **③ 裸 syscall** | Windows 上本就不用（Q0 内核 `return -1`） | — | Windows N/A；实测靶场是 Linux/OpenBSD（本机测不了） |
| **③ 符号解析** | 能 | **能**（不受 ACG 影响） | 一等，恒定可用 |

### 可信度分层（本实验交付物之一）

| 论断 | 可信度 | 依据 |
|------|--------|------|
| Windows 默认放行 ①②；ACG opt-in、默认 0x0 | **实测** | `probe_win.exe default`，本机 |
| ACG 断 M1/M2/M3 三条路，错误码 1655 | **实测** | `probe_win.exe acg`，本机 |
| `1655 = ERROR_DYNAMIC_CODE_BLOCKED` | **本地一手依据** | 本机 Windows SDK `winerror.h` |
| 前置 RX 存活、AllowThreadOptOut 线程退出两条窗口成立 | **实测** | `probe_win2.exe t1/t2`，本机 |
| ③ 符号解析 / ④ 不依赖可执行内存、不受 ACG 影响 | **实测（侧面）** | ACG 后 probe 仍正常调 kernel32 / println | 
| ACG 可由 WDAC/AppLocker/EXE 头/父进程创建属性外部强加 | **未验证的转述**（机制名取自公开文档，本机未构造外部强加场景） | Microsoft 文档，未实测 |
| 跨进程 JIT 是纯-ACG 下唯一 JIT 路 | **未验证的转述**（综述 §7.1，未实现） | 未测 |
| **Linux** `MemoryDenyWriteExecute` 专杀 RW→RX、`MFD_NOEXEC_SEAL` 封 memfd 双映射 | **未验证的转述** | 综述 §7.1，本机无 WSL/Linux，无法实测 |
| **macOS** 需 `MAP_JIT` + `com.apple.security.cs.allow-jit` entitlement | **未验证的转述** | 综述 §7.1，本机非 macOS |
| **iOS** 无合法路径（连落盘退路都无，须 Apple 代签） | **未验证的转述** | 综述 §7.1，本机非 iOS；**若属实是唯一硬性无路平台** |
| **OpenBSD** 从生成代码发 syscall 一律杀进程（`msyscall`/pinsyscalls，直接打原语 ③ 裸 syscall 半边）；W^X 需 `wxallowed` | **未验证的转述** | 综述 §7.1，本机非 OpenBSD |

---

## 对四原语模型的影响

**原语定义不需要改，但可用性前提必须标注，且原语 ② 的实现必须多态。** 具体：

1. **① 需要带「可执行内存的获取方式」这一维度**（不是改语义，是标注实现前提）：
   RW→RX 翻转、直接 RWX、section 映射——本机实测三者在 default 全通、ACG 全断。
   ① 的契约应写成「获得一段可执行内存」，**获取方式是平台/策略相关的实现细节**，
   ACG/MDWE 会关掉全部内联方式。
2. **② 有三种实现，不是一种**（综述 §7.1 的判断被实测支持）：直接 RX / 跨进程共享 section /
   **解释**。「上锁前铺好」（T1）与「线程退出」（T2）是 default→ACG 之间的过渡窗口，不是通用退路。
   在**外部强加纯 ACG** 与 **iOS（若综述属实）** 下，唯一通用合法路径是**解释执行**。
   → **解释层必须是一等层，不是给 JIT 形状事后补的补丁。**
3. **③ 要拆成两半独立标注**：`裸 syscall`（Windows 不用；OpenBSD 从生成代码发出即死——
   转述）与 `符号解析`（Windows 一等、不受 ACG 影响——实测）。综述把两半合成一条原语，
   实测显示它们的地基完全不同。
4. **原语 ④ 与「可执行内存」解耦**（实测侧面 + 综述 §7.3）：出站调用不需要可执行内存。
   → 一个「什么都能调」（③符号解析 + ④）的核在 ACG 甚至 iOS 上仍可部署；
   只有「能生成新代码」（①②）的核受地基收紧影响。这是模型里最该显式写出的分层。

**一句话**：Windows/x86_64 实测把「地基动摇」**降级为「部署前提」**——默认全通，
ACG 是可选硬化且我们对自有进程可以不开；但 ①② 在硬化进程/iOS 上的缺口是真实的，
**因此解释执行必须从第一天就是一等退路**，而非事后补丁。
