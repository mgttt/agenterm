# AgenTerm v0.2.0 公开计划（重定向骨架）

状态：规划中  
前置：**v0.1.11 Control Center / native IPC foundation** 完成并通过跨平台证据门

工作主题：**Control Center 内容成熟** — 深化独立 `agenterm-cc`，而不是回到主 GUI overlay
版本定位：在 v0.1.11 已建立的安静主界面入口、独立进程和 typed bridge
之上，让舰队驾驶舱、工作流、Extensions 与 InfoHub 形成首批真实纵向能力；
全部复用既有权威，不在 Control Center 内复制状态、调度器、安装器或网络节点。

本文是版本执行计划与决策记录，不是产品事实。接受的产品范围同步进 owning `prd/PRD_*.md`；
灵感条目见 [`PRD_02_19_inspiration_and_future_vision.md`](prd/PRD_02_19_inspiration_and_future_vision.md)。

## 产品 outcome（一棵树）

```text
Control Center（独立 agenterm-cc；v0.1.11 壳层之上的内容成熟）
├─ [ ] Cockpit — Fleet 状态、事件、异常与 typed shortcuts
├─ [ ] Workflows — definitions / runs / pipeline view / evidence
├─ [ ] Extensions — PluginHub 与 AppHub 共用 catalog / source / softmgr 底座
├─ [ ] InfoHub — source / provenance / route / Composer 或 workflow 输入
├─ [ ] renderer — native shell 与通过资格门的系统 WebView 投射
└─ [ ] diagnostics — 组件能力、版本、连接、失败与支持包
```

## 依赖与集成边界

```text
v0.1.11 Control Center process + typed bridge
        │
        ├─► Cockpit ← list-windows / journal / snapshots / receipts
        ├─► Workflows ← versioned flow authority（不是 CC 自己执行）
        ├─► Extensions ← shared catalog + softmgr transaction authority
        ├─► InfoHub ← sources + provenance + routes
        └─► optional WebView ← renderer-neutral state + typed local bridge
```

Unix Win 对齐地图：[`plan-unix-gui-win-parity.md`](plan-unix-gui-win-parity.md)  
跨平台 GUI：[`plan-multiplatform-gui.md`](plan-multiplatform-gui.md)

## v0.2.0 建议分期

### Phase A — Cockpit 首个可操作纵切

- [ ] 在 v0.1.11 read-only Cockpit 上增加健康、异常、运行与证据下钻
- [ ] 操作只调用已存在的 typed authority，并通过 receipt/post-state 验证
- [ ] server restart/epoch gap/多 server 切换有明确重建基线体验
- [ ] native/WebView 投射使用相同语义 snapshot 和 action IDs
- [ ] 独立 CC 进程复用、崩溃恢复和 terminal/server 隔离持续通过

### Phase B — Workflows 与 Extensions

- [ ] **Workflows**：定义、运行、取消、恢复和 evidence 视图；durable
  authority 仍在 flow worker/MCP orchestration
- [ ] **Extensions**：PluginHub/AppHub 分视图，共用 sources、provenance、
  compatibility、install plan 与 softmgr 事务
- [ ] 不静默下载、安装、更新或提升权限；所有 mutation 先展示计划并产生
  machine-readable result

### Phase C — InfoHub 与可选 WebView

- [ ] **InfoHub**：显式 source、provenance/CID、过滤与路由到 Composer
  draft、workflow input 或通知
- [ ] 若 WebView 六目标 availability、bridge isolation、offline、crash 与
  size 门已通过，可成为 CC 的生产 renderer；否则继续使用 native shell

### 明确非目标（v0.2.0）

- PluginHub/AppHub 的公共交易、自动下单、静默安装
- Control Center 内第二套 PTY、server、workflow、softmgr 或 net authority
- 未通过独立可靠性门即把 libp2p/IPFS 嵌入稳定 server
- 加载任意远程网页并授予 privileged host bridge

## 验收证据

| 能力 | 证据 |
|------|------|
| 入口与几何 | v0.1.11 Control Center toolbar/process qualification 持续通过 |
| Cockpit | CLI `ui-snapshot` 字段 + PNG |
| 交互 | typed action + receipt/post-state 黑盒 + 六目标 smoke |
| 不回退 | 主工作区 terminal/composer 旅程仍绿 |

## 风险与门

- **authority**：CC 只投射和发起 typed 请求，不成为第二份产品真相
- **命名**：对外统一 **Control Center**；Fleet Hub 仅为被替代的历史假设
- **quiet surface**：主界面只保留 v0.1.11 已验证入口，不把内容塞回终端

## 决策记录

| 日期 | 决策 |
|------|------|
| 2026-07-30 | v0.2.0 主题定为 Fleet Hub 四 tab；插件与信息面称 PluginHub / InfoHub |
| 2026-07-30 | 第一刀采用 overlay（对齐 Settings），非独立 HWND，除非证据要求第二窗口 |
| 2026-07-31 | 新产品推演以独立 `agenterm-cc` Control Center 取代 Fleet Hub overlay；Cockpit 保留为其默认 Fleet 视图 |
| 2026-07-31 | v0.1.11 前置壳层、进程边界、本地 IPC 与研究基建；v0.2.0 重定向为内容成熟，不重复实现入口 |
