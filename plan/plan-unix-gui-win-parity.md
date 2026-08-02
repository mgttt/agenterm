# Unix GUI ↔ Windows 对齐工作地图

状态：执行中  
主题：**嵌入式 Unix GUI（`unix_app`）与 Windows 可替换 UI 客户端（`remote_win_app`）在可见行为与 `ui-snapshot` 上对齐**  
本文是执行地图，不是产品宪法；已交付能力同步进 [`prd/PRD_02_06_human_workspace.md`](prd/PRD_02_06_human_workspace.md)。

对照基准：Windows `remote_win_app` + 共享 `ui_geometry` / `control_dispatch` / `ui_bridge` 契约。

## 依赖与集成边界

```text
ui_bridge schema + ui_geometry
        │
        ├─► P0 快照契约对齐（schema_version、layout 嵌套、per-tab render/actions）
        ├─► P0 侧栏树几何（paint / hit-test / bounds 共用 tree_row_geometry_for_mode）
        ├─► P1 侧栏滚动（offset + scrollbar + snapshot）
        ├─► P1 侧栏内联 tab 编辑（非 Composer 借用）
        ├─► P1 New 终端创建对话框
        ├─► P2 Settings 字体族可编辑 + snapshot.settings
        ├─► P2 系统菜单 / 剪贴板控制面对齐
        ├─► P2 终端选择 snapshot + 绘制世代
        └─► P3 TTF/主机字体（替代 bitmap-8x8）
```

`plan/plan-multiplatform-gui.md` 记录跨平台交付里程碑；**本文件只跟踪 Win 对齐差距**，避免与 CPMP 进度混淆。

## 进度树

Legend：`[x]` 已对齐，`[~]` 部分，`[ ]` 未做。

### 共享基础（Win + Unix）

- [x] `ui_geometry` 工作区布局（sidebar / toolbar / terminal / composer / status）
- [x] `control_dispatch` + 共享 workspace / tab lifecycle 命令
- [x] `ui_clipboard` 终端/Composer 粘贴规范化
- [x] `tab_tree::visible_tree_rows` 折叠过滤
- [x] 侧栏行高 `TAB_HEIGHT`（36px）与 Win 树几何一致
- [x] 滚动条几何、`scrollbar_hit_test`、单元格映射、滚轮增量（Unix 输入路径）
- [x] Composer Send + Ctrl+C/X/V；Settings 行距；模态时隐藏工具栏
- [x] 终端选择手势 + 剪贴板复制；截图 PNG IPC；Linux GUI 库捆绑

### P0 — 快照与树几何（当前循环）

- [x] 侧栏树行几何（paint、disclosure 命中、`tabs[].bounds`/`render`/`actions`）
- [x] **ui-snapshot 契约**：`schema_version`、`projection`、`settings`、`layout.terminal` 嵌套 scrollbar、`locale`/`feedback` 等与 Win 形状对齐（`embedded_gui`）
- [x] 黑盒：`ui-snapshot` 几何测试覆盖 180/250/480 px 侧栏与 disclosure 命中

### P1 — 侧栏交互深度

- [x] 侧栏滚动：`sidebar_scroll_offset`、wheel/drag、`layout.sidebar.scrollbar`
- [x] 侧栏内联 tab 编辑：`TreeRowMode::Editing`、`tab_editor.focus`、`tab-editor-save/cancel`（禁止 Composer 借用）
- [x] New 终端对话框：`kind: "new-terminal"` + shell/初始命令/proxy + `ui-action` create 路径
- [x] `ui-action` 窗口控制：`keep-server-running`、`stop-server-and-exit`、`window-minimize/maximize/restore/resize`

### P2 — 设置、菜单、选择

- [x] Settings：可编辑字体族；snapshot `settings` + `theme_options`；`settings-cancel`
- [x] `system_menu.copy`/`paste` enabled 状态与快捷键语义
- [x] 每 tab `selection`；`terminal_interaction` 对象形状与 Win 一致

### P3 — 渲染与字体

- [ ] TTF/FreeType 路径，使 `terminal_font_family` 影响像素而非仅行距
- [x] 树连接器绘制（Win `paint_tabs` 连续分支线）

## 结构重构：Platform 与 Frontend 职责剥离（新增）

### O0 行为一致性目标

- [x] 明确跨平台统一UX清单（点击、滚轮、选区、焦点、窗口恢复）
- [x] 建立统一验收：`Given/When/Then` + 对应证据脚本
- [ ] 明确平台能力缺口：每个OS的 `Unsupported/Failed` 和回退策略

```text
O1 并发树（并行）
├─ O1A 能力注入层（Owner: Platform）[x]
│  ├─ 目标：保持入口分发在平台能力上，不在入口层承载UI策略
│  ├─ 交付：能力路由在 `src/frontend/mod.rs` + `platform::frontend_host`（曾写
│  │     `services/frontend.rs`；该路径已删除，见 `plan/ARCHITECTURE.md`）
│  ├─ 交付：`agenterm` 启动参数解析进入 `src/frontend/mod.rs` 共享策略器（Windows/Unix 同步）
│  ├─ 约束：`run_gui_entry` 与 `request_gui_wake` 返回码一致，启动失败原因可归并为统一证据
│  └─ 风险：handoff 与服务端接管回退回归
├─ O1B 窗口生命周期（Owner: Product-UI）
│  ├─ 目标：统一窗口恢复/焦点行为的语义形状
│  ├─ 交付：`focus/query/state` 的产品行为一致，能力不足返回清晰失败而非静默降级
│  ├─ 约束：Windows hide/show 与 Unix 激活策略在契约上可比较
│  └─ 风险：macOS 重开时序与焦点事件的竞态
└─ O1C 输入语义层（Owner: Product-UI）
   ├─ 目标：统一坐标、点击、滚轮、选区、拖拽与快捷键的产品语义
   ├─ 交付：同一场景在两套前端下产生同字段 `ui-snapshot` 变更
   ├─ 约束：平台特性只保留在 `agenterm-platform` 能力返回，不进产品策略
   └─ 风险：`TerminalPoint`、`selection_generation` 与自动滚动边界
```

