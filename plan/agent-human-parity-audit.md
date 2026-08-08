# Agent↔Human 交互面对齐审计

> **一句话**：产品要同时面对人类与 agent，则 **agent 能做的** 必须可证地覆盖
> **人类能做的**（键盘 / 鼠标 / 声音 / 截图 / 结构化内容树）。本文件记录
> 2026-08-08 的一次证据化审计与由此排出的待办，**不是**已排期的工作树。

| 字段 | 值 |
|------|-----|
| **审计日期** | 2026-08-08（HEAD `506395d8`） |
| **方法** | 只读代码取证，逐条 `file:line`；不采信文档自述 |
| **相关** | [`goal-cli-input-parity.md`](goal-cli-input-parity.md)（本轮 `ui-input` 的上游任务书）、[`plan-unix-gui-win-parity.md`](plan-unix-gui-win-parity.md)、[`platform-ux-parity-evidence-matrix.md`](platform-ux-parity-evidence-matrix.md) |
| **owning PRD** | [`PRD_02_07_agent_control_plane.md`](../prd/PRD_02_07_agent_control_plane.md)（观察/动作）、[`PRD_02_15_command_line.md`](../prd/PRD_02_15_command_line.md)（CLI 契约） |

---

## 0. 结论先读

**设计是对的，落地只落了一半平台。**

`ui-input`（像素级 pointer / wheel / key）在 Unix 前端的实现**没有第二套
hit-test**：`apply_pointer_request`（`src/platform/adapters/unix/frontend/mod.rs:5940`）
合成真实 `PixelWindowEvent` 喂进人类鼠标同一个 `handle_pixel_event`
（`:6087`），press/move/release 成对、多击是真的 N 对、拖拽时末次 press
故意保持按住（`:5978-5987`）。`ui-snapshot` 的 bounds 覆盖面也确实厚，并带
focus / caret / anchor / selection，手势可机器验证。

**但**：

> **「agent 能做人类能做的事」目前在 Linux/macOS 成立（那边人类 GUI 仍 `[~]`），
> 在 Windows 不成立（而那是 shipped 的人类平面）。**

外加一类**不是功能缺失、而是能力发现面在说谎**的问题（F3/F4）：agent 照
`--help` 学产品会被引到死命令，同时看不见活命令。对一个要被 agent 自主
驱动的产品，这比缺功能更伤——**自我迭代的前提是产品对自己的描述是真的**。

### 五条人类能力轴的现状

| 轴 | agent 侧现状 | 缺口 |
|----|-------------|------|
| **键盘** | `send-keys`（PTY 层）✅ + `ui-input key`（GUI 层，Unix only） | Windows 无 GUI 层键入；`key` 未注册进 `operations.rs` |
| **鼠标** | `ui-input pointer/wheel`（GUI 层，Unix only）✅ | Windows 无；**PTY 层鼠标 `send-mouse` 声明但零实现**（F3） |
| **截图** | `screenshot` / `screenshot-pane` 三平台可用 ✅ | headless 无路径（依赖活客户端） |
| **结构化树** | `ui-snapshot` + `list-tab-tree` + `capture-pane` ✅ 厚 | headless 无几何（F2）；模态/系统菜单无 bounds（F6）；Win 缺 composer caret（F7） |
| **声音** | **完全不存在** | 见 F8——注意这是**双侧**缺口，不是对齐缺口 |

---

## 1. 发现表

