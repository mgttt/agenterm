# agenterm + agenterm-cli 合并路径

> ⚠️ **已归档（2026-08-09 shipped）**。权威口径：`prd/PRD_02_02_executable_family.md`
> 的 `agenterm cli` 条目 + `plan/plan-v0.1.16.md` §CLI。本文保留设计过程与
> 「落地记录」作历史证据。
>
> 2026-08-09。确认：分离的唯一阻挠是 Windows 子系统。平台层已有解法。
> 合并后 CLI 路径 = `agenterm cli <args>`，与 `agenterm server` 同模式。
>
> **状态：已落地并真机验证（见文末「落地记录」）。** §1–§4 是当时的方案
> 草案，与最终实现有出入：CONOUT$ 每次写入的方案不够（需要句柄快照 +
> `DuplicateHandle` + worker 端 attach）；§2 的 `is_cli_subcommand` 裸命令
> 检测未实现（显式 `cli` 前缀是唯一入口）；§4 的「保留薄 wrapper」被否决
> ——`agenterm-cli.exe` 已彻底删除，无任何 wrapper。

---

## 0. 现状与目标的差异

```
现状：
  agenterm.exe --help          → 输出到父控制台（已实现）
  agenterm.exe server ...      → headless server（已实现）
  agenterm-cli.exe list-windows → CLI 控制面（独立 PE）
  agenterm-cli.exe mux ...     → mux 兼容层（已合并到 CLI）
  agenterm-cli.exe mcp ...     → MCP 服务（已合并到 CLI）

目标：
  agenterm.exe --help          → 不变
  agenterm.exe server ...      → 不变
  agenterm.exe cli list-windows → CLI 控制面（合入 agenterm PE）
  agenterm.exe cli mux ...     → mux（合入 agenterm PE）
  agenterm.exe cli mcp ...     → MCP（合入 agenterm PE）
  agenterm-cli.exe             → 薄转发 wrapper，最终可废弃
```

---

## 1. Windows 子系统问题已解决

`crates/agenterm-platform/src/adapters/windows/process.rs` 第 47 行：

```rust
if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
    return false;
}
let written = OpenOptions::new()
    .write(true)
    .open("CONOUT$")
    .is_ok_and(|mut console| {
        console.write_all(payload.as_bytes()).is_ok() && console.flush().is_ok()
    });
unsafe { FreeConsole() };
```

这段代码已经在 `agenterm.exe --version` 和 `agenterm.exe --help` 中跑了至少三个版本（v0.1.12+），稳定可靠。

**原理：**
- `agenterm.exe` 编译为 `windows_subsystem = "windows"` → 启动时无控制台
- 从 cmd/pwsh 启动时，`AttachConsole(ATTACH_PARENT_PROCESS)` 附着到父进程的控制台
- 通过 `CONOUT$`/`CONIN$` 读写控制台
- 完成后 `FreeConsole()` 释放
- 双击启动时，没有父控制台 → `AttachConsole` 失败 → 走 GUI 路径（不变）

**唯一需要扩展的：stdin。** 当前 `write_parent_console` 只处理输出。CLI 路径需要读取 stdin（例如 `mcp serve --stdio`）。`CONIN$` 同样可用——`OpenOptions::new().read(true).open("CONIN$")`。

---

## 2. 合并后的入口逻辑

```rust
// src/bin/agenterm.rs（合并后）

#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    // 1. 已有的离线 CLI（--version / --help）
    if let Some(code) = offline_cli_exit(&args) {
        std::process::exit(code);
    }

    // 2. 已有的 server 子命令
    if args.first().map(String::as_str) == Some("server") {
        args.remove(0);
        std::process::exit(agenterm::run_server_entry_with_args(args));
    }

    // 3. 新增：cli 子命令（替代 agenterm-cli.exe）
    if args.first().map(String::as_str) == Some("cli") {
        args.remove(0);
        agenterm_platform::process::attach_parent_console();  // 扩展：stdin+stdout+stderr
        std::process::exit(agenterm::run_cli_entry_with_args(args));
    }

    // 4. 旧 agenterm-cli 调用模式的兼容（检测已知的 CLI 子命令模式）
    if is_cli_subcommand(&args) {
        agenterm_platform::process::attach_parent_console();
        std::process::exit(agenterm::run_cli_entry_with_args(args));
    }

    // 5. GUI 路径（无匹配 → 开窗口）
    std::process::exit(agenterm::run_gui_entry());
}
```

**`is_cli_subcommand()` 检测逻辑：** 如果第一个参数匹配已知的 CLI 子命令（`list-windows`, `list-commands`, `mux`, `mcp`, `script`, 等 → 来自 `COMMAND_CATALOG`），自动走 CLI 路径。这样 `agenterm list-windows` 等价于 `agenterm cli list-windows`。

---

## 3. 需要在平台层新增的能力

```rust
// crates/agenterm-platform/src/process.rs（新增）
pub fn attach_parent_console() -> bool;
// 调用 AttachConsole(ATTACH_PARENT_PROCESS)
// 成功 → stdin/stdout/stderr 连接到父控制台的 CONIN$/CONOUT$
// 失败 → 返回 false（调用方决定降级策略：可能是 GUI 模式，或退出报错）
```

