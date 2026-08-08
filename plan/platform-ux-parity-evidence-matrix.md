# 平台 UX 对齐证据矩阵（并发回归模板）

本文件用于每轮回归后的“分支-场景-证据”归并。未通过项直接阻断该分支收敛，先补齐 `platform` 能力或产品行为再推进。

## 并发执行入口

- Windows: `agenterm-rh task run platform-ux-parity-smoke -- --list-evidence`
  - fallback: `agenterm-rhai task run platform-ux-parity-smoke -- --list-evidence` (compatibility shim)
  - 回归: `agenterm-rh task run platform-ux-parity-smoke -- --emit-matrix`
- Linux: `agenterm-rh task run platform-ux-parity-smoke-linux -- --list-evidence`
  - fallback: `agenterm-rhai task run platform-ux-parity-smoke-linux -- --list-evidence`
  - 回归: `agenterm-rh task run platform-ux-parity-smoke-linux -- --emit-matrix`
- macOS: `agenterm-rh task run platform-ux-parity-smoke-macos -- --list-evidence`
  - fallback: `agenterm-rhai task run platform-ux-parity-smoke-macos -- --list-evidence`
  - 回归: `agenterm-rh task run platform-ux-parity-smoke-macos -- --emit-matrix`

## 分支-场景归并表（按循环填充）

| 分支 | 场景 | 对应 evidence | Windows | Linux | macOS | 归因标签 | 下一步动作 |
|---|---|---|---|---|---|---|---|
| startup | 首窗口/启动合同 | `ux-parity.startup` | Supported | Supported | Supported | - | - |
| startup | 启动标题/窗口恢复 | `ux-parity.startup-title` | Supported | Supported | Supported | - | - |
| ux-startup | 唤醒契约 | `ux-parity.wake-coalescing` | Supported | Supported | Supported | - | - |
| ux-startup | 焦点恢复 | `ux-parity.window-focus-contract` | Supported | Supported | Supported | - | - |
| frontend-lx | Workbench 与窗口行为（linux） | `ux-parity.linux.unix-frontend.workbench` | Unsupported | Supported | Unsupported | - | - |
| frontend-lx | 剪贴板语义（linux） | `ux-parity.linux.unix-frontend.clipboard` | Unsupported | Supported | Unsupported | - | - |
| frontend-mx | Workbench 与窗口行为（macos） | `ux-parity.macos.unix-frontend.workbench` | Unsupported | Unsupported | Supported | - | - |
| frontend-mx | 剪贴板语义（macos） | `ux-parity.macos.unix-frontend.clipboard` | Unsupported | Unsupported | Supported | - | - |
| remote-ui | replaceable 客户端 | `ux-parity.remote-ui.replaceable-client` | Supported | Unsupported | Unsupported | windows-only-contract | - |
| remote-ui | selection 语义 | `ux-parity.remote-ui.selection` | Supported | Unsupported | Unsupported | windows-only-contract | - |

### 当前轮次优先级归类（P0/P1/P2）

- P0（功能主线阻塞）：`startup` / `startup-title` / `window-focus-contract` 在目标平台失败且为非能力缺口时，阻断该 run 全量下游（当前 Windows 侧本轮为 Windows 通过、Linux/macOS 预检为环境缺口，不进入 P0）。
- P1（交互行为偏差）：`remote-ui`（Windows-only）与 `front-end`（跨 Unix host）出现行为缺口，影响体验一致性，按平台 owner 分拆修复。
- P2（能力边界）：`platform-gap` / `infra/platform-binary-missing` 归类为能力边界，需在对应 host pipeline 补齐后重试，而不阻塞其他平台分支。

