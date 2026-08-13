# AgenTerm 移动端计划（占位稿 / 思维工作树）

状态：**占位草案**（2026-08-04 起草；2026-08-13 与 PRD 33 对齐）。
产品真理：[`prd/PRD_02_33_mobile_reach.md`](../prd/PRD_02_33_mobile_reach.md)。
本文只保留 **原生壳**（M-A / M-B / M-C）的执行投影。

**第一宿主是 PWA**，不是商店 App：`https://agenterm.work/app`，源在 `docs/`，
首页加 **Mobile App** 入口。iOS / Android 因审核慢（及避免双壳抢跑）**不急开**，
树上只留占位。扫码绑定桌面是 PWA 之后的增量。

不改变任何已发布/在途版本的授权状态；不创建 tag/Candidate/Release；
不把本轨塞进 v0.1.18 / v0.1.19。原生开工前仍需人工确认 §三 壳选型与 §七 K1–K3。

产品定位（用户已定，不再讨论）：**移动端 = 桌面端的接入端 + 去中心化链接端**。
手机上**不跑** agent/terminal 本体（无 PTY、无 workspace 权威、无 server）。
手机是第三个 host：它连接一个已存在的 `agenterm server` 权威并投影其状态。

---

## 一、背景与定位

### 1.1 为什么「第三个 host」是既有架构的自然延伸，不是新架构

桌面端**早已**完成 server ↔ UI 分离，移动端只是复用同一条缝：

- `plan/ARCHITECTURE.md` §2：`agenterm server` 是「工作区/PTY/事件权威（可替换 UI 的
  headless）」；`src/platform/adapters/windows/` 明标 “replaceable remote UI ↔ agenterm server”。
- `src/ui_bridge.rs` 已是**成文线协议**：`UI_BRIDGE_SCHEMA_VERSION = 7` /
  `UI_BRIDGE_PROTOCOL_VERSION = 1`，`negotiate()` 做 `UiProtocolRange` 协商并返回
  `UiCompatibility::{Compatible, ClientTooOld, ClientTooNew}`；`headless_server_facts()`
  相对 `current_facts()` 打开 `replaceable_ui / interactive_lease / reconnect /
  rollback_proven` 四个事实位——「可替换 UI 客户端」不是设想，是已声明能力。
  它还是 `src/lib.rs` 里少数 `pub mod` 之一（`ui_client` / `ui_lease` / `ui_snapshot` /
  `platform::contract::ipc` 全是 crate-private）：协议面已半独立，客户端内核还没有。

移动端要做的不是「移植 GUI」，而是**再写一个 `UiClientModel` 的宿主**。

### 1.2 为什么同仓、为什么不是 `src_mobile/`

- 协议与产品语义必须**单点**。另起仓库 = 让 `ui_bridge` 的 schema 版本在两个仓库里漂移，
  这正是 ARCHITECTURE.md 「禁止第三现实」要打掉的东西。
- `src_mobile/` 是**错误的分层轴**：它把「host 差异」升格成顶层目录，等价于在产品树上
  开第二套 OS adapter（ARCHITECTURE.md §6 禁令 1/4）。桌面端 Win/Unix 双 host 的教训是：
  host 差异只能停在「怎么画/怎么收事件」，不能上浮成目录级并行宇宙。
- 正解是 **workspace member**：协议与客户端内核是**库**，移动壳是**宿主应用**。
  现状实证：根 `Cargo.toml` 只有 `members = [".", "crates/agenterm-platform"]`，**没有**
  `default-members` 键——新增 member 会默认参与全量构建，故 M-A 落地时必须**同批显式声明
  `default-members`**，否则桌面 30 分钟资格门会被移动端拖长。（`research/agenterm-net`
  用的是「自带 `[workspace]` 独立隔离」，是另一条可选路径，见 §七 K2。）

---

## 二、三阶段目标树（占位，未定版）

