# Agent↔Human 交互面对齐审计

> **一句话**：产品要同时面对人类与 agent，则 **agent 能做的** 必须可证地覆盖
> **人类能做的**（键盘 / 鼠标 / 声音 / 截图 / 结构化内容树）。本文件记录
> 2026-08-08 的一次证据化审计与由此排出的待办，**不是**已排期的工作树。

| 字段 | 值 |
|------|-----|
| **审计日期** | 2026-08-08（HEAD `506395d8`） |
| **方法** | 只读代码取证，逐条 `file:line`；不采信文档自述 |
| **相关** | [`archive/goal-cli-input-parity.md`](archive/goal-cli-input-parity.md)（历史上游任务书）、[`plan-unix-gui-win-parity.md`](plan-unix-gui-win-parity.md)、[`platform-ux-parity-evidence-matrix.md`](platform-ux-parity-evidence-matrix.md) |
| **owning PRD** | [`PRD_02_07_agent_control_plane.md`](../prd/PRD_02_07_agent_control_plane.md)（观察/动作）、[`PRD_02_15_command_line.md`](../prd/PRD_02_15_command_line.md)（CLI 契约） |

---

## 0. 北极星（2026-08-08 用户原话，落盘防丢）

> **通过 agenterm 工具能 100% 操控自身和所有能控制的资源（硬件、进程）并获取
> 反馈（截图、视频、流式结构化数据等等），未来才能跟大模型自主反馈式自进化。**

这条重新定义了本文件的评分标准：**不是「补齐 CLI 动词」，而是「把
perceive→act 闭环收紧到 LLM 能自己开、自己看结果、自己改进」**。据此，
缺一条反馈通道不是「少个功能」，是**自进化回路上的一个洞**。

### 两轴现状

**① 操控**

| 面 | 状态 |
|----|------|
| **自身**（GUI/终端/工作区） | 见下 §0.1—§2：Unix 接近闭环；**Windows 像素输入已补齐**（§1.4，2026-08-08）；余：模态/菜单无 bounds（F6，现为最大瓶颈）、原生 `EDIT` 内部不可达 |
| **可控资源**（硬件、进程） | **未立项** —— computer-use（L-CU，`plan-v0.1.15.md` §5.6.1），决策项 P4 未拍板 |

**② 反馈**

| 通道 | 状态 | 证据 |
|------|------|------|
| **截图** | ✅ shipped | `screenshot` / `screenshot-pane`，三平台；headless 无路径 |
| **视频** | ❌ **完全不存在** | 全仓无帧流 / 录制 / 编码依赖 |
| **流式结构化数据** | ~ **半成品** | `ui-deltas`（`UiDeltaBatch`/`UiDeltaEvent`/`UI_DELTA_MAX_EVENTS`）+ Observable Fleet epoch/sequence journal 已在；但形态是**批量轮询**，不是真正的推流 |
| **声音** | ❌ 不存在（双侧） | 见 F8 |

> **新增缺口（由北极星导出，不在原 F1–F8 内）**：
> **F9 视频/帧流** 与 **F10 真·推流**（把 `ui-deltas` 从 poll 升级为
> subscribe/push）。这两条原审计没覆盖，因为它们不是「人类能做而 agent
> 不能」——人类也不看视频流。它们是**自主驱动者特有的需求**：LLM 需要
> 低延迟、连续、可对齐时间轴的证据，而不是一次一张的快照。

---

## 0.1 结论先读

**设计是对的，落地只落了一半平台。**

`ui-input`（像素级 pointer / wheel / key）在 Unix 前端的实现**没有第二套
hit-test**：`apply_pointer_request`（`src/platform/adapters/unix/frontend/mod.rs:5940`）
合成真实 `PixelWindowEvent` 喂进人类鼠标同一个 `handle_pixel_event`
（`:6087`），press/move/release 成对、多击是真的 N 对、拖拽时末次 press
故意保持按住（`:5978-5987`）。`ui-snapshot` 的 bounds 覆盖面也确实厚，并带
focus / caret / anchor / selection，手势可机器验证。

**但**：

> ~~**「agent 能做人类能做的事」目前在 Linux/macOS 成立（那边人类 GUI 仍 `[~]`），
> 在 Windows 不成立（而那是 shipped 的人类平面）。**~~
>
> **2026-08-08 更新（P-win-input 完成，§1.4）**：这条倒挂已经填平——Windows 的
> `ui-input pointer/wheel/key` 现在真的派发，且走的是人类鼠标同一条
> `dispatch_window_event`。剩余不对等**不再是「平台没做」，而是「Win32 的原生
> `EDIT` 内部没有可合成的事件」**，并且每一处都返回点名到具体控件的 typed 失败，
> 不再静默。

外加一类**不是功能缺失、而是能力发现面在说谎**的问题（F3/F4）：agent 照
`--help` 学产品会被引到死命令，同时看不见活命令。对一个要被 agent 自主
驱动的产品，这比缺功能更伤——**自我迭代的前提是产品对自己的描述是真的**。

### 五条人类能力轴的现状

| 轴 | agent 侧现状 | 缺口 |
|----|-------------|------|
| **键盘** | `send-keys`（PTY 层）✅ + `ui-input key`（GUI 层，**三平台**，§1.4） | Windows 上原生 `EDIT` 内的字符输入不可合成（typed 拒绝，指向 `set-composer`）；`key` 未注册进 `operations.rs` |
| **鼠标** | `ui-input pointer/wheel`（GUI 层，**三平台**，§1.4）✅ | Windows 上原生 `EDIT` 内部（caret 定位/文本拖选）不可达；**PTY 层鼠标 `send-mouse` 声明但零实现**（F3） |
| **截图** | `screenshot` / `screenshot-pane` 三平台可用 ✅ | headless 无路径（依赖活客户端） |
| **结构化树** | `ui-snapshot` + `list-tab-tree` + `capture-pane` ✅ 厚；headless 已供 synthetic 几何（F2 已闭合，§1.2） | headless 供了几何但**不能派发** `ui-input`（F2b）；模态/系统菜单无 bounds（F6）；Win 缺 composer caret（F7） |
| **声音** | **完全不存在** | 见 F8——注意这是**双侧**缺口，不是对齐缺口 |

---

## 1. 发现表