> 说明：
> - full-modal 输入拦截由 FocusTransitionGate::full_modal_blocked() 单点生成（window-close/settings/new-terminal/tab-close），Win/Unix 的 terminal mouse、wheel、focus ring、system menu 与 CWD 入口共用。
> - modal 打开入口由 FocusTransitionGate::modal_entry_blocked() 单点判定；Windows/Unix 的 settings、new-terminal、CWD editor、live-tab close 共用同一互斥规则。
> - window-close 请求分支由 WindowCloseRequest/window_close_request() 单点判定；任一平台若正在 live-tab close 确认，先取消该确认并返回，不再叠加窗口关闭确认。
> - live-tab close 请求统一为：先取消 inline editors/CWD、同步 composer，再打开 close confirmation；两端行为一致。
> - cancel 动作优先级由 CancelTarget/cancel_target() 单点判定：window-close > live-tab close > settings > new-terminal > CWD > tab editor。
> - confirm 动作优先级由 ConfirmTarget/confirm_target() 单点判定：window-close > live-tab close；Enter 默认 keep-server-running 或确认关闭 live tab，Esc 统一走 cancel 优先级。
> - modal/focus 表面命名由 src/frontend/interaction.rs 的 ModalSurface + modal_surface_from_gate() 单点生成（window-close > settings > new-terminal > cwd-editor > tab-close；tab-editor 走独立 `tab_editor` 快照），普通 focus 表面也由 `FocusSurface::as_str()/from_ipc()` 单点生成（terminal/composer/tabs|sidebar），Windows remote 与 Unix embedded 共用。
> - `Supported`/`Failed`/`Unsupported` 只允许取 `Unsupported` 来表示“当前平台能力缺口”。`Failed` 代表脚本执行失败（回归阻断）。

## 证据与分支归因规则

1. 若 `startup` 或 `startup-title` 失败：阻断全部分支，优先修复 `frontend` 启动入口与进程生命周期。
2. 若 `window-focus-contract` 失败：阻断启动分支与所有交互分支，优先修复窗口焦点语义。
3. 若 `linux/unix-frontend` 失败：只阻断对应平台的 Unix 前端分支；Windows remote-ui 分支可继续。
4. 若 `remote-ui` 失败：只阻断 Windows 远端前端分支；Unix 平台继续执行其分支。

## 自动化执行建议（树状并发）

```text
O3 并发执行循环（每日）
├─ O3A Windows branch
│  └─ platform-ux-parity-smoke -- --emit-matrix
├─ O3B Linux branch
│  └─ platform-ux-parity-smoke-linux -- --emit-matrix
└─ O3C macOS branch
   └─ platform-ux-parity-smoke-macos -- --emit-matrix

O3 回合后汇聚
├─ 同步收集三端 evidence-matrix
│  ├─ target/smoke/test-runs/platform-ux-parity-*/platform-ux-parity-smoke-matrix.json
│  └─ 按 evidence_id 进行三列并行汇总（windows/linux/macos）
└─ 填写到 plan/platform-ux-parity-evidence-matrix.md 的统一状态列
```

汇总：
- `Failed` 先阻断并派发修复分支；`Unsupported` 仅记录能力缺口
- 把失败按表格写入本文件
- 触发对应 owner 的修复分支（O1A/O1B）
- 通过后切换下一轮对照场景

## 结果落地格式（JSON / CSV）

已提供两份固定模板，统一用于 CI 报告与人工复盘：

- `plan/platform-ux-parity-evidence-matrix.template.json`
- `plan/platform-ux-parity-evidence-matrix.template.csv`

建议字段语义：

- `run_id`：本次 CI 运行或本地回归流水 ID
- `branch`：`startup | ux-startup | frontend-lx | frontend-mx | remote-ui`
- `scenario`：具体场景标识（例如 `first-window-startup`, `linux-workbench`）
- `state`：`Supported | Failed | Unsupported`
- `root_cause`：`windows-only-contract` / `platform-gap` / `infra/...` / `bug/<ticket-id>` / 空
- `owner`：`platform` / `ui` / `infra`

填充方式：

- 仅保留本次执行结果，逐行覆盖对应 `evidence_id`。
- 任何 `Failed` 记录会阻断对应分支，若为 `Unsupported` 仅阻断该平台能力缺口。
- 与 [plan/plan-unix-gui-win-parity.md](plan-unix-gui-win-parity.md) 的 O3 结果页同步。

## 可直接复制的当前轮次实例（JSON）