这个函数是 `write_parent_console` 的泛化——不只是写入一条消息，而是建立完整的 stdio 连接。Windows 实现用 `AttachConsole` + `CONIN$`，Linux/macOS 实现是空操作（本来就是控制台程序）。

---

## 4. 过渡期：保留 agenterm-cli.exe 为薄 wrapper

```
agenterm-cli.exe（过渡）:
  → spawn("agenterm.exe", ["cli", ...args])
  → 转发 exit code
```

等所有下游（CI、脚本、文档）迁移到 `agenterm cli` 后，可以废弃 `agenterm-cli.exe`。

但要注意：`agenterm-cli` 是一个轻量 PE（~0.5 MiB），而 `agenterm` 是 ~3 MiB。合并后 `agenterm cli list-windows` 的启动时间会比现在的 `agenterm-cli list-windows` 慢——因为加载整个 GUI 栈。对于交互式使用（毫秒级差异），这通常不可感知。对于脚本循环调用，可能需要测量。

---

## 5. 合并的利弊

| 维度 | 合并后 | 当前 |
|------|--------|------|
| 产品语义 | `agenterm cli <cmd>` 统一入口 | 两个入口，用户需要记住哪个用哪个 |
| Windows 启动 | 1 个 `AttachConsole` 调用（已证明可行） | 两个 PE，各自正确 |
| CLI 启动速度 | 加载 GUI 栈（可能慢 ~100-300ms） | 极快（仅 IPC + JSON） |
| 崩溃隔离 | 不变——CLI 子命令是短生命周期的，GUI 崩不崩取决于是否要开窗口 | 不变 |
| 二进制体积 | 无法按 CLI-only 轻量分发（但 `agenterm server` 已有此特征） | CLI 可独立分发 |
| 维护成本 | 1 个 bin 入口，`agenterm-cli` 薄 wrapper 逐步废弃 | 2 个 bin 入口 |

---

## 6. 结论

**技术上可行。** `AttachConsole` 模式已经过验证。`agenterm server` 子命令已经证明了「一个 PE，多个角色」的可行性。

**产品语义上正确。** `agenterm cli list-windows` 比 `agenterm-cli list-windows` 更一致——就像 `git` 不是 `git-cli`。

**唯一需要认真评估的：CLI 启动速度。** 合并后每次 CLI 调用都要加载完整 GUI 栈。对于一次性命令（`list-windows`、`inspect`）这个开销可能不可感知。对于脚本循环（100 次调用），累积延迟可能显著。（当时建议保留薄 wrapper；最终决定不保留——见落地记录。）

---

## 7. 落地记录（2026-08-09，真机验证）

最终实现与 §1–§4 草案的差异，以及为什么：

**转发机制（`src/bin/agenterm.rs` + `crates/agenterm-platform/.../console.rs`）**

1. `agenterm.exe cli <args>` 父进程：
   - **attach 前**快照三个 std 句柄——调用方的管道/文件重定向在此时可见；
   - `AttachConsole(ATTACH_PARENT_PROCESS)`；attach 可能把槽位顶成控制台
     句柄，快照里有效的重定向被**恢复**回槽位；两边都无效才回退打开
     `CONOUT$`/`CONIN$`；
   - `GetStdHandle` + `DuplicateHandle` 复制真实 stdin/stdout/stderr，以
     显式 `OwnedHandle`→`Stdio` 启动同 PE 隐藏子命令
     `__agenterm-internal-cli <args>`，同步 `status()` 等待并透传退出码。
     不用 `Stdio::inherit`（信任 spawn 时槽位状态，不可靠）。
2. worker 子进程：std 槽位从进程启动即持有显式句柄，但**控制台句柄只有
   在持有进程有 ConDrv 控制台连接时才可写**——这是真机测出的关键缺陷
   （无连接时写入静默失败，输出黑洞）。所以 worker 也 attach 转发者的
   控制台，用 `attach_parent_with_default_interrupts()`：不装 Ctrl+C
   忽略钩子，保持与控制台 CLI 一致的中断语义（Ctrl+C 杀 worker，父进程
   透传退出码）。纯管道调用方（如 MCP 客户端 spawn）无控制台，attach
   失败是无害的。

**验证矩阵（Windows Server 2022 真机）**：PS/cmd 直接调用、文件重定向
（调用方句柄保留）、管道、stderr/stdout 分流、退出码 0/1/2、
`mcp serve --stdio` 双向 JSON-RPC、agenterm 自身 ConPTY 内真实控制台输出
（capture-pane 自证）、Ctrl+C 中断。集成测试
`tests/agenterm_cli_forwarding.rs` 5/5。

**已知边界**：交互式 shell 按 PE subsystem 决定是否等待——GUI PE 的裸
交互调用，输出可能在提示符返回后打印（PowerShell 管道/捕获时等待，
`cmd /c`/批处理总是等待）。这正是当年 `agenterm.com` 存在的理由；产品
决定接受此边界，换取单一 PE、零 wrapper。

**§5 疑虑的实测回答**：崩溃隔离不变（CLI 是短生命周期 worker）；启动
速度对一次性命令不可感知；`agenterm-cli.exe` 无保留必要。
