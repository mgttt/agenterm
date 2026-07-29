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
- [ ] 黑盒：`ui-snapshot` 几何测试覆盖 180/250/480 px 侧栏与 disclosure 命中

### P1 — 侧栏交互深度

- [x] 侧栏滚动：`sidebar_scroll_offset`、wheel/drag、`layout.sidebar.scrollbar`
- [x] 侧栏内联 tab 编辑：`TreeRowMode::Editing`、`tab_editor.focus`、`tab-editor-save/cancel`（禁止 Composer 借用）
- [ ] New 终端对话框：`kind: "new-terminal"` + shell/初始命令/proxy + `ui-action` create 路径
- [ ] `ui-action` 窗口控制：`keep-server-running`、`stop-server-and-exit`、`window-minimize/maximize/restore/resize`

### P2 — 设置、菜单、选择

- [ ] Settings：可编辑字体族；snapshot `settings` + `theme_options`；`settings-cancel`
- [ ] `system_menu.copy`/`paste` enabled 状态与快捷键语义
- [ ] 每 tab `selection`；`terminal_interaction` 对象形状与 Win 一致

### P3 — 渲染与字体

- [ ] TTF/FreeType 路径，使 `terminal_font_family` 影响像素而非仅行距
- [ ] 树连接器绘制（Win `paint_tabs` 连续分支线）

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