```json
{
  "run_id": "ci-2026-08-02-0001",
  "timestamp_utc": "2026-08-02T08:00:00Z",
  "suite": "platform-ux-parity-smoke",
  "environment": {
    "branch": "main",
    "commit": "abcdef1234567890",
    "runner": "windows-latest-1"
  },
  "result": [
    {
      "branch": "startup",
      "scenario": "first-window-startup",
      "evidence_id": "ux-parity.startup",
      "state": "Supported",
      "root_cause": "",
      "owner": "platform"
    },
    {
      "branch": "startup",
      "scenario": "startup-title",
      "evidence_id": "ux-parity.startup-title",
      "state": "Supported",
      "root_cause": "",
      "owner": "platform"
    },
    {
      "branch": "ux-startup",
      "scenario": "gui-wake-contract",
      "evidence_id": "ux-parity.wake-coalescing",
      "state": "Supported",
      "root_cause": "",
      "owner": "platform"
    },
    {
      "branch": "ux-startup",
      "scenario": "window-focus-contract",
      "evidence_id": "ux-parity.window-focus-contract",
      "state": "Supported",
      "root_cause": "",
      "owner": "ui"
    },
    {
      "branch": "frontend-lx",
      "scenario": "linux-workbench",
      "evidence_id": "ux-parity.linux.unix-frontend.workbench",
      "state": "Unsupported",
      "platform": "linux",
      "root_cause": "platform-gap",
      "owner": "ui"
    },
    {
      "branch": "frontend-mx",
      "scenario": "macos-workbench",
      "evidence_id": "ux-parity.macos.unix-frontend.workbench",
      "state": "Unsupported",
      "platform": "macos",
      "root_cause": "platform-gap",
      "owner": "ui"
    },
    {
      "branch": "frontend-lx",
      "scenario": "linux-clipboard",
      "evidence_id": "ux-parity.linux.unix-frontend.clipboard",
      "state": "Unsupported",
      "platform": "linux",
      "root_cause": "platform-gap",
      "owner": "platform"
    },
    {
      "branch": "frontend-mx",
      "scenario": "macos-clipboard",
      "evidence_id": "ux-parity.macos.unix-frontend.clipboard",
      "state": "Unsupported",
      "platform": "macos",
      "root_cause": "platform-gap",
      "owner": "platform"
    },
    {
      "branch": "remote-ui",
      "scenario": "replaceable-client",
      "evidence_id": "ux-parity.remote-ui.replaceable-client",
      "state": "Supported",
      "platform": "windows",
      "root_cause": "",
      "owner": "ui"
    },
    {
      "branch": "remote-ui",
      "scenario": "selection",
      "evidence_id": "ux-parity.remote-ui.selection",
      "state": "Supported",
      "platform": "windows",
      "root_cause": "",
      "owner": "ui"
    }
  ]
}
```

## 可直接复制的当前轮次实例（CSV）

```text
run_id,timestamp_utc,suite,branch,scenario,evidence_id,platform,state,root_cause,owner
ci-2026-08-02-0001,2026-08-02T08:00:00Z,platform-ux-parity-smoke,startup,first-window-startup,ux-parity.startup,windows,Supported,,platform
ci-2026-08-02-0001,2026-08-02T08:00:00Z,platform-ux-parity-smoke,startup,startup-title,ux-parity.startup-title,windows,Supported,,platform
ci-2026-08-02-0001,2026-08-02T08:00:00Z,platform-ux-parity-smoke,ux-startup,gui-wake-contract,ux-parity.wake-coalescing,windows,Supported,,platform
ci-2026-08-02-0001,2026-08-02T08:00:00Z,platform-ux-parity-smoke,ux-startup,window-focus-contract,ux-parity.window-focus-contract,windows,Supported,,ui
ci-2026-08-02-0001,2026-08-02T08:00:00Z,platform-ux-parity-smoke,frontend-lx,linux-workbench,ux-parity.linux.unix-frontend.workbench,linux,Unsupported,platform-gap,ui
ci-2026-08-02-0001,2026-08-02T08:00:00Z,platform-ux-parity-smoke,frontend-mx,macos-workbench,ux-parity.macos.unix-frontend.workbench,macos,Unsupported,platform-gap,ui
ci-2026-08-02-0001,2026-08-02T08:00:00Z,platform-ux-parity-smoke,frontend-lx,linux-clipboard,ux-parity.linux.unix-frontend.clipboard,linux,Unsupported,platform-gap,platform
ci-2026-08-02-0001,2026-08-02T08:00:00Z,platform-ux-parity-smoke,frontend-mx,macos-clipboard,ux-parity.macos.unix-frontend.clipboard,macos,Unsupported,platform-gap,platform
ci-2026-08-02-0001,2026-08-02T08:00:00Z,platform-ux-parity-smoke,remote-ui,replaceable-client,ux-parity.remote-ui.replaceable-client,windows,Supported,,ui
ci-2026-08-02-0001,2026-08-02T08:00:00Z,platform-ux-parity-smoke,remote-ui,selection,ux-parity.remote-ui.selection,windows,Supported,,ui
```

