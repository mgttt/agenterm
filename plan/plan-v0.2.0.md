# AgenTerm v0.2.0 公开计划（Control Center 内容成熟）

状态：**规划中**（2026-08-04 升级；原 7-31 骨架重定向后经 v0.1.11→v0.1.14
 验证，CC 壳层与 Platform Facade 已 shipped，见 §零 前置现状）。
前置：v0.1.11 Control Center / native IPC foundation 已通过跨平台证据门。
本文是版本执行计划与决策记录，不是产品事实；接受的产品范围同步进
owning `prd/PRD_*.md`；灵感条目见 `PRD_02_19_inspiration_and_future_vision.md`。

工作主题：**Control Center 内容成熟** — 深化独立 `agenterm-cc`，而不是
回到主 GUI overlay。在 v0.1.11 已建立的安静主界面入口、独立进程和 typed
bridge 之上，让 Cockpit、Workflows、Extensions 与 InfoHub 形成首批真实
纵向能力；全部复用既有权威，不在 Control Center 内复制状态、调度器、
安装器或网络节点。

## 零、前置现状（2026-08-04 已核）

- [x] v0.1.11 CC 壳层 shipped：进程边界 / typed bridge / read-only Cockpit；
  三平台 lifecycle/renderer/caller-instance 有 matching-host 证据
  （alignment-contract `control-center.*` 29 项 evidence）
- [x] Platform Facade revision 4 收口：产品代码无 OS 分支，机制全部经
  `crates/agenterm-platform`；`src/platform/boundary_tests.rs` 拦新原生导入
- [x] 共享 UX 语义单点化收敛（interaction/selection/modal/focus/snapshot schema）
- [~] agenterm-net N2-M1 在 `research/` 进行中（非 v0.2.0 前置，见 §三）
- [ ] Workflows authority（C1）、softmgr 底座（D1/D2）、InfoHub source
  framework（E1）均未开工——是 v0.2.0 各纵切的**外部依赖**，不是 CC 自己实现

## 一、产品 outcome（一棵树）

```text
Control Center（独立 agenterm-cc；v0.1.11 壳层之上的内容成熟）
├─ [ ] Cockpit — Fleet 状态、事件、异常与 typed shortcuts（v0.1.11 read-only 之上加深）
├─ [ ] Workflows — definitions / runs / pipeline view / evidence
│      权威在 flow worker/MCP orchestration；CC 只投影与发起 typed 请求
├─ [ ] Extensions — PluginHub 与 AppHub 共用 catalog / source / softmgr 底座
├─ [ ] InfoHub — source / provenance / route / Composer 或 workflow 输入
├─ [ ] renderer — native shell 为主；WebView 仅当六目标门通过才可生产化
└─ [ ] diagnostics — 组件能力、版本、连接、失败与支持包
```

命名：对外统一 **Control Center**（Fleet Hub 为被替代的历史假设）；
用户提示可能改名 → 决策项 P2。

## 二、依赖与集成边界

```text
v0.1.11 Control Center process + typed bridge
        │
        ├─► Cockpit ← list-windows / journal / snapshots / receipts
        ├─► Workflows ← versioned flow authority（flow worker / MCP orchestration，C1）
        ├─► Extensions ← shared catalog + softmgr transaction authority（D1/D2）
        ├─► InfoHub ← sources + provenance + routes（E1/E2 framework）
        └─► optional WebView ← renderer-neutral state + typed local bridge
```

- CC 只投射与发起 typed 请求；不成为第二份产品真相（server 仍是唯一权威）。
- 不静默下载 / 安装 / 更新 / 提升权限；所有 mutation 先展示计划并产生
  machine-readable result。
- 远程包管理（agenterm.work）与 net（libp2p/IPFS）是**后续主线**，v0.2.0
  只在边界上预留（InfoHub 未来可接 net 源；Extensions 未来可接远程 catalog），
  不提前嵌入（见 §三、§五）。

Unix Win 对齐地图：[`plan-unix-gui-win-parity.md`](plan-unix-gui-win-parity.md)
跨平台 GUI：[`plan-multiplatform-gui.md`](plan-multiplatform-gui.md)

## 三、v0.2.0 分期（建议）

### Phase A — Cockpit 首个可操作纵切（第一优先）

- [ ] 在 v0.1.11 read-only Cockpit 上增加健康、异常、运行与证据下钻
- [ ] 操作只调用已存在的 typed authority，并通过 receipt/post-state 验证
- [ ] server restart / epoch gap / 多 server 切换有明确重建基线体验
- [ ] native/WebView 投射使用相同语义 snapshot 和 action IDs
- [ ] 独立 CC 进程复用、崩溃恢复和 terminal/server 隔离持续通过

### Phase B — Workflows 与 Extensions（依赖外部 authority 先开工）

- [ ] **Workflows**：定义、运行、取消、恢复和 evidence 视图；durable
  authority 仍在 flow worker / MCP orchestration（C1 立项后 CC 才投影）
- [ ] **Extensions**：PluginHub/AppHub 分视图，共用 sources、provenance、
  compatibility、install plan 与 softmgr 事务（D1/D2 立项后 CC 才消费）
- [ ] 在 authority 未开工前，各视图暴露 truthful planned/unavailable 状态，
  不把 Rhai 任务提升为 durable flow（PRD_02_21 Workflows 段已声明）