| # | 面 | 证据 | 严重度 | 状态 |
|---|-----|------|--------|------|
| **F1** | `ui-input` **Windows 完全未实现** | ~~`remote_frontend.rs:8124-8144` 的 `REVIEW(macos → windows owner)`；`server_app.rs` 中继白名单不含 `ui-input`~~ | **P0** | **已闭合**（P-win-input，见 §1.4） |
| **F2** | headless 快照**无任何几何** | `src/server_app.rs:1937-1977`（`projection: "headless_server"`）：`layout` 只有硬编码 `composer:{visible:false,…}`（`:1951-1957`），`focus.surface` 硬编码 `null`（`:1959`），tabs 只有 id/title/pid/state/rows/cols，**无 bounds / selection / render** | **P0** | **已闭合**（P-headless，见 §1.2）；余 **F2b「headless 可派发 `ui-input`」** 另开，见 §2 |
| **F3** | `send-mouse` 是**活着的谎**：四处声明、零处 dispatch，却仍在 `extensions` 广告 | 声明 `src/commands.rs:61,734`、`src/control_authority.rs:251`、`src/client/mod.rs:5039,5146`；全仓无 dispatch 分支；Unix 落 `unix/frontend/mod.rs:4682-4695` 的 `unix_gui_unsupported`。且其坐标系是**终端 cell**（`-x col -y row`），本就够不到工具栏 | **P1** | 开 |
| **F4** | `ui-input` **不在 `--help`** | `src/client/mod.rs:5019-5049` 列了 `ui-snapshot`/`ui-action`/`ui-hello`/`ui-bootstrap`/`ui-deltas`/`send-mouse`，**唯独没有 `ui-input`** | **P1** | 开 |
| **F5** | typed 目录漂移 | `src/operations.rs` 只注册 **10** 个 `ui-action`（`:482-647` + `open-control-center`）；`src/frontend/ui_action_catalog.rs:29-88` 的 `SHARED_UI_ACTIONS` 有 **58** 个、`UNIX_ONLY_UI_ACTIONS`（`:109-121`）**11** 个；CLI help 广告约 40 个。另：`ui-input key` **未注册**（`operations.rs:749-756` 对非 pointer/wheel 返回 `Ok(None)`），`POINTER_PARAMETERS`（`:181-196`）只声明 `x`/`y`，`button`/`action`/`count`/`mods`/`delta-y`/`units`/`key` 全未声明 | **P1** | **shared 面已闭合**（见 §1.1）；`UNIX_ONLY` 11 个仍开 |
| **F6** | 模态框 / 系统菜单**只给 kind+actions，不给 bounds** | `snapshot_modal()`：`src/frontend/settings.rs:342-354`、`new_terminal.rs:181-189`、`instance_picker.rs:160-173`、`cwd_editor.rs:78-82`、`window_close.rs:46-48`、`close_confirmation.rs:39`；人类那侧**有**完整 hit-test：`unix/frontend/render.rs:413,541,766,809`。`system_menu_json`（`src/ui_snapshot.rs:127-149`）同样只给 id/label/enabled | **P2** | 开 |
| **F7** | Windows 快照缺 composer caret/anchor/selection | Unix 有（`unix/frontend/mod.rs:3025-3038`）；`remote_frontend.rs` 全文无 `"caret"`/`"anchor"`/`"draft_length"` | **P2** | **已关**（2026-08-09：ControlWindow 新增 `control_selection`(EM_GETSEL)，UTF-16→字符换算共享于 `text_selection`，Win 快照补顶层 `composer`；`tests/snapshot_key_parity.rs` 钉住不再回退。边界：原生 EDIT 无选区方向，anchor/caret 为序化端点） |
| **F8** | **声音：产品完全不存在**（双侧缺口） | vt100 内核**已解析** BEL：`third_party/vt100/src/perform.rs:44` → `callbacks.audible_bell()`、`:74` → `visual_bell()`；但 `third_party/vt100/src/callbacks.rs:6,9` 是空默认实现，**`src/` 全仓无人覆盖**。全仓无 `cpal`/`rodio`/`winmm`/`PlaySound`/`Beep` 依赖 | **P2**（产品决策） | 开（决策 D-2 已定，见 §3） |
| **F9** | **视频 / 帧流不存在** | 全仓无帧流、录制、编码依赖；只有一次一张的 `screenshot` | **P2**（北极星导出） | 开 |
| **F10** | 结构化反馈是**批量轮询**，非推流 | `ui-deltas`（`UiDeltaBatch`/`UiDeltaEvent`/`UI_DELTA_MAX_EVENTS`，`src/ui_bridge.rs`）+ `wait-*` 谓词；无 subscribe/push 通道 | **P2**（北极星导出） | 开 |

---

## 1.1 F5 落地记录（P-catalog，2026-08-08）

`OPERATION_CATALOG` 从 **32 条 / 10 个 `command: "ui-action"`** 变成
**77 条 / 55 个**（`+45`）。`SHARED_UI_ACTIONS` 的 58 个字符串现在**全部**能
经 `operation_for_args` 解析出 typed 身份：55 个 `ui-action` 操作 +
`open-control-center`（归在 `control-center.open` 名下）＝ 56 个身份，另两个
`toggle-tabs`、`open-instance` 是 alias。由
`operations::tests::every_shared_ui_action_has_a_typed_identity` 机器守住。

### 切分原则（为什么这么切，比代码重要）

**① 1 个动词 = 1 个 typed 操作，不做"合并成带参数的家族"。**

考虑过把 `settings-preset-*`(4) + `settings-theme-dark/light`(2) 合成一个
`ui.settings.preset(preset)`，把 `instance-picker-*`(5) 合成
`ui.instance_picker(action)`。**否决**，理由：这需要在
`control_dispatch.rs` **新造**一个带参数的动词，于是同一件事有了两条路
（旧动词 + 新动词），而目录任务的产品价值恰恰是**让目录等于控制面本身**。
F3（`send-mouse` 声明四处、零处 dispatch）教训就是"目录描述的东西必须真的
在那儿"；为了目录好看去改控制面，是把因果颠倒了。
**例外**：`select-server-tab` / `open-instance` 与 `tabs-toggle` /
`toggle-tabs` 在 dispatcher 里本来就是**同一条 match 臂**，那是同义词，
用 `aliases` 表达；这不是合并，是如实记录。

同理保留了 `settings-theme-dark` 与 `settings-preset-classic-night` 这对
**行为完全相同**的重复项——它们是产品对人类暴露的两个不同按钮，只在目录里
合并会让目录和产品对不上。要合并，先合并 dispatcher。