## 本轮实测化进展（2026-08-02）

## 本轮 D3 结构收敛证据（2026-08-03）

| 收敛项 | 共享位置 | 行为证据 | Windows | Linux | macOS |
|---|---|---|---|---|---|
| selection gesture | `src/frontend/selection.rs` | `SelectionGesturePhase` + 泛型 `SelectionGestureState<TabId, Point>` 单份定义；Unix 与 Windows remote 共用同一状态机，adapter 只映射原生事件 | Supported | Supported | Supported |
| focus navigation | `src/frontend/interaction.rs` | `FocusState::navigate`（含 `focus_surface_navigation`），两侧只映射原生事件 | Supported | Supported | Supported |
| wheel accumulation | `src/frontend/interaction.rs` | `WheelAccumulator`，高分辨率增量按 `WHEEL_DELTA` 统一累积 | Supported | Supported | Supported |
| modal focus state | `src/frontend/interaction.rs` | `FocusState`（surface+gate），语义 surface 由两端存储，modal 打开时禁止 focus transition/navigation；两端 adapter 以 `focus_gate()` 单点映射原生 modal flags | Supported | Supported | Supported |
| wheel routing | `src/frontend/interaction.rs` | `route_wheel`，sidebar/terminal/ignored 目标统一 | Supported | Supported | Supported |
| alternate-screen wheel fallback | `src/commands.rs` + Windows remote | `alternate_screen_wheel_bytes` 单份编码；Windows ConPTY 会吞掉 alternate-screen 序列，remote 以 `application_cursor` 识别 raw full-screen 状态，无可滚动 viewport 时回退发送箭头序列 | Supported | Supported | Supported |
| scrollbar thumb drag | `src/frontend/interaction.rs` | `ScrollbarThumbDrag` + sidebar offset 单点计算 | Supported | Supported | Supported |
| sidebar scrollbar geometry | `src/ui_geometry.rs` | `sidebar_row_capacity` + `sidebar_scrollbar_geometry` 单点；Windows remote 与 Unix embedded 共用同一 track/thumb/row-capacity 算法 | Supported | Supported | Supported |
| composer send/input geometry | src/ui_geometry.rs | composer_geometry() 单点；Windows 控件 bounds、Unix send hit-test/render/snapshot 共用同一 input/send rect | Supported | Supported | Supported |
| system menu clipboard state | src/frontend/interaction.rs | system_menu_clipboard_state() 单点；edit-focus/terminal-ready/selection/clipboard 四输入，Windows remote 与 Unix embedded 共用同一 copy/paste enabled 策略 | Supported | Supported | Supported |
| modal/focus surface naming | `src/frontend/interaction.rs` | `ModalSurface` + `modal_surface_from_gate()` + `FocusSurface::as_str()/from_ipc()` 单点生成 ui-snapshot `modal`/`focus.surface` 与 IPC 别名；tab-editor 走独立 `tab_editor` 快照 | Supported | Supported | Supported |
| composer/workspace visibility | `src/frontend/interaction.rs` | `FocusTransitionGate::workspace_controls_visible()` 单点：window-close/settings/new-terminal/tab-close 隐藏 composer 与 workspace toolbar；cwd-editor 与 tab-editor 保持可见 | Supported | Supported | Supported |
| pointer modifiers | `agenterm-platform::contract::input::ModifierState` | Windows `ControlWindowEvent` 与 Unix `PixelWindowEvent` 都携带 modifiers；Win32 鼠标消息从同一 `current_modifiers()` 读取 | Supported | Supported | Supported |
| mouse report encoding | `src/frontend/interaction.rs` | `MouseReportEncoding` + `mouse_report_bytes`；Windows remote 与 Unix embedded 共用同一编码器 | Supported | Supported | Supported |
| raw-mouse arbitration | src/frontend/interaction.rs | mouse_report_outcome 统一裁决/编码，组合 mouse_delivery 与 mouse_report_bytes；两端 adapter 只映射原生事件 | Supported | Supported | Supported |
| selection autoscroll | src/frontend/selection.rs | utoscroll_step 单份计算；Unix embedded 与 Windows remote poll tick 共用同一拖出视口滚动语义 | Supported | Supported | Supported |
| word/line selection | `src/frontend/selection.rs` | `TerminalCellSource` + `word_selection_bounds` 统一 vt100 与 snapshot cell grid 的单词边界；Windows remote 双击/三击接线已补，raw-mouse 优先时仍走应用上报 | Supported | Supported | Supported |
| CWD editor modal | src/frontend/cwd_editor.rs | open/close/target/action/snapshot/submit_mode 单点；Unix embedded 与 Windows remote 共用 CwdEditorDialog，primary shortcut + Shift/Alt 模式选择单点，adapter 只保留原生编辑控件/焦点与平台命令执行 | Supported | Supported | Supported |
| CWD editor write modes | `src/frontend/composer.rs` | `ComposerWriteMode` 单点定义并供 Unix embedded、Windows remote、server dispatch 共用；Windows Ctrl+Enter=empty-only、Shift+Ctrl+Enter=append、Alt+Ctrl+Enter=replace 与 Unix 对齐 | Supported | Supported | Supported |
| keyboard/text-field policy | `src/frontend/input.rs` | Composer/TextField/Terminal shortcut 与 PTY key bytes 单点定义；Unix embedded 从 `src/platform/adapters/unix/frontend` 迁出引用 | Supported | Supported | Supported |
| new-terminal modal | src/frontend/new_terminal.rs | open/reset/校验/action 与 shell argv 单点；Unix embedded 迁出 adapter，Windows remote 继续原生控件呈现，状态/校验/action/argv 与 Unix 共用共享 dialog 与 platform policy | Supported | Supported | Supported |
| settings modal | src/frontend/settings.rs | open/重置/校验/scope/override/theme/action 单点；Unix embedded 与 Windows remote 共用 SettingsDialog，adapter 只负责原生呈现与事件映射 | Supported | Supported | Supported |
| live-tab close confirmation | src/frontend/close_confirmation.rs | open/close/snapshot 单点；Unix embedded 与 Windows remote 共用 CloseConfirmation，adapter 只负责原生确认控件与关闭执行 | Supported | Supported | Supported |
| inline tab editor | src/frontend/tab_editor.rs | open/draft/focus/校验/snapshot 单点；Unix embedded 与 Windows remote 共用 TabEditorDialog，adapter 只保留原生编辑控件/IME/事件映射 | Supported | Supported | Supported |
| window close confirmation | src/frontend/window_close.rs | choice/state/snapshot 单点；Unix embedded 与 Windows remote 共用 WindowCloseDialog/WindowCloseChoice，adapter 只保留原生窗口执行与按钮呈现 | Supported | Supported | Supported |