### O2 汇聚后验收

- [x] 所有交互分支都以 `agenterm-platform` 能力结果驱动，不再通过 OS cfg 做策略选择。2026-08-02 复核：`src` 非平台目录使用 `rg` 全量扫描后未发现 `is_windows_host`、`is_unix_host`、`platform_kind`、`#\[cfg(windows)]`、`#\[cfg(unix)]`，交互分支差异统一在能力证据和宿主差异记录，不直接进入非平台产品代码分支。
- [x] `ui-snapshot`、`terminal_state`、`selection` 与可见行为在同场景可回放比对
- [x] 文档同步：跨平台UX能力缺口矩阵同步到 PRD_02_20 与 plan

### O3 验证循环（Rhai 首选）

- [x] `scripts/rhai/platform-ux-parity-smoke.rhai` 覆盖 Windows startup+remote-ui 与 Unix startup+unix-frontend
- [x] 三平台统一入口与 `--list-evidence` 已通 (`platform-ux-parity-smoke`, `platform-ux-parity-smoke-linux`, `platform-ux-parity-smoke-macos`)
- [~] 按 3 平台并发执行实体验收：Windows 与 Linux/macOS 的入口已执行并聚合到 `plan/platform-ux-parity-evidence-matrix.md`，Linux/macOS 当前为环境边界失败（`platform-binary-missing`）不归为回归脚本缺陷
- [x] 形成“分支-场景-证据”表结构：见 `plan/platform-ux-parity-evidence-matrix.md`

#### O3 最近执行分派（按 owner）

- Owner: Platform（P2）
  - `platform-ux-parity-smoke-linux -- --emit-matrix`  
    - `run_id: 1785678831415-84776`
    - 结果：`platform_gui_missing`，`infra/platform-binary-missing`
    - 处置：补齐 Linux GUI 二进制可执行链路后补跑
- Owner: Platform（P2）
  - `platform-ux-parity-smoke-macos -- --emit-matrix`  
    - `run_id: 1785678837047-34972`
    - 结果：`platform_gui_missing`，`infra/platform-binary-missing`
    - 处置：补齐 macOS GUI 二进制可执行链路后补跑
- Owner: UX（P1）
  - `platform-ux-parity-smoke -- --emit-matrix`  
    - `run_id: 1785678683554-260172`
    - 结果：Windows 侧 `ux-parity.remote-ui.replaceable-client`、`ux-parity.remote-ui.selection`、`ux-parity.window-focus-contract` 为 `Supported`
    - 归因：`windows-only-contract` 与 `Supported` 条目已进入上限验证

### 并发执行与收敛回路

```text
执行循环：
1) 并行提案（O1A/O1B/O1C）
2) 同步汇聚：统一能力缺口文档 + 统一证据标签
3) Rhai 回归循环：platform-ux-parity-smoke 全场景
4) 复测：`ui-snapshot` 与 PNG 证据差异归一化
```

### O1D 产品 UI/UX 物理归属（2026-08-03）

```text
目标：UI/UX 语义住在 src/frontend/，platform 只保留能力契约与主机 adapter
├─ D1 [x] src/platform/toolbar.rs → src/frontend/toolbar.rs
├─ D1 [x] src/platform/mod.rs `action` → src/frontend/action.rs
├─ D1 [x] src/platform/window.rs → src/frontend/window.rs
├─ D1 [x] src/platform/control_center.rs → src/frontend/control_center.rs
├─ D2 [x] src/platform/mod.rs 产品策略拆 policy/{input,paths,control_center,runtime,test_fixtures}
├─ D3 [~] Win remote vs Unix embedded 共享交互管线（selection phase、focus 导航与 wheel 累积已收为 crate 共享；modal/focus 规则仍待收敛）
└─ D4 [ ] 每条可见差进入 evidence matrix；无 adapter 内产品 if
```

下一波并发：D3 与 D4 串行；D4 依赖 D3 落地。

> 现行结构 SSOT：`plan/ARCHITECTURE.md`。  
> 历史边界叙事（非权威）：`plan/platform-ui-ux-boundary-tree.md`。

## 验收证据

| 能力 | 证据 |
|------|------|
| 几何 | `ui_geometry` 单元测试 + `ui-snapshot` 字段黑盒 |
| 交互 | Linux `DISPLAY` 下 CLI：`ui-snapshot`、`ui-action`、截图 |
| Win 不回退 | `windows-latest` CI + `remote_win_app` 测试 |

## 非目标

- 像素级克隆 Win32 GDI 字体抗锯齿
- 把完整 `tests/*.ps1` 搬到 Linux
- tmux/RMUX 全兼容