**② 模态内动词：全部注册，前置条件靠 `ui-snapshot` 运行时发现。**

`confirm`/`cancel`/`settings-apply`/`cwd-*`/`instance-picker-*`/
`tab-editor-*`/`keep-server-running`/`stop-server-and-exit` 只在特定模态下
有意义。**仍然注册**，理由是一条不对称：agent 可以用
`ui.tab.close`/`ui.settings.open`/`ui.cwd-editor.open` **打开**模态——如果
不注册 `confirm`/`cancel`，它就有了一个能进不能出的陷阱。
**暴露入口却隐藏出口，比两个都暴露更危险。**
前置条件不是靠隐藏表达，而是靠 `ui-snapshot` 的 `modal.kind` +
`modal.actions`（F6 会再补 bounds），调用时机不对会**报错**而不是静默无效。
`OperationSpec` 目前没有 `preconditions` 字段（那是 schema 变更），所以前置
条件写在 `operations.rs` 的分组注释里。

**③ `UNIX_ONLY_UI_ACTIONS`（11 个）不注册。**

`create`、`shell-*`(7)、`new-terminal-set-*`(3) 只在 Unix 存在。不注册因为：

- `OperationSpec::available` 是**一个 bool，没有平台轴**。填 `true` 会在
  Windows（**已 shipped 的人类平面**）说谎，正是 F3 那一类错误；
- `catalog_has_stable_unique_ids_and_all_classes` 里 `assert!(operation.available)`
  把"目录 = 已发布且到处可用的面"写死成了不变量，填 `false` 要先改这条不变量；
- `ui_action_catalog.rs:104-108` 已经写明"Prefer promoting create / shell-\*
  only when Windows exposes matching ui-action ids"——先注册 typed 身份等于
  抢跑那次提升。

**后续工作**：给 `OperationSpec` 加平台可用性轴（`available_on:
&["unix"]` 之类，需要 bump `OPERATION_CATALOG_SCHEMA_VERSION`），或先让
Windows 补齐这 11 个动词。二选一之前，agent 在两个平台上都只能"打开新终端
对话框"而不能驱动它——这是**已知且已记录**的洞，不是漂移。

**④ `events` 一律留空，不是偷懒。**

receipt↔事件关联（`src/client/mod.rs:4119-4138`）要求事件带上 `request_id`
或 `operation_id`。现在只有 `ui.tabs.*` 做到了（`set_tabs_visible(...,
UI_TABS_SHOW)` 把操作 id 传进去）。新动词没有这条接线，声明事件等于承诺一个
**永远不会到达**的关联。等 dispatcher 逐个打上 operation id 再补。

**⑤ `errors` 只写 dispatcher 真的会发的类型码。**

`close-tab`/`edit-tab` 一度声明了 `operation_target_not_found`——错的：两个
host 找不到目标时发的是 `IpcResponse::failure("can't find tab")`（**无类型**），
而 `pane.capture` 能声明它是因为 `control_dispatch.rs:1829` 确实
`typed_failure(..., "operation_target_not_found", ...)`。已改回，并加
`declared_error_identities_stay_within_the_typed_vocabulary` 守住。

**⑥ 顺手补齐的参数声明（F5 后半）。**

`select-tab`/`new-child`/`toggle-tree`/`composer-send` 原本声明
`NO_PARAMETERS`，导致 `validate_operation_args` 的"不接受额外参数"规则把
`-t` 一并拒了——**agent 只能操作当前 tab**。现已声明 `tab` 参数；同时把那条
arity 检查限制为**只对 `NO_PARAMETERS` 的动词**生效，否则任何带参数的 UI 动词
都不可达。

### 分类结果

| 家族 | typed id 前缀 | 动词数 | 备注 |
|------|--------------|--------|------|
| 窗口 | `ui.window.*` | 6 | `close-window` 是 Control：它只是**请求**关闭并弹确认框 |
| 字体 / 语言 | `ui.font.*` / `ui.locale.*` | 3 | |
| 剪贴板 | `terminal.copy-selection` | 1 | 与 `terminal.paste` 配对 |
| 标签生命周期 | `ui.tab.*` | 7 | `ui.tab.close` = **Destructive**（终结活 PTY + 回滚缓冲） |
| 设置对话框 | `ui.settings.*` | 14 | 全模态内 |
| 工作上下文 | `ui.cwd-editor.*` / `ui.new-terminal.open` | 6 | `--path` 声明为必填＝跨 host 契约 |
| 实例选择器 / server strip | `ui.instance-picker.*` / `ui.server-strip.select` | 7 | `open-instance` 作 alias |
| 模态收尾 | `ui.modal.*` / `ui.window-close.*` | 4 | `stop-server-and-exit` = **Destructive** |

### 机器守卫（新增）

| 测试 | 守什么 |
|------|--------|
| `every_shared_ui_action_has_a_typed_identity` | 目录覆盖**整个** shared 控制面 |
| `every_typed_ui_action_is_dispatchable_on_both_hosts` | 反向：typed 身份不得指向没人实现的动词（F3 类谎言） |
| `declared_error_identities_stay_within_the_typed_vocabulary` | 不声明 dispatcher 不发的错误码 |
| `closing_a_tab_is_classified_destructive` / `stopping_the_server_is_classified_destructive` | 不可逆操作必须带警示标签 |

> **不影响的东西（已实测）**：`script_catalog` 自动从 `OPERATION_CATALOG`
> 派生（`:185`），不用手改；`tests/rhai_migration.rs` pin 的
> `"70 catalog entries, 98 public names…"` 数的是
> `list-commands` 的 **CLI 命令名**（`scripts/rh/prd-alignment.rh:396-456`），
> 与操作数无关。**要手改的只有** `crates/agenterm-rh/src/shipped_surfaces.rs`。

---

## 1.2 F2 落地记录（P-headless，2026-08-08）

`projection: "headless_server"` 现在跑**同一份** `src/ui_geometry.rs`，从一个
**名义 viewport** 算出 bounds，并在快照根与 `layout` 两处标
`geometry_source: "synthetic"`；另两个投影（`embedded_gui` /
`replaceable_ui_client`）同步标 `"rendered"`，消费者无法把算出来的当成画出来的。

### 供了哪些几何

键名与 `embedded_gui` 完全一致（agent 的解析代码不按投影分叉）：