说明：Windows 列来自本地 Quick Gate/单元测试；Linux/macOS 列由 CI 全矩阵编译与 `unix-frontend-smoke` 真机证据支撑（见下方 D4）。


- 已落地收口：非平台目录 `src/` 里的平台差异判定，除必要 UI/UX 可见性场景外已从编译期 `cfg` 分裂收拢到平台能力入口：
  - `src/script_process.rs`：`long_running_process_*` 与 `shell_wrapped_process_command` 改为统一 `crate::platform::script_process_test_host_supported()` 运行时分支。
  - `src/script_stdlib.rs`：windows 特有路径替换测试改为运行时 `is_windows_host()` 返回早退。
  - `src/workspace.rs`：`unix_default_workspace_contains_workspaces_component` 改为运行时 host 早退。
- 与 O1C 对齐的当前建议：
  - 先在该收口后执行一次 `platform-ux-parity-smoke` 的分支回归（Windows / Linux / macOS 各自任务）补齐 `run_id` 覆盖。
  - 回归结果应落入本文件“JSON/CSV”字段，按 `evidence_id` 覆盖本轮三端状态。

### 最近一次 Windows 回归（`platform-ux-parity-smoke -- --emit-matrix`）

- `run_id`: `1785722327057-244172`
- `timestamp_utc`: `2026-08-03T01:59:23.373Z`
- `suite`: `platform-ux-parity-smoke`
- `failure`: 无（`result_class: success`）

