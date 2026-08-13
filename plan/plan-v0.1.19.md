# AgenTerm v0.1.19 草案

状态：**预开草案**（不是在制唯一版本计划）。在制仍是
[`plan-v0.1.18.md`](plan-v0.1.18.md)。本文件冻结 0.1.18 关闭后立刻开工的
**整棵已接受树**，不是一条「先做半屏」的示范轨。

不创建 tag / Candidate / Release，除非人工明确授权。

工作方法（与 [`AGENTS.md`](../AGENTS.md) §Planning 第 6 条一致）：

1. 宏观一次画全：已接受的动作表、平台写框、命令/授权、历史、证据矩阵。
2. 按真实依赖拆并行枝，**同时写**。
3. 用测试和黑盒暴露偏差，再调，而不是先交一个 PoC/MVP 再排队加功能。
4. 只有热文件冲突或缺机制才串行。「先做小的」不是依赖。

## 主题

| 轨 | 范围 | 产品合同 |
|----|------|----------|
| **A. App Substrate Phase 1** | 0.1.18 §1.9 已预订的 CC 静态语义（展开仍以那节为准） | [10](../prd/PRD_02_10_rhai_scripting.md) / [21](../prd/PRD_02_21_control_center.md) |
| **D+. cu window-place** | Spectacle **整张**命名摆放进 `agenterm-cu` | [32](../prd/PRD_02_32_cu_window_placement.md) |

两轨并行、互不阻塞。D+ 按下面 DAG **整图并发**，不以 WP1→WP2 削成串行示范。

## D+ 用户问题

编排器已经能观察/点击，仍不能按稳定 Action ID 摆窗。日用热键继续用 Spectacle
1.2.1 宿主；`cu window-place` 必须覆盖目录里的全部动作，而不是先交半屏再「以后再说」。

## 不变量

- 不把 Spectacle.app、热键、菜单栏、登录项、`Shortcuts.json` 搬进 agenterm。
- 几何纯函数，无 OS import。写框只经 `agenterm-platform`（cu 不直调 AX/Win32/X11）。
- `window-place` 是 `actuate`。无 grant → `refused`；审计写失败 → 不移动。
- Action ID 双写（kebab + `SpectacleWindowAction*`），见 [32](../prd/PRD_02_32_cu_window_placement.md)。
- 不 sleep；完成观察走已有 `wait` / `windows`。
- **已接受的 18 个 ID 都在本版图内。** 缺机制用 typed `unsupported`/`failed`，
  不用「顺延到下个 MVP」当借口。

## 宏观图（一次接受）

```text
D+ window-place
│
├─ G 几何核（纯 Rust，无 OS）
│    全部 place 动作：center / fullscreen / 四半屏循环 / 四角循环
│    / next-previous third / next-previous display / larger / smaller
│    fixture ≡ Spectacle *CalculationSpec（≤1 pt）
│
├─ P 写框机制（agenterm-platform，三平台同时铺）
│    枚举顶层窗 + focused 默认
│    set-rect：size→position→size
│    quantized −2pt / 85% + 居中
│    best-effort clamp + round
│    sheet/系统对话框 → failed
│    Win：已有 window_op::move_window，接上管线
│    macOS：补 enum + AX set-rect（unix stub 今天是 Unsupported）
│    Linux：已有 X11 enum；补 _NET_WM 或等价 move；不能写则 typed unsupported
│
├─ C 命令面 + 授权（crates/agenterm-cu）
│    Command::WindowPlace { action, window }
│    18 个 ID 全部可解析
│    actuate + 审计前后；observe 必须 refused 且窗不动
│
└─ H 每应用 undo/redo 历史
     与 G/P/C 并行设计；落地依赖 P 能读/写框
     无历史实现时 undo/redo 返回 typed unsupported（保留 ID），
     不是从目录里删掉
```

真实汇合点只有一个：**I 集成 + 证据矩阵**（G∩C 的纯测；P∩C 的黑盒；H 有则进矩阵）。

```text
G ────────┐
P ────────┼── I  全动作 × 有后端的平台
C ────────┤      测偏 → 改几何或管线 → 再测
H ──(需 P)┘
```

## 并行叶（不是阶段）

同时开工，文件互斥：

| 枝 | 文件域 | 完成定义 |
|----|--------|----------|
| **G** | 新几何模块（cu 内或 platform-neutral lib）+ fixture | 目录内每个计算动作有测；与 Spectacle spec 1 pt 对齐 |
| **P-mac** | `adapters/macos/` enum + window_op | `move_window` / enumerate 不再走 unix stub |
| **P-lin** | `adapters/linux/window_op`（或现有 X11） | 能写框或诚实 unsupported |
| **P-win** | 接现有 `window_op` + 量化/夹紧 | 与 G 的 rect 对接，不另造 Win 几何 |
| **C** | `command.rs` / `executor.rs` / `cu.rs` / audit | 动词进枚举；无 grant 黑盒 |
| **H** | 历史结构 + 每 bundle 栈 | 测：连续两次 place 后 undo 回到 before |
| **I** | 黑盒 / smoke | 每个已接线平台 × 每个非 unsupported 动作 |

禁止再出现「先 G 的四个 half，再 P 的一条 macOS 竖线，再顺延 thirds」。

## Gate（汇合，不是队列）

| Gate | 证明 | 失败 |
|------|------|------|
| **G-contract** | PRD 32 + Spectacle `FEATURE-CATALOG` 与上表一致 | 不写产品代码（已满足则可立刻并行开枝） |
| **G-all** | I 在至少一个真实 `current` 宿主上对**已接线**动作全绿；其余平台 typed；undo/redo 要么历史测绿要么 typed unsupported | 不得把 `window-place` 标 shipped |

没有「G-half-only」门。

## 非目标（整图排除，不是下一期 backlog）

- 菜单栏宿主、全局热键、Rectangle 扩展、把 Spectacle.app 嵌进来。
- ssh/rdp/vnc 上的摆放（那是 30 的远程档，不是本图的「以后再加半屏」）。
- 公开发布「cu 已是窗口管理器」替代本机 Spectacle 1.2.1。

## 与 0.1.18 / 轨 A

关闭 0.1.18 **或**负责人授权 D+ 并行后整图开工。  
G 不碰 Host ABI，可与 CC Phase 1 并行。  
P 的 platform 文件与 0.1.18 轨 D 原型若撞热文件，**按文件互斥分人**，不把整条 D+ 停成单线程。