| 面 | 内容 |
|----|------|
| `layout.toolbar` | 逐按钮 bounds（tabs / new / control_center / settings / locale / font_decrease / font_increase），走共享的 `ui_snapshot::workspace_toolbar_snapshot_json`——Unix 那份已改为**委托**同一函数，两端不可能再漂 |
| `layout.sidebar` | bounds、`tree`、`resize_grip`、configured/effective width、`row_capacity` |
| `layout.server_strip` / `sidebar_clock` | strip bounds；**chips 留 `null`**（枚举活实例是 fleet 查询不是布局数学，归 `list-instances`） |
| `layout.terminal` | bounds、viewport_width、rows/cols、scrollbar track/thumb/offset |
| `layout.composer` | bounds + `input.bounds` + `send.bounds` |
| `layout.status_bar` | bounds、`tabs_recovery`、`cwd`（含 action）、archived proxy 槽 |
| `tabs[].bounds` / `.render` / `.actions` | 逐行 row/selection/node/expander/status/disclosure_hit/text/name/note；**仅活动行**给 `new_child`/`close`（与两个 GUI host 同规则）。另补 `depth`/`has_children`/`visible` |

**仍然是 `null` 的**：`focus.surface`、`modal`。headless 真的没有键盘焦点、没有
模态栈——合成它们是编故事，不是布局数学。这条界线是本叶的诚实性下限。

### 名义 viewport 怎么定

固定 **1280x800**，可用 `AGENTERM_HEADLESS_VIEWPORT=<w>x<h>` 覆盖。

- **不从 tab 的 rows/cols 反推**：那需要 cell 度量，cell 度量需要字体与 DPI，
  headless 两样都没有。捏一个出来只会让 bounds **看起来像量出来的**而实际并不更真，
  还会让几何随任意一次 `resize-pane` 抖动——CI 要的恰恰是可复现。
- 越界请求**拒绝而非 clamp**：给一个 40x40 的"窗口"静默返回 clamp 后的布局，会让
  一个根本放不下 chrome 的请求长得跟被采纳了一样。
- chrome 高度跟随**本平台已发布的 GUI host**（Win composer 104 / Unix 64，两端今天
  就不一致），并且**一并发布**在 `layout.viewport` 里，调用方不必猜。

### `ui-input` 可派发性结论：**不做，另立叶 F2b**

`ui-input` 的硬约束是「合成真实 `PixelWindowEvent` 喂进人类同一条
`handle_pixel_event`，**不得有第二套 hit-test**」（见下方「不要回退」小节）。
headless 既无窗口也无事件循环，`handle_pixel_event` 本身是 per-frontend 的。要让它
可派发只有两条路：

1. 在 `server_app.rs` 里另写一套命中测试 —— **正是被禁止的那件事**；
2. 把整个前端事件状态机（焦点、拖拽、选区、模态栈）上提到平台无关核心 —— 比"供几何"
   大一个量级，且应当与 P-win-input 一起规划。

而且 headless 没有 composer buffer、没有选区、没有模态——点击本该改变的状态一个都
不存在，勉强路由过去也只是**空转的表演**。

改为把拒绝**说实话**：`ui-input` 在 headless 下不再落 `server_command_unsupported`
（"does not implement"、不可重试），而是 `ui_client_unavailable` + `retryable=true`，
文案点名缺的是 GUI client。原来的措辞会让 agent 去找一个**已经存在**的功能，并把
「接一个 GUI 再试」这个正确动作标成不可重试——那是 F3 同一类的谎。

### 新增的 CI 覆盖

`tests/headless_ui_geometry.rs`（起真 server 进程、**不依赖活窗口**，3 个测试）：

| 测试 | 守什么 |
|------|--------|
| `headless_snapshot_publishes_labelled_synthetic_chrome_geometry` | **perceive**：自称 synthetic、viewport 报出处；**每个工具栏按钮的中心点只落在自己身上**（退化或重叠的 bounds 无法瞄准，发布它比不发布更糟）；composer input 在 composer 内、滚动条贴住终端右缘 |
| `selecting_a_tab_moves_the_published_row_actions_to_the_new_active_row` | **perceive→act→perceive 闭环**：读活动行 Close 位置 → `select-window` 换活动 tab → 再读，`actions` 跟着搬到新活动行、旧行回落 `null`、行与行不重叠、Close 仍在自己行内 |
| `ui_input_refuses_headless_dispatch_by_naming_the_missing_client` | 边界：朝发布出来的 Close 中心发 `ui-input pointer`，必须点名缺 GUI client，且**不得**说 "does not implement" |

另有 `src/ui_snapshot.rs` 3 个单测（viewport 解析拒绝而非 clamp、chrome 非退化且
在 viewport 内、行不重叠且仅活动行有 actions）。

> **它不证明什么**：不证明任何像素被画出来过。它验证的是命中/路由/派发逻辑，
> 真窗 smoke 不可省——二者分工等同于「布局数学单测 vs 像素回归」。

---

## 1.4 F1 落地记录（P-win-input，2026-08-08）

`ui-input pointer/wheel/key` 现在在 Windows 前端**真的派发**，并且**没有第二套
hit-test**。中继白名单已加 `ui-input`（`src/server_app.rs` 的
`relays_to_ui_client`）。

### 关键结构变更：抽出唯一的事件入口

`ControlWindowApplication::event` 那段巨大的 `match` 被提成自由函数
`dispatch_window_event(state: &mut RemoteWindowState, event) -> (consumed, redraw)`
（`remote_frontend.rs`）。人类消息泵和 `apply_pointer_request` 现在**调用同一个
函数**——不是「长得一样的两份」，是同一份。任何绕开它的命中测试都是 bug。

### 真正的难点：Windows 的像素归谁

原注释指出的问题比 composer 更广：**工具栏按钮、Send、所有对话框控件在
Windows 上都是原生子控件**。Win32 把落在子控件上的点击直接路由进那个子控件，
父窗口的事件处理器**根本收不到**。所以「合成 PointerButton 喂给窗口」这条路
在这些区域天然无效——静默无效，正是 F1 的病。

解法不是猜矩形，而是**问 OS**：新增 `ControlWindow::control_at(point)`
（`crates/agenterm-platform`），底层是 `ChildWindowFromPointEx`——**Win32 自己
路由真实点击时用的那个函数**，连 skip invisible/disabled 都一致。它不是「第二
份命中测试」，它就是第一份。然后分三种情况：

