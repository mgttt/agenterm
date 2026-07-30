# AgenTerm v0.2.0 公开计划（骨架）

状态：规划中  
前置：**v0.1.10 Rhai 自举收尾**、**Unix GUI ↔ Win 对齐合入 `main`**（含 PR #15）、主工作区 UI 调顺  
工作主题：**Fleet Hub** — 工具栏 Settings 左侧新入口打开的二级枢纽窗口  
版本定位：在安静主界面之上，集中呈现舰队驾驶舱、工作流、PluginHub、InfoHub 四条能力线，全部绑定同一 server 权威与公开控制面，不在 Hub 内复制舰队状态。

本文是版本执行计划与决策记录，不是产品事实。接受的产品范围同步进 owning `prd/PRD_*.md`；
灵感条目见 [`PRD_02_19_inspiration_and_future_vision.md`](prd/PRD_02_19_inspiration_and_future_vision.md)。

## 产品 outcome（一棵树）

```text
Fleet Hub（二级窗口 / overlay，工具栏 Hub 按钮，Settings 左侧）
├─ [ ] 壳层：入口、焦点、模态契约、ui-snapshot / ui-action、Win + Unix 几何
├─ [ ] Cockpit（驾驶舱）— 只读舰队仪表盘
├─ [ ] Workflows — 工作流与管道入口（持久化图编排后挂）
├─ [ ] PluginHub — 可选组件 / sidecar 发现（安装走 softmgr，GUI 启动不下载）
└─ [ ] InfoHub — 外部信号订阅与路由（Composer 草稿，不自动执行）
```

## 依赖与集成边界

```text
ui_geometry toolbar + ui_bridge snapshot
        │
        ├─► Hub 按钮几何（New 与 Settings 之间）与 compact 模式
        ├─► Hub overlay / 第二窗口投影（首选 overlay，见 PRD）
        ├─► Cockpit ← list-windows / journal / ui-snapshot / inspect（只读）
        ├─► Workflows ← script task catalog + 未来 flow runtime（C1）
        ├─► PluginHub ← softmgr + 签名包清单（D1/D2）
        └─► InfoHub ← 订阅连接器 + 谓词（E1，非媒体 App）
```

Unix Win 对齐地图：[`plan-unix-gui-win-parity.md`](plan-unix-gui-win-parity.md)  
跨平台 GUI：[`plan-multiplatform-gui.md`](plan-multiplatform-gui.md)

## v0.2.0 建议分期

### Phase A — 壳 + Cockpit（本版最小可发布）

- [ ] 工具栏 **Hub** 按钮（Settings 左侧）；模态时隐藏主工具栏
- [ ] Hub overlay 框架：四 tab 导航 + 空状态占位
- [ ] **Cockpit**：活跃/ dead 摘要、journal epoch/sequence、快捷 inspect / select-tab
- [ ] `ui-snapshot`：`modal.kind: "fleet-hub"`（或等价）+ `fleet_hub.active_tab`
- [ ] `ui-action`：`open-fleet-hub`、`close-fleet-hub`、`fleet-hub-tab`（cockpit|workflows|plugin-hub|info-hub）
- [ ] Rhai smoke：打开 Hub、切 tab、Cockpit 字段黑盒（扩 `remote-ui-smoke` 或嵌入式等价）

### Phase B — 三条 Hub 内容（可拉伸进 v0.2.0 或 v0.2.1）

- [ ] **Workflows**：已注册 Rhai task / 未来 MCP flow 只读目录；图编排 UI 占位
- [ ] **PluginHub**：manifest 浏览、「通过 softmgr 安装」占位；遵守 D3（启动不下载）
- [ ] **InfoHub**：订阅源注册 UI；过滤 → 通知 → Composer 草稿路径

### 明确非目标（v0.2.0）

- 像素级第二窗口多屏方案（可 v0.2.x 后期）
- PluginHub / InfoHub 的远程交易、自动下单、静默安装
- Hub 内第二套 PTY 或独立 server 状态
- TTF/FreeType（留在 Unix parity P3 或 v0.2.x 并行）

## 验收证据

| 能力 | 证据 |
|------|------|
| 入口与几何 | `ui_geometry` 单元测试 + `layout.toolbar.hub` snapshot |
| Cockpit | CLI `ui-snapshot` 字段 + PNG |
| 交互 | `ui-action` 黑盒 + Windows CI smoke |
| 不回退 | 主工作区 terminal/composer 旅程仍绿 |

## 风险与门

- **共享热点**：`ui_geometry.rs`、`remote_win_app`、`unix_app/mod.rs` — Hub 几何与 Win 对齐同一补丁序列化
- **命名**：对外统一 **PluginHub** / **InfoHub**（不用「市场」作为主称谓）
- **quiet surface**：Hub 为显式二级入口，不增加日常工具栏噪音

## 决策记录

| 日期 | 决策 |
|------|------|
| 2026-07-30 | v0.2.0 主题定为 Fleet Hub 四 tab；插件与信息面称 PluginHub / InfoHub |
| 2026-07-30 | 第一刀采用 overlay（对齐 Settings），非独立 HWND，除非证据要求第二窗口 |