| # | 面 | 证据 | 严重度 | 状态 |
|---|-----|------|--------|------|
| **F1** | `ui-input` **Windows 完全未实现** | `src/platform/adapters/windows/remote_frontend.rs:8124-8144` 自带 `REVIEW(macos → windows owner)` 注释，明说「on Windows the command currently does nothing」；`src/server_app.rs:1398-1411` 的客户端中继白名单含 `ui-action\|focus\|get-settings\|set-setting\|screenshot\|screenshot-pane\|screenshot-tab`，**不含 `ui-input`**，故落到 `:1465` 的 `server_command_unsupported` | **P0** | 开 |
| **F2** | headless 快照**无任何几何** | `src/server_app.rs:1937-1977`（`projection: "headless_server"`）：`layout` 只有硬编码 `composer:{visible:false,…}`（`:1951-1957`），`focus.surface` 硬编码 `null`（`:1959`），tabs 只有 id/title/pid/state/rows/cols，**无 bounds / selection / render** | **P0** | 开（含决策项 D-1） |
| **F3** | `send-mouse` 是**活着的谎**：四处声明、零处 dispatch，却仍在 `extensions` 广告 | 声明 `src/commands.rs:61,734`、`src/control_authority.rs:251`、`src/client/mod.rs:5039,5146`；全仓无 dispatch 分支；Unix 落 `unix/frontend/mod.rs:4682-4695` 的 `unix_gui_unsupported`。且其坐标系是**终端 cell**（`-x col -y row`），本就够不到工具栏 | **P1** | 开 |
| **F4** | `ui-input` **不在 `--help`** | `src/client/mod.rs:5019-5049` 列了 `ui-snapshot`/`ui-action`/`ui-hello`/`ui-bootstrap`/`ui-deltas`/`send-mouse`，**唯独没有 `ui-input`** | **P1** | 开 |
| **F5** | typed 目录漂移 | `src/operations.rs` 只注册 **10** 个 `ui-action`（`:482-647` + `open-control-center`）；`src/frontend/ui_action_catalog.rs:29-88` 的 `SHARED_UI_ACTIONS` 有 **58** 个、`UNIX_ONLY_UI_ACTIONS`（`:109-121`）**11** 个；CLI help 广告约 40 个。另：`ui-input key` **未注册**（`operations.rs:749-756` 对非 pointer/wheel 返回 `Ok(None)`），`POINTER_PARAMETERS`（`:181-196`）只声明 `x`/`y`，`button`/`action`/`count`/`mods`/`delta-y`/`units`/`key` 全未声明 | **P1** | 开 |
| **F6** | 模态框 / 系统菜单**只给 kind+actions，不给 bounds** | `snapshot_modal()`：`src/frontend/settings.rs:342-354`、`new_terminal.rs:181-189`、`instance_picker.rs:160-173`、`cwd_editor.rs:78-82`、`window_close.rs:46-48`、`close_confirmation.rs:39`；人类那侧**有**完整 hit-test：`unix/frontend/render.rs:413,541,766,809`。`system_menu_json`（`src/ui_snapshot.rs:127-149`）同样只给 id/label/enabled | **P2** | 开 |
| **F7** | Windows 快照缺 composer caret/anchor/selection | Unix 有（`unix/frontend/mod.rs:3025-3038`）；`remote_frontend.rs` 全文无 `"caret"`/`"anchor"`/`"draft_length"` | **P2** | 开 |
| **F8** | **声音：产品完全不存在**（双侧缺口） | vt100 内核**已解析** BEL：`third_party/vt100/src/perform.rs:44` → `callbacks.audible_bell()`、`:74` → `visual_bell()`；但 `third_party/vt100/src/callbacks.rs:6,9` 是空默认实现，**`src/` 全仓无人覆盖**。全仓无 `cpal`/`rodio`/`winmm`/`PlaySound`/`Beep` 依赖 | **P2**（产品决策） | 开（决策项 D-2） |

### 已经做对、**不要回退**的部分

- `ui-input` 走**同一条** `handle_pixel_event`，无第二套 hit-test
  （`goal-cli-input-parity.md` T1 的硬性约束已被遵守）——**任何 Windows 移植
  必须保持这条**，不得为绕开 native `EDIT` composer 而另写一份命中测试。
- `ui-snapshot` 的 bounds 覆盖：工具栏逐按钮（`:6721-6731`）、tab 行 8 个子矩形
  （`:2852-2871`）、`actions.new_child`/`actions.close`（`:2770-2812`）、滚动条
  track/thumb、sidebar `resize_grip`、composer input、server strip 芯片
  **及右键菜单项 bounds**（`:2914-2955`，注释明说是为了让 agent 用
  `ui-input pointer` 点）。
- `WINDOWS_ONLY_UI_ACTIONS` 已归零（`src/frontend/ui_action_catalog.rs:98`），
  剩余不对称是**反向**的 11 个 Unix-only。

---

## 2. 排序建议（成本 / 依赖 / 泳道）