| 点落在 | 做什么 | 理由 |
|--------|--------|------|
| **自绘区域**（sidebar 树、终端、server strip、状态栏、滚动条、绘制的右键菜单） | 合成 `PointerMoved` + `PointerButton`，喂 `dispatch_window_event` | 与 Unix 完全同构 |
| **原生按钮**（工具栏 7 个、对话框按钮、Send） | 合成 `ControlWindowEvent::Command(id)` | 人类点原生 BUTTON，Win32 发的就是 `WM_COMMAND` → 这个事件。**替换的是「谁产生事件」，不是「事件怎么处理」**；处理它的仍是 `dispatch_window_event` 里那一条 `Command` 臂 |
| **原生 EDIT**（composer 输入、tab 编辑器 title/note、Settings 字体/字号、New terminal 三个字段、new-server 名字） | **typed 失败** | 点进 EDIT 的语义是「把 caret 放到某个文本偏移」，控件契约里**没有**这个事件，文本缓冲区归 OS。这是本叶诚实边界的核心 |

**捕获例外**：窗口持有 pointer capture 时，Win32 把所有鼠标消息给父窗口，
不管光标在哪个子控件上。所以 `control_at` 检查只在**未捕获**时生效——从自绘
区域按下的拖拽可以横穿 composer 带，和人类一模一样（实测通过）。

### 失败语义（typed，全部可理解、可行动）

| code | category | retryable | 何时 |
|------|----------|-----------|------|
| `ui_input_outside_window` | validation | false | 坐标在 client area 之外；消息里带上实测的 `WxH` |
| `ui_input_native_control_unreachable` | unsupported | false | 点落在原生控件上且无法投递：EDIT（附**替代命令**：`set-composer` / `set-setting terminal.font-size` …），或对原生按钮发 `--action release/move` |
| `ui_input_focus_native_control` | precondition | **true** | `key` 没被任何快捷键消费，而焦点在原生控件上；文案给出 `focus terminal` 这个正确动作，所以标可重试 |
| `operation_invalid_arguments` | validation | false | 解析失败，与 Unix 一致 |

为此 `process_client_command` 让 `ui-input` **绕开** `ui_client_command_failed`
这个漏斗——把「哪块面板吞了手势」压扁成一个通用 code，等于把刚补上的诚实性
再扔掉一次。

### 键盘：照抄真实的 Win32 消息泵

消息循环先把每个 `WM_KEYDOWN` 交给应用（`KeyPreview`），**只有没人消费时**才
让 `TranslateMessage` 生成字符。`apply_key_request` 复制这个顺序：先发
`KeyPreview`，被消费就结束（实测：composer/tab-editor 打开时 `--key Escape`
正常关闭对话框）；没被消费且焦点在窗口上才发 `TextInput`；没被消费而焦点在
原生控件上——那一跳是 OS 往控件缓冲区里塞字符，本进程做不到，于是报
`ui_input_focus_native_control`。

### 多击：**不**合成人类做不到的手势

Win32 只有 `WM_LBUTTONDOWN`(clicks=1) / `WM_LBUTTONDBLCLK`(clicks=2)，**没有
三击消息**。所以合成序列是 `1,2,1,2…`（`win32_click_index`）。

> **诚实标注**：`handle_terminal_double_click` 里有一条 `clicks >= 3` 的行选分支，
> 但 Windows 前端**任何真实输入都到不了它**——那是既存的死分支（不是本叶引入）。
> 因此 `ui-input pointer --count 3` 在 Windows 上不会行选。这是**平台既有差异**，
> 不是 `ui-input` 的洞；合成 `clicks: 3` 去点亮它，等于给 agent 一个人类做不到的
> 手势，正是本设计要防的漂移。要修就去修 Windows 的三击提升本身。

### 滚轮

`ControlWheelDelta::Pixels` 原本从 `match` 里漏下去（静默丢弃）；现在走同一个
`wheel_delta_units` 累加器，`--units pixels` 与 Unix 行为一致（实测：5 notch
→ offset +15；-240 px → -6）。滚轮**不**做原生控件拦截：子控件不消费滚轮时
`DefWindowProc` 会转给父窗口，所以它本来就到得了工作区处理器。

### 覆盖到什么程度（实测，真窗）

在 Windows Server 2022 上起真 server + 真 GUI，逐条驱动：

| 区域 | 结果 |
|------|------|
| 工具栏原生按钮（Tabs / Settings） | ✅ 像素点击收起/展开 sidebar；打开 Settings 模态 |
| sidebar 行内 `actions.new_child` | ✅ 建出子 tab 并打开内联编辑器（与人类点击同效） |
| 终端双击选词 | ✅ `selection` = row 11, col 12..99 |
| 终端拖拽选区（press→move→release） | ✅ start(16,6) → end(72,13) |
| 拖拽横穿 composer 带 | ✅（capture 例外生效） |
| 滚轮 lines / pixels | ✅ offset 15 / 9 |
| `key` 文本进终端 | ✅ `echo hi` 落进 PTY |
| `key Escape` 关内联编辑器 | ✅ |
| composer EDIT（press/move） | ⛔ 按设计 typed 拒绝，文案指向 `set-composer` |
| tab 编辑器 EDIT 上打字 | ⛔ 按设计 typed 拒绝，文案指向 `focus terminal` / `tab-editor-save` |
| 原生按钮 `--action release/move` | ⛔ typed 拒绝（原生按钮只报「完成的一次点击」） |
| 窗口外坐标 | ⛔ typed 拒绝，报出 `1164x721` |

### 测试覆盖 vs 只能人工验

**机器守住的**（`cargo test --lib`，4 个新测试）：

| 测试 | 守什么 |
|------|--------|
| `server_app::tests::ui_input_reaches_the_gui_client_only_when_one_is_attached` | 中继判定：有客户端才转发；无客户端保留 headless 那句更好的拒绝；服务器自有命令不得被转手 |
| `remote_frontend::tests::multi_click_replays_the_sequence_windows_itself_reports` | 序列是 `1,2,1,2` 且**永不出现 3** |
| `remote_frontend::tests::coordinates_outside_the_client_area_are_refused_by_name` | 越界拒绝 + 四条边界 + 消息里带实测尺寸 |
| `remote_frontend::tests::refusals_carry_a_category_the_client_can_read` | 每个 category 都能被 `error_category_from_wire` 解出（否则退化成 `Internal`，等于告诉 agent「我们坏了」而不是「这块不可像素寻址」） |

`tests/headless_ui_geometry.rs` 3 个仍绿（含
`ui_input_refuses_headless_dispatch_by_naming_the_missing_client`）。