| 分支 | 场景 | evidence_id | Windows | Linux | macOS | 归因 |
|---|---|---|---|---|---|---|
| startup | first-window-startup | `ux-parity.startup` | Supported | not-executed-yet | not-executed-yet |  |
| startup | startup-title | `ux-parity.startup-title` | Supported | not-executed-yet | not-executed-yet |  |
| ux-startup | gui-wake-contract | `ux-parity.wake-coalescing` | Supported | not-executed-yet | not-executed-yet |  |
| frontend-lx | linux-workbench | `ux-parity.linux.unix-frontend.workbench` | Unsupported | not-executed-yet | Unsupported | platform-gap |
| frontend-lx | linux-clipboard | `ux-parity.linux.unix-frontend.clipboard` | Unsupported | not-executed-yet | Unsupported | platform-gap |
| frontend-mx | macos-workbench | `ux-parity.macos.unix-frontend.workbench` | Unsupported | Unsupported | not-executed-yet | platform-gap |
| frontend-mx | macos-clipboard | `ux-parity.macos.unix-frontend.clipboard` | Unsupported | Unsupported | not-executed-yet | platform-gap |
| remote-ui | replaceable-client | `ux-parity.remote-ui.replaceable-client` | Supported | Unsupported | Unsupported | windows-only-contract |
| remote-ui | selection | `ux-parity.remote-ui.selection` | Supported | Unsupported | Unsupported | windows-only-contract |
| remote-ui | wheel/scrollback | `ux.mouse-scrollback` | Supported | Unsupported | Unsupported | windows-only-contract |
| remote-ui | selection copy | `ux.terminal-selection-copy` | Supported | Unsupported | Unsupported | windows-only-contract |
| remote-ui | CWD editor | `ux.working-context-cwd` | Supported | Unsupported | Unsupported | windows-only-contract |
| ux-startup | window-focus-contract | `ux-parity.window-focus-contract` | Supported | not-executed-yet | not-executed-yet |  |

### 上次 Windows 通过回归（`platform-ux-parity-smoke -- --emit-matrix`）

- `run_id`: `1785678683554-260172`
- `timestamp_utc`: `2026-08-02T13:52:00.636Z`
- `suite`: `platform-ux-parity`
- `failure`: 无（`result_class: success`）

| 分支 | 场景 | evidence_id | Windows | Linux | macOS | 归因 |
|---|---|---|---|---|---|---|
| startup | first-window-startup | `ux-parity.startup` | Supported | Failed | Failed | infra/platform-binary-missing |
| startup | startup-title | `ux-parity.startup-title` | Supported | Failed | Failed | infra/platform-binary-missing |
| ux-startup | gui-wake-contract | `ux-parity.wake-coalescing` | Supported | Failed | Failed | infra/platform-binary-missing |
| frontend-lx | linux-workbench | `ux-parity.linux.unix-frontend.workbench` | Unsupported | not-executed-yet | Unsupported | platform-gap |
| frontend-lx | linux-clipboard | `ux-parity.linux.unix-frontend.clipboard` | Unsupported | not-executed-yet | Unsupported | platform-gap |
| frontend-mx | macos-workbench | `ux-parity.macos.unix-frontend.workbench` | Unsupported | Unsupported | not-executed-yet | platform-gap |
| frontend-mx | macos-clipboard | `ux-parity.macos.unix-frontend.clipboard` | Unsupported | Unsupported | not-executed-yet | platform-gap |
| remote-ui | replaceable-client | `ux-parity.remote-ui.replaceable-client` | Supported | Failed | Failed | infra/platform-binary-missing |
| remote-ui | selection | `ux-parity.remote-ui.selection` | Supported | Failed | Failed | infra/platform-binary-missing |
| ux-startup | window-focus-contract | `ux-parity.window-focus-contract` | Supported | Failed | Failed | infra/platform-binary-missing |

