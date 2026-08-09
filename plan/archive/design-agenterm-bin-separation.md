# 为什么 agenterm 和 agenterm-cli 不能合并（已归档：结论被推翻）

> 2026-08-09。基于 `src/bin/agenterm.rs`、`src/bin/agenterm-cli.rs`、`Cargo.toml`、`src/client/mod.rs` 的实际代码分析。
>
> **归档说明（同日）**：本文结论已被同日落地并真机验证的合并实现推翻。
> §1 视为"workaround"的 `AttachConsole` 路线（加上句柄快照、
> `DuplicateHandle`、worker 端 attach）就是最终交付的设计；`agenterm-cli.exe`
> 已删除，`agenterm cli <command>` 是唯一 CLI 入口。§2/§3/§4 的成本疑虑
> 实测不成立或被接受。现行设计见
> `plan/design-agenterm-cli-merge.md` 的「落地记录」。

---

## 一句话

**不是不能——是合并的代价远大于收益。** 当前分离是刻意设计，每条理由都在代码里有根据。

---

## 1. Windows 子系统冲突（硬件约束）

```rust
// src/bin/agenterm.rs 第 1 行
#![cfg_attr(windows, windows_subsystem = "windows")]
```

这是 Windows PE 头里的编译期属性。含义：

| 属性 | 效果 |
|------|------|
| `windows_subsystem = "windows"` | 启动时不分配控制台窗口。`println!` 输出消失。 |
| 默认（console subsystem） | 启动时分配控制台窗口。GUI 启动会闪一个黑框。 |

`agenterm` 是 GUI 程序——必须设为 `windows` 子系统，否则双击 `agenterm.exe` 会弹出控制台黑框。

`agenterm-cli` 是控制台程序——必须保持 console 子系统，否则 `agenterm-cli list-windows` 的输出无处可见。

**合并意味着二选一：要么 GUI 启动带黑框，要么 CLI 无输出。** Windows 不支持「运行时切换子系统」。

> 唯一 workaround：编译两个 PE，或者 `AttachConsole` + 重定向 stdout。但这就是本质上两个入口——只是共享一个文件名而已。

---

## 2. 依赖权重（编译 + 启动成本）

```
agenterm 的依赖链：
  winit → wgpu/skia → 字体渲染 → ConPTY → frontend 全套 → 窗口/IME/DPI/截图
  → 编译产物 ~3 MiB（需控制在 4 MiB 预算内）

agenterm-cli 的依赖链：
  serde_json → IPC transport → EventJournal → script protocol
  → 编译产物 <0.5 MiB（估算）
```

合并后 `agenterm-cli list-windows` 需要加载整个 GUI 栈。一个毫秒级的 CLI 命令变成了秒级的 GUI 初始化。

---

## 3. 启动路径完全不同

```
agenterm 的 main():
  parse --version/--help（offline，不初始化 GUI）
  → 检测 "server" 子命令 → run_server_entry_with_args()
  → 否则 → run_gui_entry()
    → 创建窗口 → 连接 ConPTY → 启动/连接 server → 渲染循环

agenterm-cli 的 main():
  run_cli_entry()
  → 解析 --endpoint/--address/--instance
  → 连接 IPC → 发送请求 → 打印结果 → 退出
```

这是两条**互斥**的代码路径——不共享任何启动逻辑。合并后需要 `if gui_mode { ... } else { cli_mode { ... } }` 在最外层分支——本质上还是两个程序，只是塞进一个文件。

---

## 4. 崩溃隔离（运维约束）

```
合并后：
  GUI 崩溃（GPU 驱动 bug / DPI 异常 / 字体 panic）
    → agenterm.exe 进程终止
    → agenterm-cli 也不可用
    → 无法用 CLI 关闭 server / 保存 workspace / 诊断问题

分离时：
  GUI 崩溃 → server + CLI 仍存活
  agenterm-cli close           # 正常关闭
  agenterm-cli list-windows    # 查看状态
  agenterm-cli save-workspace  # 保存工作区
```

agenterm 的 PRD 硬约束：**CC 崩溃不影响 PTY。** 同理，GUI 崩溃不应影响 CLI 控制面。

---

## 5. `agenterm server` 已经是最小化复用的答案

`agenterm` PE 已经承载了两个角色：GUI 和 headless server。这是合理的——它们共享同一个渲染/PTY 代码库，只是入口不同。

但如果把 `agenterm-cli` 也塞进去，就变成三个角色共享一个 PE。`agenterm-cli` 不依赖 GUI 的任何东西——它只需要 IPC + JSON。把它强塞进 `agenterm` PE 既不省体积，也不省复杂度。

当前的安排是最优的：

| PE | 角色 | 共享什么 |
|----|------|---------|
| `agenterm` | GUI + `server` 子命令 | 渲染/PTY 代码库 |
| `agenterm-cli` | control plane + `mux` + `mcp` | IPC + script 协议 |
| `agenterm-cc` | Control Center 投影 | IPC + snapshot 渲染 |
| `agenterm-rh` | Script 运行时 | 独立 crate，不与 GUI 库链接 |

---

## 6. 什么可以合并（且已经合并了）

`agenterm-mux` 和 `agenterm-mcp` 以前是独立 PE。已经在 v0.1.12 合并到 `agenterm-cli` 的子命令中。这是正确的合并——它们共享同样的 IPC 路径、同样的轻量依赖。

`agenterm-cli` 的 `hosted_subcommand()` 函数（`src/client/mod.rs:149`）就是一个清晰的「子命令路由」模式——`mux` 和 `mcp` 是独立子命令，但共享同一个轻量二进制。

---

## 7. 结论

| 维度 | agenterm + agenterm-cli 合并 | 当前分离 |
|------|----------------------------|---------|
| Windows 子系统 | 冲突（GUI 黑框 or CLI 无输出） | 各自正确 |
| 启动速度 | CLI 需要加载 GUI 栈 | CLI 快速 |
| 崩溃隔离 | 互拖 | 独立存活 |
| 体积 | 无法分离分发 | 可按需安装 |
| 代码复杂度 | 一个 main 里分支三条路径 | 三个干净的入口 |

**答案：刻意分离，每一项都有代码级理由，不是历史遗留。**