**只能人工验的**：上表「实测，真窗」那一整块。原因是这条路径的入口需要一个活
的 `ControlWindow`（真 HWND + 真消息泵），`control_at` 更是直接问 OS；没有活窗口
就没有子控件可问。要机器化，只能等 F2b（把前端事件状态机上提到平台无关核心）。

### 仍然不行 / 已知边界

- **原生 EDIT 内部**：定位 caret、选中一段文本、鼠标拖选文本——做不到，原因如上。
  文本层有 `set-composer` / `set-setting`；**没有**文本层命令的是 New terminal 的
  三个字段和 new-server 名字，拒绝文案会如实说「this field has no text-level
  command yet」。
- **模态框内的按钮可以点，但 agent 瞄不准**：Windows 的 `snapshot_modal()` 只给
  kind + actions，不给 bounds（**F6**）。所以「Settings 的 Cancel 按钮」这类目标
  目前得靠 `ui-action settings-cancel`。F1 通了，F6 就成了下一个真瓶颈。
- **三击行选**：见上，Windows 平台既有差异。
- **headless 仍不能派发**：F2b 不变。

---

### 已经做对、**不要回退**的部分

- `ui-input` 走**同一条** `handle_pixel_event`，无第二套 hit-test
  （`archive/goal-cli-input-parity.md` T1 的硬性约束已被遵守）——**任何 Windows 移植
  必须保持这条**，不得为绕开 native `EDIT` composer 而另写一份命中测试。
- `ui-snapshot` 的 bounds 覆盖：工具栏逐按钮（`:6721-6731`）、tab 行 8 个子矩形
  （`:2852-2871`）、`actions.new_child`/`actions.close`（`:2770-2812`）、滚动条
  track/thumb、sidebar `resize_grip`、composer input、server strip 芯片
  **及右键菜单项 bounds**（`:2914-2955`，注释明说是为了让 agent 用
  `ui-input pointer` 点）。
- `WINDOWS_ONLY_UI_ACTIONS` 已归零（`src/frontend/ui_action_catalog.rs:98`），
  剩余不对称是**反向**的 11 个 Unix-only。

---

## 1.3 P-agent-tools 落地记录（2026-08-08）

> **架构原则（本节的全部要点）：超控智能体的工具表是 `OPERATION_CATALOG` 的
> 投影，是它唯一的能力来源。任何手写的第二张表都是漂移源，不是备份。**

`src/agent_tools.rs` 把 `OPERATION_CATALOG` 投影成 LLM 工具定义；
`agenterm-cli agent-tools` 是它的出口。它与
`src/script_catalog.rs:185`（脚本 API 从同一目录派生）**并列**，不另立真理：
两者读同一个 const，`operations.rs` 加一条，两边同时变宽，无需同步。

这一节存在的理由就是 F3/F4：`scripts/lua/lib/fleet.lua` 与
`scripts/qjs/lib/fleet.js` 是手工逐行互译的两份，`--help` 广告过一个**从未
实现**的 `send-mouse` 而藏起了唯一能用的 `ui-input`。**超控智能体不得再拿到
一张这样的表。**

### 投影长什么样

77 个 available 操作 → 77 个工具（Observe 13 / Control 60 / Destructive 4）。
每个工具携带：

| 字段 | 来源 | 为什么 LLM 需要它 |
|------|------|------------------|
| `name` | `id` 的机械 slug | 见下"工具名怎么定" |
| `title` | `id` 原样 | typed 身份就是标题 |
| `description` | **生成**，非撰写 | 见下"描述为什么是生成的" |
| `class` | `OperationClass` | 「先看后动」的规划依据 |
| `mutating` | `class != Observe` | 只读 / 变更的二分 |
| `approval` | `destructive` | **批准门**，见下 |
| `annotations` | class + destructive | MCP `readOnlyHint`/`destructiveHint`，camelCase 直接可转发 |
| `input_schema` | `parameters` | JSON Schema，见下"参数映射" |
| `invocation` | `id`/`script_surface`/`command`/`action`/`aliases` | 真的怎么调 |
| `errors` | `errors` | 已声明的 typed 失败词表 |
| `events` | `events` | 可等待的关联事件 |
| `available` / `unavailable_reason` | `available` | 诚实性，见下 |

### 参数 → JSON Schema

`value_type` 的 7 个取值全部有显式映射：`number`/`integer` 直通，
`uint32` 补 `0..=4294967295`，`uint64` 只补下界（`u64::MAX` 超出 JSON 数字能
无损往返的范围，声明上界等于撒谎），`string`/`session_name` 是字符串，
`stable_tab_id` 额外带 `pattern: "^@[0-9]+$"`——否则 agent 会把 tab 标题传进去。

**一个真实的坑**：`OperationParameterSpec::{minimum, maximum}` 在数值类型上是
**取值边界**，在字符串类型上是**字节长度边界**（`note` 是 `0..=4096` 字节，
`client_id` 是 `1..=128`）。把两者一律映射成 JSON Schema 的 `minimum`，等于
告诉模型「备注必须是个数字」。所以字符串走 `minLength`/`maxLength`，由
`parameter_bounds_and_requiredness_reach_the_schema` 断言 `note` **没有**
`maximum` 字段。

`required` 数组从 `parameter.required` 派生；`additionalProperties: false`
让 schema 校验器替 agent 挡住拼错的参数名。

### 批准门怎么表达

```
"approval": { "required": true, "gate": "explicit_human_approval",
              "reason": "destructive_operation" }
```

`gate` 是枚举而不是 bool，因为 `plan/design-cc-hyper-control-agent.md` §1.2 要求
的是**显式批准门**，而 CC 未来还会有别的门（配额、租约）；一个 bool 只够表达
"要不要问"，不够表达"问谁"。当前 4 个门：`ui.tab.close`、
`ui.window-close.stop-server-and-exit`、`server.kill`、`workspace.shutdown`。

守卫 `destructive_operations_carry_an_explicit_approval_gate` 断言的是**集合
相等**——被门控的工具集恰好等于目录里 `destructive` 的集合，不多不少。少一个是
静默越权，多一个是把 `ui.tabs.show` 也拿去烦人类。

同时保留 `classification_only: true` / `authorization_policy: false`
（与 `protocol-info` 对原始目录的口径一致）：投影标注**哪些**调用需要门，
**谁来开门**仍归操作员，投影不是策略引擎。

