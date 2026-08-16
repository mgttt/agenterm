# agenterm-dyn 跨平台加固评审（CC review）

Date: 2026-08-15. Reviewer: Claude Code 会话。
Scope: `crates/agenterm-dyn` 全部源码 + 测试 + `hosts.rs` 六格表 + PRD 02.34 + CI 配置。
Status: 评审意见，未实施。第 2/4 步与 PRD 02.34 现行方向冲突，需政委拍板。

## 1. 现状体检

| 面 | 状态 |
|----|------|
| Linux × {x86_64, aarch64} | `HostCell.system_probes` 35 行全部 `LiveDlcall`；`tests/smoke.rs` 有真实 libc 交叉校验。成熟。 |
| macOS × {x86_64, aarch64} | 35 行 `system_probes` **全为 `Placeholder`**；只有 pid / `time` secondary / `ioctl` on `/dev/tty` / `getenv` 四个 smoke。 |
| Windows × {x86_64, aarch64} | 同上全为 `Placeholder`；只有 `GetCurrentProcessId` / `GetCurrentThreadId` / CRT `getenv` 三个 smoke。 |
| CI 保护 | **只有 ubuntu job 跑 dyn。** |

### 最要命的一条：非 Linux 侧从未被 CI 编译过

- `.github/workflows/ci-agenterm.yml:47` 的 `cargo test --locked -p agenterm-dyn` 位于 ubuntu 单 job 内。
- `target-cells` 六格矩阵只做 `cargo check --locked -p agenterm --all-targets`，而 `agenterm-dyn` **不是** `agenterm` 的依赖（PRD：尚未接入根二进制）。
- 结论：`tests/smoke.rs` 的 `mod macos`（`smoke.rs:886`）、`mod windows`（`smoke.rs:998`）以及 `windows-sys` dev-dependency，**在 CI 上一次都没有被编译过**。README 宣称的 "ISA×2 / OS×3 from day one" 目前在 CI 层面无任何保护，随时可能静默腐烂。

## 2. 加固路线（按优先级）

### 步骤 1 — 先补编译 / 执行门（最高性价比，只改一个 CI 文件）

在 `ci-agenterm.yml` 的 `target-cells` 矩阵中增加 dyn 步骤：

- `win-x86_64`（windows-latest）、`osx-aarch64`（macos-15）、`osx-x86_64`（macos-15-intel）三个 native cell：
  `cargo test --locked -p agenterm-dyn --target ${{ matrix.target }}`
- `win-aarch64`（xwin 交叉）：`cargo xwin check --locked -p agenterm-dyn --all-targets --target aarch64-pc-windows-msvc`
- `lnx-aarch64`（linux-cross）：`cargo check --locked -p agenterm-dyn --all-targets --target aarch64-unknown-linux-gnu`

交叉格跑不了测试，至少保证编译。**没有这一步，后续所有 mac/win 探针都是盲写。**

### 步骤 2 — 改 `HostCell` 形状，让非 Linux 行不必假装

`system_probes: [SystemProbe; 35]`（`hosts.rs:52`）是硬编码 Linux 语义的定长数组：Windows 行被迫填 `nice_zero`、`umask`、`fcntl_stdin_getfd` 这类根本不存在的名字，纯噪声。

- 改为 `system_probes: &'static [SystemProbe]`。
- 把每个探针需要的常量搬进行数据（目前只有 `SizeProbe::IoctlTiocgwinsz.request` 一个字段带常量）。

这是补 mac/win 探针的前置条件。

### 步骤 3 — 常量必须 per-OS，禁止照抄 Linux 字面量

最易出错处。`hosts.rs` 现有数值全部是 Linux 值：

| 常量 | Linux | macOS |
|------|-------|-------|
| `TIOCGWINSZ` | `0x5413` | `0x40087468` |
| `_SC_PAGESIZE` | 30 | 29 |
| `_SC_NPROCESSORS_ONLN` | 84 | 58 |

`F_GETFD` / `F_GETFL` / `SEEK_CUR` / `RUSAGE_SELF` 亦各有差异。这些必须作为行数据进表，并在 `tests/hosts.rs` 里对着 `libc` crate 的常量交叉断言（而不是写死重复字面量）。

### 步骤 4 — 填 live 探针（对称化）