下轮建议：
- 用 `platform-ux-parity-smoke-linux -- --emit-matrix` 补齐 Linux 列。
- 用 `platform-ux-parity-smoke-macos -- --emit-matrix` 补齐 macOS 列。

### 最近一次平台预检（Windows 主机）

- `platform-ux-parity-smoke-linux -- --emit-matrix`
- `run_id: 1785678831415-84776`
  - `suite: platform-ux-parity-smoke`
  - `failure: platform_gui_missing`
  - 根因归类：`infra/platform-binary-missing`
  - Linux 相关可执行场景状态示例：`ux-parity.startup`、`ux-parity.wake-coalescing`、`ux-parity.window-focus-contract` 为 `Failed`（`infra/platform-binary-missing`）。
- `platform-ux-parity-smoke-macos -- --emit-matrix`
  - `run_id: 1785678837047-34972`
  - `suite: platform-ux-parity-smoke`
  - `failure: platform_gui_missing`
  - 根因归类：`infra/platform-binary-missing`
  - macOS 相关可执行场景状态示例：`ux-parity.startup`、`ux-parity.wake-coalescing`、`ux-parity.window-focus-contract` 为 `Failed`（`infra/platform-binary-missing`）。

说明：这是当前 Windows 开发机的执行边界，不是回归逻辑失败；脚本已改为在该场景输出 `matrix` 与失败根因，便于三平台聚合不阻塞。

## 宿主矩阵门禁记录（v0.1.13 Wave B，2026-08-04）

| 宿主 | parity-smoke 任务 | 负责列 | 状态 | 证据 |
|---|---|---|---|---|
| Windows 开发机 | `platform-ux-parity-smoke` | windows | ✅ 绿 | `run_id 1785778126142-6792`（2026-08-04 01:29 +0800，`result_class: success`）：startup×2 / wake-coalescing / window-focus-contract / remote-ui×2 全 `Supported`；frontend-lx/mx 为平台能力缺口 `Unsupported`（platform-gap），Windows 主机上属预期 |
| Linux matching-host CI | `platform-ux-parity-smoke-linux` | linux | 归 CI | ubuntu runner `check.sh --quick --target x86_64-unknown-linux-gnu` + 真机 `unix-frontend` smoke；Windows 主机预检仅 `platform_gui_missing`（`infra/platform-binary-missing`，非回归） |
| macOS matching-host CI | `platform-ux-parity-smoke-macos` | macos | 归 CI | macos-15-intel / macos-14 runner；Windows 主机预检同上 `platform_gui_missing` |

门禁规则：
- Windows 列由本机 `platform-ux-parity-smoke -- --emit-matrix` 提供；绿 = `result_class: success` 且 startup / wake / focus / remote-ui 无 `Failed`。
- Linux/macOS 列只认 matching-host CI 回执；Windows 主机上 linux/macos 任务输出 `platform_gui_missing` 属基础设施边界，不算回归失败，也不冒充三平台绿。
- 任何 `Failed` 阻断对应分支；`Unsupported` 仅记录能力缺口（`platform-gap` / `windows-only-contract`）。

## 本轮 D4 真机 UX 证据（2026-08-02 CI run 30767566925）

| 场景 | evidence_id | Linux | macOS | 证据来源 |
|---|---|---|---|---|
| Unix workbench | `ux.unix-frontend-linux-workbench` / `ux.unix-frontend-macos-workbench` | Supported | Supported | Linux x86_64 与 macOS aarch64 真机 smoke PASS |
| Unix native clipboard | `ux.unix-frontend-linux-native-clipboard` / `ux.unix-frontend-macos-native-clipboard` | Supported | Supported | Linux x86_64 与 macOS aarch64 真机 smoke PASS |
| stale paste isolation | `ux.unix-frontend-linux-stale-paste` / `ux.unix-frontend-macos-stale-paste` | Supported | Supported | Linux x86_64 与 macOS aarch64 真机 smoke PASS |
| no-activate launch | `ux.unix-frontend-linux-no-activate` / `ux.unix-frontend-macos-no-activate` | Supported | Supported | Linux x86_64 与 macOS aarch64 真机 smoke PASS |