| 序 | 叶 | 内容 | 成本 | 泳道 / 热域 | 依赖 |
|----|-----|------|------|------------|------|
| 1 | **P-honest** | F3 + F4 + F5：删或实现 `send-mouse`、`ui-input` 进 `--help`、补 `operations.rs` 登记与参数声明 | **小**（半天量级） | `src/client/mod.rs`、`src/commands.rs`、`src/operations.rs`、`prd/` | 无 |
| 2 | **P-win-input** | F1：Windows `ui-input` 移植 + `server_app.rs` 中继白名单 | **大** | Win-UX（`remote_frontend*`） | 1（先让目录诚实） |
| 3 | **P-headless** | F2：headless 供几何 | **中** | `server_app.rs` | **决策项 D-1** |
| 4 | **P-modal** | F6 + F7：模态/菜单 bounds、Win composer caret | 中 | 两端 | 2 |
| 5 | **P-bell** | F8：BEL 是否落地（视觉/声音/事件三选） | 小–中 | vt100 + frontend | **决策项 D-2** |

**关键提醒（来自 `goal-cli-input-parity.md` T2）**：动 `src/operations.rs` 加公开
命令时，`tests/rhai_migration.rs` 的
`prd_alignment_task_matches_public_catalogs_and_fails_closed` 会 fail-closed，
**必须同步更新 `prd/` 目录与该测试里 pin 的计数串**。

---

## 3. 决策项（agent 不自主拍板）

| ID | 题 | 阻塞 | 说明 |
|----|-----|------|------|
| **D-1** | headless 是否供几何？ | P-headless，进而阻塞 `ui-input` 的**全部** CI 覆盖 | 现状：`ui-input` **零行为测试**——仅 `src/frontend/pointer_input.rs:254-454` 的参数解析单测，`tests/` 全目录无 `ui-input` 引用。因为 perceive→act 闭环需要活窗口，CI 跑不了。供几何 = 让 headless 也能被机器驱动与回归；不供 = 这条最重要的 agent 接口**长期只能人工验证** |
| **D-2** | BEL 落地形态？ | P-bell | 三选（可叠加）：① 事件化（进 Observable Fleet 序列，agent 可 `wait`）② 视觉（状态栏/标签闪烁）③ 真声音（引入音频依赖，影响体积预算）。**建议至少做 ①**——它同时补上人类与 agent 两侧，且不碰体积预算 |
| **D-3** | `send-mouse` 删除还是实现？ | P-honest | 删 = 承认 `ui-input pointer` 已取代它（但 PTY 层鼠标——例如在 `htop` 里点进程——仍无 agent 通路）；实现 = 补齐 PTY 层鼠标上报（编码已在 `agenterm_platform::terminal_input`，C5 已落地） |

> **D-3 的产品含义**：人类在 `htop`/`vim` 里点鼠标走的是 **PTY 层**（应用协商
> `?1002h;?1006h` 后收到上报），和点工具栏走的 **GUI 层**是两个子系统。
> `ui-input` 只覆盖了 GUI 层。**agent 目前无法在全屏 TUI 应用内点击**，
> 而人类可以——这是「鼠标」这条轴上真正剩下的对齐缺口。

---

## 4. 与 computer-use（L-CU）的关系

`plan-v0.1.15.md` §5.6.1 的核心判断——**`current` 不是一种远程协议，而是协议族
的 local 退化档**，与 ssh/rdp/vnc 共用同一套抽象命令集（截图 / 枚举窗口与控件树 /
点击 / 输入 / 剪贴板 / 文件传输）——意味着：

> **本文件的对齐工作 = computer-use 的 `current` 档。** 二者不是两件事。

而且 agenterm 作为 computer-use **目标**有一个别人没有的优势：通用
computer-use 靠截图 + OCR + 猜坐标，不可靠；agenterm 的 `ui-snapshot` 直接给
**精确 bounds**。把 F1/F2/F6 补齐，等于让 agenterm 成为**第一个自带结构化
控件树的 computer-use 目标**——这比做一个通用 computer-use 客户端更有产品
差异度。L-CU 立项（决策项 P4）时应把本文件当作 `current` 档的现状输入。

---

*执行投影，非产品宪法。能力状态以 PRD 为准。*
