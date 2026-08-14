# AgenTerm v0.1.19 草案

状态：**预开草案**（不是在制唯一版本计划）。在制仍是
[`plan-v0.1.18.md`](plan-v0.1.18.md)。本文件冻结「0.1.18 关闭后立刻收口」的
已接受叶，并记录 window-place / 热键宿主已提前落地的部分，避免合同与代码分叉。

不创建 tag / Candidate / Release，除非人工明确授权。

## 主题

两条**并行、互不阻塞**的轨：

| 轨 | 范围 | 产品合同 |
|----|------|----------|
| **A. App Substrate Phase 1** | 首条真实 CC 静态语义竖线（0.1.18 §1.9 已预订） | [10](../prd/PRD_02_10_rhai_scripting.md) / [21](../prd/PRD_02_21_control_center.md) |
| **D+. cu window-place** | Spectacle 命名摆放收进 `agenterm-cu`；**代码已先于本版关闸落地（macOS）** | [32](../prd/PRD_02_32_cu_window_placement.md) |

轨 A 的展开仍以 0.1.18 §1.9 为准。D+ 的实现进度以仓库代码与 PRD 32 勾选为准，
不因本文件仍写「草案」而假装未开工。

## D+ 用户问题

agent 已经能 `windows` / `tree` / `click`，还需要像人按热键那样把窗口甩到
左半 / 全屏 / 另一块屏。编排器用 `agenterm-cu window-place`；人机日用走
`agenterm-cu host` / `AgentermCu.app`（Spectacle 默认键位），不再依赖本机 Spectacle.app。

## 不变量

- 几何纯函数，无 OS import；写框只经 `agenterm-platform`（macOS 先 AX）。
- `window-place` 是 `actuate`。无 grant → `refused`；审计写失败 → 不移动。
- Action ID 与 Spectacle 常量双写（kebab + `SpectacleWindowAction*`），见
  [32](../prd/PRD_02_32_cu_window_placement.md)。
- 不 sleep；完成观察走已有 `wait` / `windows`。
- 日用热键宿主可以存在，但**不是**第二套几何：只调用同一 `window-place` 管道。
- 辅助功能信任看 **launchd 进程 + 当前签名**，不是 Settings 标签 alone；见
  `docs/agenterm-rust-cheatsheet.md`（macOS Accessibility trust）。

## 叶（D+）— 进度与代码对齐

宏观整图一次画全，独立枝并行，不以「先半屏再 thirds」的 MVP 竖线推进。

- [x] **目录冻结** — [32](../prd/PRD_02_32_cu_window_placement.md) 与
  Spectacle `docs/FEATURE-CATALOG.md` 动作 ID 对上。
- [~] **几何核** — 18 动作纯函数 + fixture（half 循环、thirds、跨屏、量化等）；
  以 crate 测试为据，继续对齐边缘案例。
- [~] **`agenterm-cu window-place` 竖线** — `current` + macOS AX set-rect；Windows 接
  既有 move；Linux 写框仍可 typed unsupported。证据：真实 `agenterm-cu` 移动可见窗。
- [~] **授权/审计** — 无 `actuate` → `refused`；审计走 cu 既有模型。
- [~] **日用宿主** — `scripts/install-cu-hotkeys.sh` → `~/Applications/AgentermCu.app`
  + launchd + 菜单栏；Carbon 默认键位。TCC 安装诚实（重签 reset）。
- [ ] **跨 OS 写框完整化** — Linux/Windows 与 macOS 同一动词语义的公共黑盒。
- [ ] **undo/redo 产品历史** — ID 已保留；每应用历史的公共证据未关。

## Gate

| Gate | 必须证明 | 不通过时 |
|------|----------|----------|
| **G-WP-math** | 几何 fixture 全绿 | 不宣称「行为等于 Spectacle」 |
| **G-WP-mac** | macOS `agenterm-cu window-place` + grant 黑盒 | 不得把 32 标 shipped |
| **G-WP-host** | launchd `ax-status` `trusted=1` 且热键能改窗 | 不得说「已取代 Spectacle」而不带 TCC 说明 |

## 非目标

- Rectangle 扩展功能（gaps、自定义区域等）除非另开 ID。
- 嵌入 Spectacle.app / Sparkle / JavaScriptCore。
- ssh/rdp/vnc 上的摆放（动词可先 `unsupported`）。
- 公开发布「cu 已是通用窗口管理器」。

## 开工条件

1. D+ 已提前实现的部分不回滚；0.1.18 关闭时把本文件的勾选与 PRD 32 对齐一次。
2. 轨 A（CC Phase 1）不在此重写。两轨抢人时：几何与 platform set-rect 已分文件，
   继续避免与 0.1.18 轨 D 原型抢同一热文件。

## CU Windows desktop-host checkpoint

- [x] 唯一 executable 为 `agenterm-cu`；CLI 与 `host` 共用一个 binary。
- [x] CU 是首个运行时 `libagenterm` 消费者。Windows desktop-host ABI 1.7
  已实现 notification area/menu/`RegisterHotKey`，CU 目录为 18 个 placement
  action 加 Quit。
- [x] 本机 `target/abi-dev` `host --self-test --json` 已报告
  `actions=19`、`cleaned_up=true`。
- [ ] 正式 `dist`、Candidate、qualification 尚未闭环；本 checkpoint 只能
  支持 `[~]`，不得把 CU 根或 Windows 正式交付标为 shipped。