- **macOS**（`libSystem.B.dylib`）：`getppid` / `getuid` / `geteuid` / `getgid` / `getegid` / `getpgrp` / `getsid` / `getpgid`；`sysconf` 三件套；`getpagesize`；`getcwd` / `uname` / `times` / `getrusage`（caller-owned ptr 缓冲）；`isatty`；`open` / `close` / `access` / `fcntl` / `dup` / `lseek`；`gettimeofday`；`mach_absolute_time`。
- **Windows**（`kernel32.dll`）：`GetTickCount64`；`GetLastError` / `SetLastError`；`GetStdHandle(-11)` + `GetConsoleScreenBufferInfo`(ptr)；`GetSystemTimeAsFileTime`(ptr)；`QueryPerformanceCounter` / `QueryPerformanceFrequency`(ptr)；`GetCurrentDirectoryW`(ptr)；`GetSystemInfo`(ptr)；`GetModuleHandleW(NULL)`；`GetACP`；`GetProcessHeap`（ptr 返回）。

Windows 这一侧刚好把 `ptr` 出参路径与 `ptr` 返回值路径都练到，价值不低于 Linux 侧的第 36 个 libc 探针。

沿用 Linux 侧规矩：任何进程级副作用必须在测试结束前还原（`umask` 是范式）；`SetLastError` 同理。

### 步骤 5 — 库名鲁棒性（当前在 musl 上直接死）

- `libc.so.6` 在 Alpine / musl 上不存在（musl 为 `libc.musl-<arch>.so.1`）。
- 建议 `HostCell` 带候选列表（`libc.so.6` → `libc.so` → `libc.musl-aarch64.so.1`；macOS `libSystem.B.dylib` → `/usr/lib/libSystem.B.dylib`），**仅供表与测试使用**；`dlcall` 的脚本语义不变，库名在 eval 边界仍是不透明字符串。
- Windows 的 `ucrtbase.dll` → `msvcrt.dll` 回退目前是 `smoke.rs` 里手写的 `match`，应上升为表数据。

### 步骤 6 — ABI 门本身的跨平台真相（`native.rs`，影响所有平台）

- **变参**：`printf`、`open(path, flags, mode)` 这类在 **macOS arm64（Apple AAPCS64 变体）上走完全不同的栈传参规则**。当前 `dlcall` 会静默返回错误结果而**不报错**，且无法在门内检测。必须在 README 与 crate 文档写死"variadic 未支持且不可检测"，并规定探针目录禁止出现变参符号。
- **架构边界**：整套 `transmute` + 全参数按 `u64` 传递的做法只在 x86_64 / aarch64 成立。32 位 Windows 的 `stdcall` 名字修饰（`_Foo@4`）不支持，应显式声明为非目标。
- **`u64` 返回值 > `i64::MAX` 会报错**（`native.rs` 的 `SigType::U64` 分支）。Windows 句柄 / 地址类返回值必须声明为 `ptr`，否则 mac/win 探针会踩坑——需在文档点明。
- 参数窄类型扩展（`i8`/`i16`/`u16` → 64 位）当前在 `DynArg::from_value` 里做了正确的符号/零扩展，满足 Apple arm64 要求 caller 扩展的约定，此处无需改动。

## 3. 与 PRD 02.34 的冲突

PRD 02.34 的 "probes" 分支明确写着 *"Keep Win/macOS as placeholders. No C shim."*，顺序为 harden → probes(Linux only) → examples。

- 步骤 **2 / 4** 直接改这条方向，需政委点头；一旦开启，同时修改 PRD 02.34 "probes" 分支措辞。
- 步骤 **1 / 3 / 5 / 6** 属纯加固，不违反 PRD，可立即执行。

## 4. 建议的落地顺序

1. 步骤 1（CI 编译门）+ 步骤 6（文档 / 边界声明）—— 不违反 PRD，立刻消除盲区。
2. 等政委裁决后开步骤 2 → 3 → 4 → 5，并同步改 PRD。

## 5. 并发注意

评审期间 `crates/agenterm-dyn/README.md` 被另一会话追加了 `getrlimit` 条目——该 crate 目录当前有并发作业。步骤 1 只动 `.github/workflows/ci-agenterm.yml`，不与之撞文件域；步骤 2–5 需先协调独占文件域。