```text
Mobile  接入端 + 去中心化链接端
│
├─ M-P. PWA 第一宿主（产品在 PRD 33；本仓 docs/）
│  ├─ [ ] P1 docs/ 首页增加 Mobile App 入口，打开 /app
│  ├─ [ ] P2 Web App Manifest + 可安装独立显示
│  ├─ [ ] P3 诚实占位 UI（未绑定设备列表 / 「等桌面出示二维码」）
│  └─ [ ] P4 以后：扫码绑定（LAN、observe）再谈 live 投影
│     非目标：不把 PWA 做成第二个网站；不进 0.1.18/0.1.19
│
├─ M-A. crates/agenterm-protocol —— 抽取线协议（原生壳仍需要；PWA 可后接）
│  ├─ [ ] A1 从 src/ui_bridge.rs 抽出传输无关 DTO 层
│  │     动机：移动客户端与桌面 remote UI 必须共用同一份 schema，否则版本漂移
│  │     候选抽取物（以代码为准，均在 src/ui_bridge.rs）：
│  │       · 握手/协商：UiHelloRequest / UiHelloResponse / UiProtocolRange /
│  │         UiCompatibility / negotiate()
│  │       · 事件位置：UiEventPosition { server_epoch, sequence }
│  │       · 投影：UiBootstrapSnapshot / UiTabBootstrap / UiScreenSnapshot / UiCellRun /
│  │         UiCellStyle / UiColor / UiCursorSnapshot / UiComposerSnapshot /
│  │         UiWorkingContextSnapshot
│  │       · 增量：UiDeltaBatch / UiDeltaEvent（after/through/current_sequence 三元序
│  │         + complete/truncated 的 validate() 不变式）
│  │       · 租约：UiLeaseGrant（lease_id / ttl_ms / expires_unix_ms / observed_sequence
│  │         ≤ position.sequence）+ src/ui_lease.rs 的 UI_LEASE_TTL_MS 与 UiLeaseError
│  │         码表（code/category/retryable）
│  │       · 硬上限：UiContractLimits / UiBridgeFacts（bootstrap 8MiB、delta 64 事件、
│  │         screen 512×512、input 256KiB…）——移动端必须继承同一组拒绝阈值
│  │     依赖：无（纯 serde + std）
│  │     证据要求：抽取前后 `ui_bridge` 全部 validate() 单测逐字保留并全绿；
│  │       主 crate 改为 re-export，`UI_BRIDGE_SCHEMA_VERSION` 不变（纯搬家，非改版）
│  ├─ [ ] A2 身份契约的**传输无关子集**（src/platform/contract/ipc.rs）
│  │     可抽：LogicalInstance（Main/Dev/Ephemeral/Custom + canonical_name /
│  │       display_label / FromStr / serde）、ServerScopeId（"agt-v1-" + 32 hex，
│  │       SHA256 over "agenterm.server-scope.v1"，不含 username/UID/SID）
│  │     **不可抽**：resolve_ipc_endpoint / EndpointSelectorArgs /
│  │       ResolvedIpcEndpoint / TrustedOsUserScope —— 它们读进程安全上下文与
│  │       环境变量，且 IpcEndpoint 走 validate_local()（强制 loopback / 本机 pipe），
│  │       是**本机**授权语义，手机上无对应物，必须留在桌面侧
│  │     动机：手机要显示/选择「连的是哪个权威」，需要 LogicalInstance 的显示语义与
│  │       ServerScopeId 的不可反推身份；但**不能**继承本机 endpoint 解析
│  │     证据要求：agenterm-platform 边界纪律照抄——protocol crate 里不得出现
│  │       Win32/POSIX 类型、`AGENTERM_` 环境变量、进程/文件系统调用（加边界测）
│  └─ [ ] A3 ui-snapshot schema 的 typed 化前置审计（本叶是**审计**，不是直接抽取）
│        现状（已核）：src/ui_snapshot.rs 全是 pub(crate) 的 serde_json::json!() 构造器
│        （locale_json / settings_json / working_context_json / terminal_interaction_json
│        / embedded_window_json_with_state / …），**没有** typed 结构体；版本号
│        UI_CLIENT_STATE_SCHEMA_VERSION = 1 挂在 ui_bridge。移动端若要消费同一投影，
│        需先定 typed 契约或 JSON schema，否则抽出来的只是字符串约定
│
├─ M-B. crates/agenterm-client-core —— 纯 Rust 无 UI 客户端内核
│  ├─ [ ] B1 以 src/ui_client.rs::UiClientModel 为蓝本重写为传输注入式
│  │     现有能力（已核）：connect / snapshot / maintain_lease_if_due / poll_deltas /
│  │       select_tab / send_input / paste_input / resize_request / publish_snapshot /
│  │       poll_client_command / detach
│  │     必须解耦：现版直接吃本机 IPC（crate 内 ipc_transport）；client-core 要把传输
│  │       收成 trait（发字节/收字节 + 超时），实现由宿主注入。依赖：M-A 完成
│  ├─ [ ] B2 连接生命周期：重连 + epoch 变更 + 序号回退的显式状态机
│  │     依据：server_epoch 变化 = 权威换代，必须丢弃投影缓存重取 bootstrap；
│  │       delta 的 after/through/current 三元序给出「是否追平」的判定
│  ├─ [ ] B3 投影缓存与租约续期：移动端网络切换/后台挂起下的 TTL 语义
│  │     UI_LEASE_TTL_MS = 5000 对移动网络是短的；需要「失租后只读投影仍可见」
│  │     而不是黑屏 —— 这是产品语义决策，见 §七 K3
│  ├─ [ ] B4 FFI-ready：无全局状态、无 panic 跨边界、错误码为 typed 枚举（沿用
│  │     ui_lease 的 code/category/retryable 三元分类风格）
│  └─ [ ] B5 平台差异纪律：推送/后台存活/网络切换/IME 走 capability 描述 + typed
│        Unsupported（照抄 agenterm-platform 的 CapabilityStatus::Unsupported）；
│        **禁止** `if ios / if android` 业务分支
│
└─ M-C. apps/mobile —— 壳（最后一步；C1 壳选型拍板见 §三 / §七 K1）
   ├─ [ ] C2 FFI 绑定层：uniffi 或 flutter_rust_bridge，只暴露 client-core
   ├─ [ ] C3 最小可用纵切：连接 → bootstrap 投影 → 只读终端画面 → 断线重连
   ├─ [ ] C4 交互纵切：取租约 → send_input → 观察 delta 追平
   └─ [ ] C5 独立 CI 车道 + 独立版本 tag（mobile-v0.1.0）
```

