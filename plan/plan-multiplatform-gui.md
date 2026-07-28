# Multi-platform GUI execution plan

状态：执行中  
工作主题：**Linux / macOS 人机窗口与共享 PTY 内核**  
本文是执行计划，不是产品事实；接受的能力必须同步进对应 `prd/PRD_*.md`。

## 产品结果

同一套 Fleet / terminal 契约，在 Windows 之外提供可启动的 `agenterm` GUI：
一个原生窗口、一页终端网格、一个 POSIX PTY 标签，并复用现有 CLI IPC。

## 依赖图

```text
PRD + Cargo deps (primary)
        │
        ├─► A pty backend trait          owns: src/pty/**
        │         │
        │         └─► B terminal_runtime unix  owns: src/terminal_runtime.rs
        │                   │
        ├─► C unix IPC server            owns: src/ipc_transport.rs (ungate)
        │         │
        └─► D unix_app (winit+softbuffer) owns: src/unix_app/**
                  │  needs A+B+C typed boundaries
                  └─► E packaging/CI      owns: artifacts.json, ci/release scripts
```

并行规则：A∥C∥E 可同时开工；B 在 A 的 API 稳定后接入；D 消费 B+C；
`Cargo.toml` / `src/lib.rs` / `PRD.md` 由 primary 串行集成。

## 进度

- [x] A PTY backend
- [x] B terminal_runtime unix
- [x] C unix IPC server + EventLoopProxy wake
- [x] D unix_app MVP (window + PTY grid + keys)
- [x] E packaging/CI includes `agenterm`
- [x] F shared `control_dispatch` + Unix wire (`protocol-info`,
  `list-sessions/windows/panes`, `send-keys`, `capture-pane`, `inspect`,
  `kill-server`); evidence: Linux black-box CLI against live GUI
- [x] G Win32 `execute_command` 收敛到同一 `ControlHost`；共享叶扩展
  `new-window` / `select-window` / `kill-window` / `active-window` /
  `display-message` / `rename-session`；Unix 实现 tab lifecycle
- [x] H Unix tab tree UI / composer / settings / ui-action / event journal
  - [x] event journal + `read-events` on Unix
  - [x] shared workspace commands (`list-tab-tree`, `scroll-pane`, rename/note/parent, `workspace-info`, `dump-cells`, `ui-snapshot` simplified)
  - [x] Unix tab sidebar (select by click)
  - [x] composer strip + `show/set/send-composer`, `focus`, `get-settings` (shared dispatch)
  - [x] `set-setting` + `ui-action` core subset (`new-tab`, `new-child`, `select-tab`, `close-tab`, `composer-send`)
  - [x] Unix `ui-snapshot` layout geometry (client + sidebar/terminal/composer bounds)
  - [x] settings modal UI + `open-settings` / `settings-apply` / `cancel`
  - [x] full shared `ui-action` tabs/layout/tree/editor subset
  - [x] Unix `ui-snapshot` scrollbar/modal/system_menu/tab metadata
- [x] I Unix mouse-wheel + scrollbar track/thumb (live max offset, event journal)
- [x] J Unix terminal cell selection + clipboard copy (`copy-selection`, Ctrl+C)
- [x] K Unix paste / word+row selection / selection autoscroll / sidebar toolbar

### A — PTY 后端抽象
- 用户问题：Windows ConPTY 与 Unix PTY 不能各写一套 runtime。
- 不变量：`ChildCommand` / `PtyMaster` / `PtyChild` / `TerminalSize` 对外签名稳定。
- 证据：Windows 仍走 `rmux-pty`；Linux `openpty`+fork/exec 能 spawn `/bin/sh` 读写。
- 安全失败：spawn 失败返回 typed error，不挂 GUI。
- 非目标：不替换 Windows ConPTY；不做多 pane。

### B — `terminal_runtime` 跨平台
- 用户问题：标签生命周期、vt100、scrollback、composer submit 必须共用。
- 不变量：`TerminalTab` 公共方法不变；HWND 退化为 `isize` wake token。
- 证据：Linux unit / smoke：spawn、output、exit、resize。
- 非目标：不移植 Windows 鼠标 console drag。

### C — Unix IPC server
- 用户问题：`agenterm-cli` 必须能连上 Unix GUI。
- 不变量：现有 JSON newline IPC 协议不变；`wake_window: isize` 可空。
- 证据：`agenterm` 起服后 `agenterm-cli protocol-info` / `list-windows` 通。
- 非目标：不改 Windows PostMessage 路径语义。

### D — Unix GUI MVP（winit + softbuffer）
- 用户问题：Linux/macOS 要有可看见的终端窗口。
- 不变量：无 GPU 要求；复用 `theme` / `ui_geometry` / `terminal_selection` / vt100 网格。
- 证据：窗口出现、shell 输出可见、键入到达 PTY、关闭干净。
- 安全失败：DISPLAY/Wayland 缺失时 stderr 说明并非零退出。
- 非目标：首切片不做完整 tab tree / settings 模态 / 专业选择手势；
  不做 Win32 像素级克隆。

### E — 打包矩阵纳入 GUI
- 用户问题：Release 包必须含 `agenterm`（linux/macos）。
- 证据：`artifacts.json` platforms 含 GUI；CI 编 `agenterm`；包内有二进制。
- 非目标：不把完整 `check.ps1` 搬到 Unix。

## 工具选型（已收敛）

| 层 | 选择 | 理由 |
|----|------|------|
| 窗口/事件 | `winit` | 跨 Linux/macOS，无 GPU 绑定 |
| 像素缓冲 | `softbuffer` | 对齐 Win32/GDI「软件栅格」 |
| Unix PTY | `libc` openpty/fork（必要时 `nix`） | 已有 libc；避免第二套 Windows PTY |
| Windows PTY | 继续 `rmux-pty` | 不扰动已交付路径 |

## 版本选择

不绑架 v0.1.8–v0.1.10 路线；本轨独立推进，能力状态记入
`PRD_02_01` / `PRD_02_02` / `PRD_02_06` / roadmap 的 multi-platform 叶。

## 集成顺序

1. A API + Windows wrapper 绿灯  
2. B 去 Windows-only cfg，Unix spawn 绿灯  
3. C IPC ungate + wake channel  
4. D 最小窗口连 B+C  
5. E 打包/CI；primary 合入 `main`（CPMP）
