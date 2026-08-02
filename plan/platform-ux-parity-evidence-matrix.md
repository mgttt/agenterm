# 平台 UX 对齐证据矩阵（并发回归模板）

本文件用于每轮回归后的“分支-场景-证据”归并。未通过项直接阻断该分支收敛，先补齐 `platform` 能力或产品行为再推进。

## 并发执行入口

- Windows: `agenterm-rhai task run platform-ux-parity-smoke -- --list-evidence`
  - fallback: `agenterm-rhai task run platform-ux-parity-smoke -- --list-evidence`
  - 回归: `agenterm-rhai task run platform-ux-parity-smoke -- --emit-matrix`
- Linux: `agenterm-rhai task run platform-ux-parity-smoke-linux -- --list-evidence`
  - fallback: `agenterm-rhai task run platform-ux-parity-smoke-linux -- --list-evidence`
  - 回归: `agenterm-rhai task run platform-ux-parity-smoke-linux -- --emit-matrix`
- macOS: `agenterm-rhai task run platform-ux-parity-smoke-macos -- --list-evidence`
  - fallback: `agenterm-rhai task run platform-ux-parity-smoke-macos -- --list-evidence`
  - 回归: `agenterm-rhai task run platform-ux-parity-smoke-macos -- --emit-matrix`

## 分支-场景归并表（按循环填充）

| 分支 | 场景 | 对应 evidence | Windows | Linux | macOS | 归因标签 | 下一步动作 |
|---|---|---|---|---|---|---|---|
| startup | 首窗口/启动合同 | `ux-parity.startup` | Supported | Supported | Supported | - | - |
| startup | 启动标题/窗口恢复 | `ux-parity.startup-title` | Supported | Supported | Supported | - | - |
| ux-startup | 唤醒契约 | `ux-parity.wake-coalescing` | Supported | Supported | Supported | - | - |
| ux-startup | 焦点恢复 | `ux-parity.window-focus-contract` | Supported | Supported | Supported | - | - |
| frontend-lx | Workbench 与窗口行为（linux） | `ux-parity.linux.unix-frontend.workbench` | Unsupported | Unsupported | Unsupported | platform-gap | - |
| frontend-lx | 剪贴板语义（linux） | `ux-parity.linux.unix-frontend.clipboard` | Unsupported | Unsupported | Unsupported | platform-gap | - |
| frontend-mx | Workbench 与窗口行为（macos） | `ux-parity.macos.unix-frontend.workbench` | Unsupported | Unsupported | Unsupported | platform-gap | - |
| frontend-mx | 剪贴板语义（macos） | `ux-parity.macos.unix-frontend.clipboard` | Unsupported | Unsupported | Unsupported | platform-gap | - |
| remote-ui | replaceable 客户端 | `ux-parity.remote-ui.replaceable-client` | Supported | Unsupported | Unsupported | windows-only-contract | - |
| remote-ui | selection 语义 | `ux-parity.remote-ui.selection` | Supported | Unsupported | Unsupported | windows-only-contract | - |

> 说明：
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
- 与 [plan/plan-unix-gui-win-parity.md](plan/plan-unix-gui-win-parity.md) 的 O3 结果页同步。

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