## 三、壳选型对比（需人工拍板）

| 维度 | Flutter | React Native | Tauri 2 mobile |
|------|---------|--------------|----------------|
| Rust 集成 | `flutter_rust_bridge`（成熟、异步友好、codegen 强） | uniffi / JSI 手写桥（链路最长） | Rust 原生宿主，client-core 直接同进程 |
| 终端渲染 | 自绘引擎，等宽网格与滚动可控性最好 | RN 原生组件不擅长字符网格，多半仍要 WebView/Canvas | WebView 渲染，字符网格靠 web 侧实现 |
| 包体 | 中（引擎自带，~10-20MB 基线） | 中偏大（JS 运行时） | 小（系统 WebView） |
| 生态/风险 | Dart 生态自成一套，需引入新技术栈 | JS 生态最大，但桥接层最厚、性能与调试成本最高 | 最年轻；系统 WebView 差异大，且本仓 WebView 仍是 research |

**起稿人倾向**：`Flutter + flutter_rust_bridge`。终端画面本质是高频重绘的字符网格
（`UiCellRun` 行程编码 + delta 追平），自绘引擎对帧稳定性与等宽排版控制力最强，且 Rust 侧
只需暴露 client-core，桥接面小。**Tauri 2 mobile** 是「坚持单一 Rust 栈」时的次选。
**此为倾向，非决定——需人工拍板（§七 K1）。**

## 四、与去中心化链接的关系

- **传输无关是硬约束**：`agenterm-protocol` 只定义「消息长什么样、怎么协商、什么算合法」，
  不定义「字节怎么送」。因此链路选型（本机 IPC / TCP over LAN / 中继 / libp2p）
  可以推迟到有真实证据时再定，而不阻塞 M-A、M-B。
- **`agenterm-net` 目前是 research**（`research/agenterm-net`，自带独立 `[workspace]`；
  成熟度门 N0→N4 见 `prd/PRD_02_22`，plan-v0.1.13 已确立「不得进 stable 宣称」）。
  本 plan **不**把 net 作为移动端的前置依赖，也不因移动端需求提前给 net 升级成熟度。