### 诚实：不可用能力不得进表

默认投影**排除** `available: false`；`--include-unavailable` 才带出来，并且带
`available: false` + `unavailable_reason: "operation_unavailable"`，描述文本里
也写死 `UNAVAILABLE: ... do not call it.`（模型读的是散文，不只是字段）。

目录当前**没有** unavailable 条目，所以这条守卫如果只跑真目录会**永远空转**。
`unavailable_operations_never_reach_the_default_table` 因此对一条**合成**
`OperationSpec` 断言，`project_catalog` 也就设计成接受任意 slice。

### 工具名怎么定：slug，不是 `id`，也不是 CLI 形式

Anthropic 与 OpenAI 的工具名都受 `^[a-zA-Z0-9_-]{1,64}$` 约束，
**带点的 `ui.tab.close` 直接非法**。所以 `name` 是机械 slug
`agenterm_ui_tab_close`（前缀让它在混合工具表里自我标识），而 typed 身份原样
留在 `invocation.operation_id`——投影因此是**可逆**的。
`tool_names_are_unique_and_wire_legal` 守住唯一性与合法性；
规则本身写成生产函数 `tool_name_is_wire_legal`，不是测试里的一段字面量。

**不用 CLI 形式**，也**不生成 argv 模板**：目录声明的是参数**名**，不是它们的
CLI 拼法（`tab` 是 `-t`，`instance` 是位置参数，`select-server-tab` 的实例名
不带旗标）。生成一条命令行等于把目录里没有的事实编出来——**正是 F3 那类
错误**。给的是 `operation_id` 和 `script_surface`，两者都逐字接受目录声明的
参数名。

同理，`title` 不再拼 `command + action`：`control-center.open` 的
`command` 是 `control-center`，而它的 action `open-control-center` 属于
`ui-action`，拼起来会读成一条**不存在**的命令行。

### 描述为什么是生成的

`OperationSpec` 没有散文字段。加一个就意味着每条操作要人写一句话——那是一张
**穿着派生表外衣的手写表**，第 78 条操作加进来时它就开始漂移。所以
`description_for` 从 class / 身份 / `result_type` / `errors` / `events` 机械
拼装。代价是文风呆板，收益是新增操作**零散文成本**。

### 为什么是运行时函数 + CLI，不是构建期 codegen

目录是 Rust `const`，投影就是纯函数，运行时算它是零成本，且**不可能**与目录
不同步。codegen 会产出一个签入仓库的产物，那个产物可以被忘记重新生成——把
"永不手写"降级成"记得跑脚本"。

CLI **不联服务器**：目录是二进制的属性，不是运行中 server 的属性。agent 必须
能在还没有任何东西可对话之前，先知道自己**可以**做什么。

### 为什么没有接进 `mcp_stdio` 的 `tools/list`

`src/mcp_stdio.rs:637` 现在手写着唯一一个工具 `agenterm_wait`，
服务器自我描述为 `Read-only AgenTerm Fleet bridge`。把 60 个 Control 工具挂上
去，而 `tools/call` 侧**没有**对应的派发与批准门执行，就是又造一次 F3——
**表里有、按下去没反应**。`agent_tool_catalog_mcp_json()` 已经按 MCP 的
`inputSchema`/`annotations` 键名产出，等执行侧就位时直接接上即可，届时不需要
重新手抄一遍键名。

### 机器守卫

| 测试 | 守什么 |
|------|--------|
| **`every_available_operation_is_projected`** | **本节的核心**：目录里每个 available 操作都在工具表里，且工具数**恰好等于** available 数。手写表会在下一条目录条目上失败 |
| `no_tool_exists_without_a_catalog_entry` | 反向：每个工具都有目录条目撑着，且 `errors`/`events`/`result_type`/`class`/`since` 原样透传 |
| `unavailable_operations_never_reach_the_default_table` | 不可用能力不得被选中（对合成 spec 断言，不空转） |
| `destructive_operations_carry_an_explicit_approval_gate` | 门控集合 == 目录 destructive 集合 |
| `observation_and_mutation_stay_distinguishable` | Observe/Control/Destructive 三分穿过投影 |
| `every_declared_value_type_has_a_schema_mapping` | 目录新增 `value_type` 时**必须**来改映射，而不是让投影猜一个 `string` 混过去 |
| `parameter_bounds_and_requiredness_reach_the_schema` | 边界/必填/字符串长度 vs 数值边界的区分 |
| `tool_names_are_unique_and_wire_legal` | slug 唯一且符合 `^[a-zA-Z0-9_-]{1,64}$` |
| `document_and_mcp_shapes_are_derived_from_the_same_tools` | 两种输出形状同源 |
| `declared_failures_are_visible_to_the_model` | typed 失败词表进入模型读得到的散文 |

### 顺带修的 / 需要知道的

- `agent-tools` 是**新增 CLI 命令**，所以按 §2 的提醒补了
  `prd/PRD_02_15_command_line.md` 的条目——`scripts/rh/prd-alignment.rh:459`
  要求每个 `list-commands` 公共名在 PRD 或其 linked 详情文档里被提到，
  fail-closed。`OPERATION_CATALOG` 增删不触发这条，新增命令会。
- 同时补进了 `agenterm-cli --help`（F4 的教训：能力不进 help 等于不存在）。

---

## 2. 排序建议（成本 / 依赖 / 泳道）

