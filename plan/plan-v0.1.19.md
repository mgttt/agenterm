# AgenTerm v0.1.19 草案

状态：**预开草案**（不是在制唯一版本计划）。在制仍是
[`plan-v0.1.18.md`](plan-v0.1.18.md)。本文件只冻结「0.1.18 关闭后立刻开工」的
已接受叶，避免目录在口头里漂。

不创建 tag / Candidate / Release，除非人工明确授权。

## 主题

两条**并行、互不阻塞**的轨：

| 轨 | 范围 | 产品合同 |
|----|------|----------|
| **A. App Substrate Phase 1** | 首条真实 CC 静态语义竖线（0.1.18 §1.9 已预订） | [10](../prd/PRD_02_10_rhai_scripting.md) / [21](../prd/PRD_02_21_control_center.md) |
| **D+. cu window-place** | 把 Spectacle 命名摆放收进 `agenterm-cu`，**本版开始做** | [32](../prd/PRD_02_32_cu_window_placement.md) |

轨 A 的展开仍以 0.1.18 §1.9 为准，关闭 0.1.18 时再写成 must-ship 叶。本文把笔墨
留给 D+，因为目录已经冻住、实现尚未开始。

## D+ 用户问题

agent 已经能 `windows` / `tree` / `click`，仍不能像人按 Spectacle 热键那样把窗
口甩到左半 / 全屏 / 另一块屏。日用热键继续用 Spectacle 1.2.1 宿主；`cu` 需要
同一套**命名动作**给编排器用。

## 不变量

- 不把 Spectacle.app、热键、菜单栏、登录项、`Shortcuts.json` 搬进 agenterm。
- 几何纯函数，无 OS import；写框只经 `agenterm-platform`（macOS 先 AX）。
- `window-place` 是 `actuate`。无 grant → `refused`；审计写失败 → 不移动。
- Action ID 与 Spectacle 常量双写（kebab + `SpectacleWindowAction*`），见
  [32](../prd/PRD_02_32_cu_window_placement.md)。
- 不 sleep；完成观察走已有 `wait` / `windows`。
- 本版**开始**即可，不要求一次做完整张 18 动作表。

## 叶（D+）

- [ ] **CU-WP0 目录冻结** — [32](../prd/PRD_02_32_cu_window_placement.md) 与
  Spectacle `docs/FEATURE-CATALOG.md` 对上。证据：本文件与两份合同交叉链接。
  安全失败：没有合同不写产品代码。
- [ ] **CU-WP1 几何核** — Rust 纯函数覆盖 `center` / `fullscreen` / 四个 half
  （含 1/2→2/3→1/3 循环）。fixture 对齐 Spectacle `*CalculationSpec`（1 pt）。
  非目标：本叶不碰 AX。
- [ ] **CU-WP2 `cu window-place` 竖线** — `current` + macOS，缺省前台窗或
  `--window`。platform 增加 set-rect；cu 不直调 AX。证据：真实 `cu` 移动一个
  可见窗，随后 `windows` 读到新 bounds。
- [ ] **CU-WP3 授权/审计** — 与 WP2 同批，不得后补。未授权窗不动。
- [ ] **CU-WP4 同版可顺延** — thirds / corners / display walk / larger/smaller。
  做不完不挡 0.1.19 关闭；ID 保持 reserved。`undo`/`redo` 明确更后。

## Gate

| Gate | 必须证明 | 不通过时 |
|------|----------|----------|
| **G-WP0** | WP0 合同交叉链接在树里 | 不写产品代码 |
| **G-WP1** | WP1 fixture 全绿 | 不宣称「行为等于 Spectacle」 |
| **G-WP2** | WP2+WP3 黑盒同时过 | 不得把 `window-place` 标 shipped |

## 非目标（本版）

- 菜单栏宿主、全局热键、Rectangle 扩展功能。
- Linux/Windows 写框（动词可以先 `unsupported`）。
- ssh/rdp/vnc 上的摆放。
- 公开发布「cu 已是窗口管理器」。
- 替换或停止本机 Spectacle 1.2.1。

## 开工条件

1. [`plan-v0.1.18.md`](plan-v0.1.18.md) 关闭或产品负责人明确授权并行开工 D+。
2. G-WP0 已满足（本文落地即视为满足，可立刻写几何核）。

轨 A（CC Phase 1）不在此重写。两轨抢人时：**几何核（WP1）可先于 CC**，因为它
不碰 Host ABI；WP2 需要 platform set-rect，避免和 0.1.18 轨 D 原型抢同一热文件
时串行。
