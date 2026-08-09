# dynacore v3：guest 真机码 + 软件 ISA 模拟，不是自造 IR

| 字段 | 值 |
|------|-----|
| **日期** | 2026-08-09 |
| **状态** | 设计，Phase 1 待派出实现 |
| **前置** | [`design-dynacore-native-core.md`](design-dynacore-native-core.md)（已归档，但 seam.rs 的 Win32 绑定要复用）；
  [`design-dynacore-logic-pack.md`](design-dynacore-logic-pack.md)（agenterm-dynacore，继续独立存在，不受本设计影响）；
  `D:\dev\moltbaby\systems\ape\vm\ape-vm.c`（参考实现，**不直接编译进 agenterm**，见 §3 provenance） |
| **触发** | 用户指出 dynacore/nativecore 两条线都从没真正做到"面对 ISA/OS"这个最初的北极星——
  七个固定 intent（nativecore）或 FleetCall-only（dynacore）都不碰"在不同 ISA 上执行"这件事本身。
  用户要求参考 `moltbaby/systems/` 里已经真机验证过的探索，想更好的方向 |

---

## 0. 核心转向：guest 是真机码，不是我们发明的 IR

Q0–Q23 那条研究路线的默认前提是"要么原生生成机器码执行（JIT/AOT），要么解释一份自造的中立 IR"——
从没认真考虑第三条路：**软件模拟另一种 ISA 的指令译码，把 guest 的系统调用翻译成宿主系统调用**
（QEMU-user / Rosetta 那一类）。这条路 `moltbaby/systems/ape/vm/ape-vm.c` 已经做出来、而且按其
自己的 README/测试脚本记录真机跑通过：x86_64 + aarch64 双指令译码，静态 ELF/Mach-O guest，
真实 syscall 翻译（`~12` 个 Linux syscall + macOS 子集，未知 syscall 诚实报 `-ENOSYS`）。

**这条路把 dynacore 两轮测出来的两条最尖锐的边界直接消解，不是绕过**：

| 边界（`design-dynacore-logic-pack.md` §6，Q1/Q4） | 换成"guest 真机码"模型后 |
|---|---|
| `params_json` 必须打包时钉死字面量，不能读运行时值 | guest 是真实编译的程序，自己的指令流想怎么算参数就怎么算 |
| `FleetCall` 调用结果只有成功/失败一个 bit | guest 程序能读写自己的内存、对返回数据做任意判断 |

同时不丢 nativecore 当初的两条差异化点：**guest 二进制在别处编译好，agenterm 这边不需要编译器
在场**；**解释器本身是提前编译好的普通代码，从不生成机器码、不申请可执行内存**——安全边界从
"IR 结构验证"挪到"syscall 翻译层的白名单"，跟 nativecore 的 F1 纪律是同一个思路，只是搬了一层。

## 1. Provenance：参考，不直接编译进来

`ape-vm.c` 依赖 POSIX `mmap`/Linux syscall 号/`MAP_FIXED` 语义，本身是给 Linux/macOS 宿主写的，
**不能直接拿来在 Windows 上编译跑**——内存映射要换成 `VirtualAlloc` 系列重写，syscall 翻译层要
换成 Win32 API。这意味着 Phase 1 是**参考它的指令译码逻辑和整体结构**（读、理解、按同样的方法在
Rust 里重新写一份），不是 vendoring 那个 C 文件——避免引入一个跨项目的 C 依赖，也避免不必要的
license/维护边界纠缠。译码表本身（opcode → 语义）是公开的 CPU 手册内容，不是需要照抄源码才能
拿到的东西。

## 2. 阶段划分

### Phase 1（这轮要做）：同 ISA，先证明"guest 真机码 + Win32 syscall 翻译"这个模型本身成立

- **不碰跨 ISA**——guest 和宿主都是 x86_64，先把"解释真机码、翻译系统调用"这条机制在 Windows 上
  跑通，机制成立是 Phase 2 的前提，不是可以跳过的步骤
- 一个新的、独立的 Rust 模块（暂定 `crates/agenterm-nativecore` 之外的新目录，具体命名留给实现者，
  不预先锁死；provenance 上是"参考 ape-vm 的方法，clean-room 实现"，不是移植它的代码）
- 指令译码范围：对齐 `ape-vm.c` README 记录的 x86_64 已覆盖 opcode 子集（REX 前缀、基本数据移动/
  ALU/栈/控制流，ModRM+SIB+disp 寻址）——够跑 `hello`/`loop`/`fib` 这类小程序，不追求覆盖全 x86_64
- syscall 翻译层：复用 `agenterm-nativecore/src/seam.rs` 已经验证过的 Win32 绑定（`write`→
  `WriteStdout`、`openat`→`FileOpen`、`mmap`→`Alloc`……），**不重新发明**，只是换一个上游触发方式
  （从"解释自造 IR 的 intent 指令"变成"解释 guest 机器码译出的等价语义"）
- 验收：一个真实用汇编器/编译器产出的静态 x86_64 ELF（可以从 `ape-vm/vm/hello.s`/`loop.s` 这两个
  最小样例参考语义，clean-room 重新汇编，不直接复制 `.s` 文件），在新解释器里真机跑出正确输出和
  退出码

### Phase 2（下一轮，不在本轮范围）：接入 aarch64 guest 译码

对齐 `ape-vm.c` 已经验证过的 A64 解码表（MOVZ/MOVN/MOVK、ADR/ADRP、ADD/SUB、分支、LDR/STR 等）——
这才是"一个运行中的 agenterm 进程，不重新编译自己，就能跑另一种 ISA 的 guest 代码"这件事真正
落地的地方。Phase 1 没做完、机制没在 Windows 上证实前，不提前做这个。

### Phase 3+（未定）：更完整的 syscall 覆盖、真实 PE 容器支持

留白，不在本轮预判范围。

## 3. 明确不做（本轮，Phase 1）

- 不做跨 ISA（Phase 2 的事）
- 不做完整 x86_64 指令集覆盖，只做够跑最小验证程序的子集
- 不碰 `agenterm-dynacore`（继续独立存在，本设计不影响它）
- 不从 `crates/agenterm-nativecore` 复活/取消归档任何东西——只**读**它的 `seam.rs` 作为 syscall
  翻译层的参考实现，新模块是独立的
- 不直接编译/vendoring `ape-vm.c` 本身（见 §1）

---

*产品设计文档。Phase 1 待派出实现，验收标准见 §2。*