| 序 | 叶 | 内容 | 成本 | 泳道 / 热域 | 依赖 |
|----|-----|------|------|------------|------|
| 1 | **P-honest** | F3（**实现** `send-mouse`，见 D-3）+ F4（`ui-input` 进 `--help`） | **小** | `src/control_dispatch.rs`（共享，两平台一次到位）、`src/client/mod.rs` | 无 |
| 2 | ~~**P-catalog**~~ | F5：`operations.rs` 登记 `ui-input key`/`send-mouse` + 补全参数声明 + **shared `ui-action` 全量登记** | 小–中 | `src/operations.rs`、`crates/agenterm-rh/src/shipped_surfaces.rs` | 1 — **shared 面已完成，见 §1.1**；余 `UNIX_ONLY` 11 个待平台可用性轴 |
| 3 | ~~**P-headless**~~ | F2：headless 供 synthetic 几何（D-1 已定），解锁 `ui-input` 的 CI 覆盖 | **中** | `server_app.rs` + `ui_geometry` 复用 | 1 — **已完成，见 §1.2** |
| 4 | ~~**P-win-input**~~ | F1：Windows `ui-input` 移植 + `server_app.rs` 中继白名单 | **大** | Win-UX（`remote_frontend*`） | 3 — **已完成，见 §1.4** |
| 5 | **P-modal** | F6 + F7：模态/菜单 bounds、Win composer caret | 中 | 两端 | 4 — **就绪，且已升格为下一个真瓶颈**：Windows 现在能用像素点原生按钮了，但模态里的按钮**没有 bounds 可瞄**（§1.4 末） |
| 6 | **P-bell** | F8：BEL 事件化（D-2 已定，只做事件，不做音频） | 小 | vt100 callbacks + event journal | 无 |
| 7 | **P-headless-act** | **F2b（新）**：headless 真的能派发 `ui-input`。前提是把前端事件状态机（焦点/拖拽/选区/模态栈）上提到平台无关核心，**不得**在 `server_app.rs` 另写命中测试 | **大** | 与 P-win-input 同一片重构 | 4（应与之合并规划） |
| — | **P-stream** | F9 + F10：视频/帧流、推流 | 大 | 待定 | **决策 D-4** |

**排序变更说明**：P-headless 提到 P-win-input **之前**——因为 Windows 移植是
本表最贵的一叶，而当时 `ui-input` **零行为测试**；先有 CI 闭环再动大改，
否则移植完也无法证明它对。

> **2026-08-08 更新**：P-headless 已完成，`tests/headless_ui_geometry.rs` 提供了
> 第一条不依赖活窗口的 perceive→act 闭环覆盖。P-win-input 的前置条件因此解除。
> 新出的 **P-headless-act（F2b）** 排在 P-win-input 之后而非之前：两者要动的是
> **同一块**——把 per-frontend 的事件状态机提取成平台无关核心。先做 Windows 移植
> 会暴露这块的真实形状，再做提取才不会提取错。

> **2026-08-08 再更新（P-win-input 完成后）**：那块的形状现在看得见了。Windows
> 侧已经把「事件 → 行为」收敛成**一个**函数 `dispatch_window_event(state, event)`
> （Unix 侧对应 `handle_pixel_event`）。F2b 要做的提取，本质就是让这两个函数收敛
> 到一个平台无关的状态机上；`ControlWindowEvent` 与 `PixelWindowEvent` 的差异
> （`clicks` 由 OS 计算 vs 由应用提升、原生子控件 vs 全自绘）**是真实的平台差异，
> 不是可以抹平的偶然**——提取时必须保留它们，否则又会造出一个人类到不了的状态面。

**关键提醒（来自 `archive/goal-cli-input-parity.md` T2）**：动 `src/operations.rs` 加公开
命令时，`tests/rhai_migration.rs` 的
`prd_alignment_task_matches_public_catalogs_and_fails_closed` 会 fail-closed，
**必须同步更新 `prd/` 目录与该测试里 pin 的计数串**。

> **2026-08-08 更正（实测）**：这条只对**新增 CLI 命令**（`COMMAND_CATALOG`）
> 成立。该测试 pin 的 `"70 catalog entries, 98 public names…"` 数的是
> `list-commands` 输出的命令名与别名（`scripts/rh/prd-alignment.rh:396-456`），
> **与 `OPERATION_CATALOG` 条目数无关**。P-catalog 新增 45 个操作、零个命令，
> 计数串无需变动。

---

## 3. 决策记录（2026-08-08，用户授权 agent 自主拍板）

| ID | 题 | **决定** | 理由 |
|----|-----|---------|------|
| **D-1** ✅ 已落地（§1.2） | headless 是否供几何？ | **供，但标注为 synthetic** | 布局数学在共享的 `src/ui_geometry.rs`（precision-audit #10/#11/#12 已把两端统一到它），**是纯函数**：给定 viewport + cell 度量 + tab 数即可算 bounds，不需要活窗口。做法：headless 接受一个名义 viewport，跑**同一份** `ui_geometry`，快照里标 `geometry_source:"synthetic"`。收益：perceive→act 闭环终于能进 CI（现状 `ui-input` **零行为测试**）。诚实性：它验证的是**命中/路由/派发逻辑**，不是渲染像素——真窗 smoke 仍不可省，二者分工与「布局数学单测 vs 像素回归」同构 |
| **D-2** | BEL 落地形态？ | **只做事件化；视觉后补；声音不做** | 事件化进 Observable Fleet 序列后 agent 可 `wait`，且它是视觉/声音两种呈现的**共同前置**（都消费同一事件），杠杆最高。真音频要引依赖并吃 4 MiB GUI 体积预算（`PRD_02_17`），换来的是一个多数终端默认关掉的功能——不值。 |
| **D-3** | `send-mouse` 删除还是实现？ | **实现** | 三条理由：① 缺口真实——人类能在 `htop`/`lazygit`/`k9s` 里点，agent 不能；② **成本已被 C5 打掉**——编码 `mouse_report_bytes`/`mouse_code_with_modifiers` 已在 `agenterm_platform`，协商态 `mouse_protocol_mode/encoding` 已在 `control_dispatch.rs:190-205` 被读，写入只是 `.send(&bytes)`；③ **它在 `control_dispatch.rs` 这个共享派发里，一次实现两个平台都有**——不像 `ui-input` 是 per-frontend 的。所以在 Windows `ui-input` 移植（P-win-input，成本大）落地之前，**这是给 Windows agent 补上鼠标能力的最便宜路径**。删掉则永久放弃 PTY 层这条轴 |

> **D-3 的层次说明**（澄清一个容易混的点）：人类在全屏 TUI 里点鼠标走
> **PTY 层**（应用协商 `?1002h;?1006h` 后收到上报，坐标是**单元格**）；
> 点工具栏/标签走 **GUI 层**（坐标是**像素**）。`send-mouse`（cell）与
> `ui-input pointer`（pixel）**不是竞争关系，是两层**——原先以为前者被后者
> 取代是误判。

### 仍需人工拍板

| ID | 题 | 为什么不是 agent 能定的 |
|----|-----|----------------------|
| **P4** | computer-use（L-CU）是否立项、归口哪个 PRD、首发平台 | 高危能力面（可用于横向移动）+ 新可执行体 + 授权模型，属产品与安全边界 |
| **D-4** | 视频/帧流（F9）与推流（F10）是否进版本 | 新增依赖、体积预算、带宽/存储语义；且需先明确消费者是谁 |

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