- 现有 `IpcEndpoint` 的 `validate_local()` 强制 loopback/本机命名管道——它是**本机安全边界**，
  不是远程传输的雏形。移动端的远程链路是**新的授权面**（配对、信任、加密），
  必须独立立项，**不得**通过放宽 `validate_local()` 来「顺手打通」。

## 五、明确非目标

- 不在手机上跑 agent/terminal 本体（无 PTY、无 workspace 权威、无 server 自启动）。
- 不动桌面发布链：不进 `.github/workflows/candidate.yml`、不进
  `scripts/qualification-gates.json`；桌面 30 分钟资格门**零增长**是硬指标。
- 不进 `workspace.default-members`（M-A 必须同批把该键显式补上，见 §1.2）；
  本 plan 不创建任何 tag / Candidate / Release，不改任何代码。
- 不借移动端需求修改 `UI_BRIDGE_SCHEMA_VERSION`、放宽 `UiContractLimits`，
  或新增第二套 GUI 启动解析 / server autostart 决策（ARCHITECTURE.md §6 禁令 4）。

## 六、与其它文档的关系

| 文档 | 关系 |
|------|------|
| `plan/ARCHITECTURE.md` | 结构 SSOT；新增 crate/目录须同批更新其 §1 分层与 §4 债务表 |
| `src/ui_bridge.rs` | M-A 抽取的**唯一真源**；本文不复制其字段定义 |
| `src/platform/contract/ipc.rs` | 身份契约来源；可抽子集与不可抽部分见 §二 A2 |
| `plan/plan-v0.1.15.md` | 发布链经济学；移动端必须不与其 30min 目标冲突 |
| `prd/PRD_02_18_roadmap.md` M12 | Control Center 内容成熟；CC 与 mobile 都是 server 的消费者，不互为前置（原 plan-v0.2.0.md 已并入） |
| `prd/PRD_02_22_decentralized_network.md` | agenterm-net 成熟度门（N0→N4），约束 §四 |
| `prd/PRD_02_20_native_platform.md` | Platform Facade 纪律来源；capability/typed Unsupported 的样板 |
| `prd/PRD_02_33_mobile_reach.md` | **产品归口**（PWA / 商店占位 / 扫码绑定） |
| `prd/PRD_02_18_roadmap.md` | 里程碑权威；33 已登记，无版本号 |
| `prd/PRD_02_19_inspiration_and_future_vision.md` | Lane F；F1–F3 已 promote 到 33 |

## 七、待拍板决策项（agent 不自主执行）

| ID | 决策 | 影响 |
|----|------|------|
| K1 | 壳选型：Flutter / React Native / Tauri 2 mobile | 决定 M-C 全部形态与 FFI 路径 |
| K2 | `apps/mobile` 是 workspace member 还是自带 `[workspace]` 隔离（照 agenterm-net） | 决定构建/CI 隔离强度 |
| K3 | 失租/断网时移动端的产品语义（只读投影 vs 显式断开态） | 决定 B3 状态机与 UX |
| K4 | 远程链路的授权面（配对/信任/加密）归哪个 PRD、证据门是什么 | **已拍：产品 UX 归 [33](../prd/PRD_02_33_mobile_reach.md)**；密码学可参考 22，不得放宽本机 `validate_local()` |
| K5 | 移动端是否立项、归口 PRD、目标版本 | **已拍：立项，归 PRD 33；无版本号。PWA 先行，商店 App 占位** |

---

## 八、决策记录

| 日期 | 决策 |
|------|------|
| 2026-08-04 | 移动端定位为「桌面接入端 + 去中心化链接端」，不跑 agent/terminal 本体 |
| 2026-08-04 | 采用同仓 workspace-member 布局（protocol → client-core → apps/mobile），否决 `src_mobile/` |
| 2026-08-04 | 移动端不进桌面发布链与 default-members；独立 CI 车道与独立 tag（mobile-v0.1.*） |
| 2026-08-04 | 协议 crate 保持传输无关；agenterm-net 维持 research，不作为移动端前置 |
| 2026-08-13 | 产品归口 PRD 33。第一宿主 = PWA（`https://agenterm.work/app`，复用 `docs/`）。iOS/Android 商店 App 保持占位。K4/K5 关闭。原生 M-C 仍待 K1 |