### Phase C — InfoHub 与可选 WebView（依赖 E1 framework）

- [ ] **InfoHub**：显式 source、provenance/CID、过滤与路由到 Composer
  draft、workflow input 或通知（E1 source framework 先立项）
- [ ] 若 WebView 六目标（availability、bridge isolation、offline、crash、
  size）门已通过，可成为 CC 的生产 renderer；否则继续使用 native shell

### 明确非目标（v0.2.0）

- PluginHub/AppHub 的公共交易、自动下单、静默安装
- Control Center 内第二套 PTY、server、workflow、softmgr 或 net authority
- 未通过独立可靠性门即把 libp2p/IPFS 嵌入稳定 server（L-NET 归后续版本）
- 加载任意远程网页并授予 privileged host bridge
- 远程包管理（agenterm.work）生产化（L-PKG 归后续版本）

## 四、验收证据

| 能力 | 证据 |
|------|------|
| 入口与几何 | v0.1.11 Control Center toolbar/process qualification 持续通过 |
| Cockpit | CLI `ui-snapshot` 字段 + PNG |
| 交互 | typed action + receipt/post-state 黑盒 + 六目标 smoke |
| 不回退 | 主工作区 terminal/composer 旅程仍绿 |

## 五、与未来主线的关系（衔接 plan-v0.1.15 §五）

| 主线 | 归口 | v0.2.0 内的边界 |
|------|------|-----------------|
| L-CC（本版） | v0.2.0 | 本版主体 |
| L-NET（ipfs/libp2p） | PRD_02_22 N2→N3→N4；v0.2.0 之后 | InfoHub 可预留 net source 类型，不嵌节点 |
| L-PKG（远程包管理 / agenterm.work） | PRD_02_04 softmgr + 后续版本 | Extensions 预留远程 catalog 概念，不实现交易 |
| L-EXT（插件/皮肤/信息 + rhai） | PRD_02_04 / PRD_02_10 | Extensions 分视图 + rhai 能力消费（不引入 Script 权限层） |
| L-CU（computer-use 自有实现） | 未入 PRD，决策项 P4 | v0.2.0 不启动；P4 拍板后走 promotion 链 |
| Mobile（第三 host） | plan-mobile.md | 与 CC 同为 server 消费者，互不前置 |

## 六、风险与门

- **authority**：CC 只投射和发起 typed 请求，不成为第二份产品真相。
- **命名**：对外统一 **Control Center**；改名需 P2 拍板后全链同步。
- **quiet surface**：主界面只保留 v0.1.11 已验证入口，不把内容塞回终端。
- **外部依赖先行**：Workflows/Extensions/InfoHub 的 authority 不在本版实现；
  若 C1/D1/E1 未按时开工，Phase B/C 顺延，不降级为 CC 内第二套实现。

## 七、待拍板决策项（agent 不自主执行）

| ID | 决策 | 影响 |
|----|------|------|
| P1 | agenterm.work 与 agenterm.mega.tech 的域名归属/迁移 | 决定 L-PKG 基建与 E1（pages 噪音治理）走向 |
| P2 | Control Center 是否改名、改什么名 | 影响 PRD_02_21 标题/命名、可执行族与全文档 |
| P3 | 「皮肤」扩展面与 theme/plugin 打包的边界 | 决定 L-EXT 范围与版本归口 |
| P4 | computer-use 是否立项、归口 PRD、首发平台与证据门 | 决定 L-CU 的版本归口（v0.2.0 之后或更后） |
| D1–D3 | 发布链政策（见 plan-v0.1.15 §一 D 组） | 与 v0.2.0 独立，但影响发布节奏 |
| K1–K5 | 移动端决策项（见 plan-mobile.md §七） | 与 v0.2.0 独立，互不阻塞 |

## 八、排序建议（起稿人观点）

1. **Phase A Cockpit**：零外部依赖（壳层 + typed bridge 已 shipped），
   一晚可落地首个可操作纵切。
2. **并行推动 C1/D1/E1 立项**：Workflows/Extensions/InfoHub 的 authority
   是外部依赖，越早立项 Phase B/C 越早可投影。
3. **P1/P2 尽早拍板**：域名与命名影响多文档与基建，晚拍板改造成本递增。
4. **L-NET / L-PKG / L-CU 归 v0.2.0 之后**：与 v0.2.0 边界清晰，不提前嵌入。

## 九、决策记录

| 日期 | 决策 |
|------|------|
| 2026-07-30 | v0.2.0 主题定为 Fleet Hub 四 tab；插件与信息面称 PluginHub / InfoHub |
| 2026-07-30 | 第一刀采用 overlay（对齐 Settings），非独立 HWND，除非证据要求第二窗口 |
| 2026-07-31 | 新产品推演以独立 `agenterm-cc` Control Center 取代 Fleet Hub overlay；Cockpit 保留为其默认 Fleet 视图 |
| 2026-07-31 | v0.1.11 前置壳层、进程边界、本地 IPC 与研究基建；v0.2.0 重定向为内容成熟，不重复实现入口 |
| 2026-08-04 | 升级规划：前置现状（CC 壳层/Platform Facade）已核；Phase A 零依赖优先；C1/D1/E1 外部 authority 先行；P1–P4 为待拍板决策项 |
