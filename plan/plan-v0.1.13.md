# AgenTerm v0.1.13 公开计划

状态：布点草案（2026-08-02，持续增补）；**不改变 v0.1.12 发布状态**，
不触发 Candidate、tag 或 Release。本文记录：为何对 v0.1.12 仍不满意、
跨平台封装分层审查、平台/产品待收敛项、以及本机基线测试中新暴露的问题。

主题（三轨）：

1. **发布与产品体感**：把 v0.1.12「能收口」变成「愿意发」——补齐
   Candidate 封印、Promotion 彩排、以及仍影响日常信任的体验/证据缺口。
2. **跨平台 UI/UX 分层妥当性**（用户点名重点）：机制在
   `agenterm-platform`，产品语义在主 crate，三 OS 只在能力缺口上分叉，
   不在业务策略上复制三套实现。
3. **平台 crate 收窄**：单一事实、typed 失败、可复用 facade；删除半迁/
   纯转发/假成功。

基线 SHA：

- GPT 跟进 filesystem facade 叶停在
  `fa92bc88a9799b2a348e00b42c177a5bc7e334dd`。
- DeepSeek 已合入并推送
  `1c582c77521ce3299fb534188e942df6b4b3c2a1`
 （`refactor(frontend): unify UI/UX ingress and restore platform CI gates`，
  37 files）。本节分层 review 以 **`fa92bc8..1c582c7`** 为对象。
- 该 commit 自称 Quick 全绿后上 main；**不**等于 all-feature crate 测与
  六平台 CI 已齐（见 §四 / §八）。

---

## 一、对 v0.1.12 不满意的原因（问题分析）

> v0.1.12 的产品主体大量已 `[x]`，但「再次公开发布」与「用得放心」仍被
> 发布闭环缺口、部分体验边界、以及持续微重构噪音挡住。不满主要不是
> 「还差三个大 feature」，而是 **资格链未封印 + 信任面仍有豁口**。

### 1.1 发布资格链未闭环（发版 P0）

| 缺口 | 证据/出处 | 影响 |
|------|-----------|------|
| 尚无 `v0.1.12` tag / 公开 Release | `git tag` 最新公开族至 `v0.1.11`；Cargo 已是 `0.1.12` | 版本号与用户可见发布脱节 |
| exact-SHA Candidate 未记录「fully sealed 成功重跑」 | `prd/PRD_02_17_delivery_quality.md`：六 runner 曾因 Windows/Unix 行尾哈希 fail closed；LF 已钉，**成功封印仍待** | Promotion 无合法输入 |
| 非发布 Promotion 彩排未记录 | 同 PRD `[~]` | 真发布前 blind |
| Wave D 入口要求 clean main + 普通 CI 六 cell | `plan/plan-v0.1.12.md` 完成定义 | 任何未收口微重构都会推迟冻结 |

**结论**：v0.1.12 若「再发」，最短路径是冻结 SHA → 成功 Candidate →
Promotion 彩排 → 人工批准；**不是**再开大功能波次。

### 1.2 产品/体验仍挂着的信任缺口（发后维护或 0.1.13 优先）

这些多已在 0.1.12 plan/PRD 标为延期或 partial，但会直接影响「发了也不爽」：

| 缺口 | 说明 | 建议归属 |
|------|------|----------|
| macOS 真人 physical pointer | 已接受 typed `Unsupported` 为本版边界；无正向指针证据 | 0.1.13+ 平台输入；不冒充 shipped |
| Cockpit 仍偏诊断壳 | 只读事实有 Windows 证据；native 指针/键盘导航、Linux renderer 纵深未齐 | 0.1.13 小步加深诊断，**大内容进 0.2.0** |
| REPL 箭头编辑 / history | supervision/Ctrl+C 已有；交互编辑仍开放 | 0.1.13 Script 体验叶 |
| Unix hosted interactive Ctrl+C | 多仅 direct protocol / unit；hosted journey 不全 | 分宿主补证据 |
| Keep Server / Job breakaway 体感债 | 0.1.12 已修主路径，但 `CallerJobFallback` 仍是可观察宿主限制 | 文档 + 诊断面说清，不装「永远 Keep」 |
| raw-mouse / 完整 professional selection | 独立叶 | 非 0.1.12 挡板；0.1.13+ 可选 |
| agenterm-net / WebView | experimental / research | 不得进 stable 宣称 |

### 1.3 架构与工程卫生（微重构带来的「不干净」）

| 缺口 | 说明 |
|------|------|
| Frontend 边界半收敛 | 产品 `src/frontend/mod.rs` / `frontend_server.rs` 与
  `platform::services::frontend` 曾并存；半删 orphan shim、import 迁移动作
  易残留。目标：单一产品入口，无死文件、无 `#[allow(unused_imports)]`
  创可贴式压制。 |
| 弱模型/开放式微重构风险 | 任务过宽时易夹带无证据行为补丁（例：坐标启发式、
  放宽 OS 校验、整文件 rewrite）。0.1.13 纪律：名单内叶子、出界即回滚。 |
| platform facade 与产品 glue 仍交织 | paths 失败语义、Capability 映射、CC 截图策略
  等仍可能在主 crate 重复或静默降级（见下方目标树）。 |
| 开发 `target/` 膨胀 | 0.1.12 有 partial 回收；整 root generation 生产删除证据仍缺 | 

### 1.4 与「不满意」对应的版本选择

```text
若目标是「尽快有一个合法 v0.1.12 公共包」
  → 冻结当前可接受 SHA，走 Wave D；0.1.13 不挡 tag

若目标是「发了也认可体感」
  → 在 0.1.13 明确收：平台失败语义、shared-memory/测试卫生、
    frontend 边界收口、1～2 个最高痛点体验叶
  → 大 CC 内容 / net / WebView 仍进 0.2.0
```

本文默认：**0.1.13 负责「信任与平台收窄」；0.1.12 发布链仍独立授权。**

---

## 二、目标树

```text
v0.1.13  Trust & platform narrowness
│
├─ A. 发布与冻结协作（不替代 0.1.12 授权，但消除「永远差一口气」）
│  ├─ [ ] 记录一次 fully sealed exact-SHA Candidate（六平台 + Windows stress receipt）
│  ├─ [ ] 记录一次 non-publishing Promotion 彩排
│  ├─ [ ] 冻结纪律：名单外 diff 不进 main；半迁必须同批删或同批恢复 shim
│  └─ [ ] 分段计时/runner 试验可做，但不得改变 eligibility
│
├─ B. 平台抽象收敛（继承草案）
│  ├─ [ ] 路径/目录失败保持 typed Failed/Unsupported，禁止静默 temp fallback
│  ├─ [ ] Control Center 截图策略由 agenterm-platform 单一提供
│  ├─ [ ] CapabilityStatus / PlatformSnapshot 减少主 crate 重复映射
│  ├─ [ ] 薄包装 facade 审计：删除纯转发，保留产品策略 glue
│  ├─ [ ] 外部依赖 feature bundle 与最小依赖树回归
│  └─ [ ] 统一跨平台 fixture/nonce/RAII cleanup，降低并行测试碰撞
│
├─ C. Frontend / server 边界收口
│  ├─ [ ] `frontend` = 启动/参数/wake；`frontend_server` = server 拉起/恢复
│  ├─ [ ] 禁止第二套 autostart 决策（CLI 只委托）
│  ├─ [x] 无 orphan `services/frontend`（已删；boundary_tests 防再长）
│  └─ [x] 结构 SSOT=`plan/ARCHITECTURE.md`；boundary-tree=历史文（非 PRD 地图）
│
├─ F. 跨平台分层（重点，见 §八）
│  ├─ [x] 取消 `frontend.rs` 对 adapter 的 `#[path]` 虚树；adapter 归属 `platform::adapters`
│  ├─ [x] new-terminal / settings / live-tab close / tab editor / window-close / CWD editor 语义已进 `src/frontend/{new_terminal,settings,close_confirmation,tab_editor,window_close,cwd_editor}.rs`
│  │     （Win/Unix 共用状态/校验/action，adapter 只保留原生呈现与事件映射）
│  ├─ [x] modal/focus surface 命名/解析单点：ModalSurface + FocusSurface::as_str()/from_ipc()（interaction.rs；Win/Unix 共用）
│  ├─ [x] sidebar scrollbar geometry 单点：sidebar_row_capacity/sidebar_scrollbar_geometry（ui_geometry.rs；Win/Unix 共用）
│  ├─ [x] composer/workspace 可见性策略单点：FocusTransitionGate::composer_visible()（interaction.rs；Win/Unix 共用）
│  ├─ [ ] Win remote / Unix embedded 保留双主机，但 **共享交互语义** 只进
│  │     一处（ui_geometry / control_dispatch / 场景矩阵），禁止各写一套策略
│  ├─ [ ] `platform/mod.rs` 产品策略表 vs `agenterm-platform` 机制 再切割
│  ├─ [x] 文档：ARCHITECTURE SSOT + 指针；禁第二棵现行树
│  └─ [ ] 行为不一致只记「能力缺口」，不记「if windows {…} 产品分支」
│
├─ D. 已知测试/契约缺陷（见 §四基线）
│  ├─ [x] shared-memory：公共名长上限与 **所有** 单测/跨进程测一致
│  │     （`apm-{pid}-{nonce}` ≤31；本机 `shared_memory_process` PASS）
│  ├─ [ ] Windows process_spawn 测试中曾出现线程 panic 噪音（需复核是否
│  │     偶发/竞态；不得「绿了就当没有」）
│  └─ [ ] quick 绿 ≠ 六平台 CI / smoke / Candidate；补宿主矩阵门禁记录
│
└─ E. 体验小叶（可选，按用户痛点排序，不做大 CC）
   ├─ [ ] REPL 行编辑/history（若仍痛）
   ├─ [ ] macOS pointer：Unsupported 诊断更清楚或真机正向证据
   └─ [ ] Cockpit 只读事实/导航小步，不进 Workflows 内容
```

---

## 三、依赖顺序与证据

1. **先钉契约与测试一致性**（shared-memory 名长、失败码），再扩 facade。
2. **先收 frontend 边界半迁**，再谈新的 platform 搬家；热文件串行。
3. 路径/CC 截图/Capability：先审计调用者与静默 fallback，再合并 API。
4. 证据阶梯：`lint` → crate/all-feature tests → `check.cmd --quick` →
   归属 smoke →（仅定版）`--release --include-stress` / Candidate。
5. 接受的产品状态变更回写 owning `prd/PRD_*.md`；**文件地图与重构过程只留 plan/**。

设计约束（保留）：

- 平台原生选择只在 `agenterm-platform` 的 `selected.rs` / adapters；主 crate
  只保留 Agenterm 命名、workspace/instance policy 和产品 renderer glue。
- `Unsupported` / `Failed` 必须可观察；不能把权限、路径、解析或 native 失败
  改写成临时目录、默认平台或“可用”。
- 公共 contract 不泄漏 Win32/POSIX/第三方原生句柄。

明确非目标：

- 不在本版本扩展 net、WebView、Fleet 或 Control Center 大内容。
- 不重做已完成的 PTY/IPC/输入/窗口/Script Runtime 大迁移（除非修回归）。
- 不创建 tag/Candidate/Release；发布仍需独立 exact-SHA 授权链。
- 不以弱模型开放式「微重构」替代有边界的叶子任务。

---

## 四、本机基线测试（2026-08-02）

环境：Windows 工作区；命令在仓库根执行。  
目的：给 0.1.13 布点提供「当前树」事实，不是 Candidate 资格。

### 4.1 `.\check.cmd --quick` — **PASS**

| 阶段 | 结果 | 约时 |
|------|------|------|
| repository static lint | PASS | ~2.8s |
| rustfmt | PASS | ~3.9s |
| PRD capability alignment | PASS（62 catalog / 84 public names / 11 protocol / 41 mux / 65 capability / 100 evidence） | ~2.8s |
| all-target Clippy | PASS | ~1.0s |
| library unit tests | **530 passed**, 0 failed | ~1.6s（门禁总 ~13.5s） |

含义：主 crate 在 **Quick 车道** 健康；**不能**替代 remote-ui / control-center
smoke、六平台 CI、或 stress qualification。

### 4.2 `cargo test -p agenterm-platform --all-features` — **FAIL（集成测）**

| 套件 | 结果 |
|------|------|
| lib 单测 | 报告 **223 passed**（见下方噪音） |
| `tests/ipc_native.rs` | PASS |
| `tests/locking_process.rs` | PASS |
| `tests/process_containment_process.rs` | PASS |
| `tests/process_tree.rs` | PASS（0 tests） |
| **`tests/shared_memory_process.rs`** | **FAIL** |

#### D1 — shared_memory 跨进程名长契约不一致（**实锤，进 0.1.13**）

```text
test named_mapping_is_cross_process_and_released ... FAILED
parent creates mapping: SharedMemoryError {
  kind: InvalidName,
  detail: "name must be 1..=31 ASCII letters, digits, '.', '_' or '-'"
}
```

- 公共 `validate` 上限 **31**（`crates/agenterm-platform/src/shared_memory.rs`）。
- 集成测仍生成
  `agenterm-platform-process-map-{pid}-{nanos}` → **超长**。
- 单元测已缩短 `unique_name`（`a-{label}-…`），集成测未跟进 →
  **「单测绿、进程测红」**。
- 修复方向（择一写清契约）：
  1. 所有 fixture 统一 ≤31 的可移植名；或
  2. 若平台允许更长，放宽 validate 并在 Windows/POSIX 真机证明；
  禁止只改单测、不改集成测。

#### D2 — process_spawn 测试运行中的 panic 噪音（**待复核**）

同次 all-feature 跑中日志出现：

```text
thread 'selected::process_spawn::tests::explicit_handle_scope_restores_flags_during_unwind'
panicked at crates/agenterm-platform/src/adapters/windows/process_spawn.rs:223:70
```

但汇总仍写 lib **223 passed**。可能是：预期 panic 路径、子进程、或竞态被吞。
**0.1.13 动作**：单独 `cargo test -p agenterm-platform explicit_handle_scope -- --nocapture`
复核；若 flaky，修 RAII/继承恢复并加稳定证据，不靠「总通过数」。

### 4.3 未在本轮执行（明确欠账）

| 门禁 | 原因 |
|------|------|
| `check.cmd` 完整（含 public smoke） | 时长/GUI；发版前必做 |
| `check.cmd --release --include-stress` | Candidate 级；需 clean 定版 SHA |
| Linux/macOS native matrix | 本机 Windows；依赖 CI/宿主 |
| remote-ui-smoke / control-center-smoke | 归属 GUI/CC；0.1.13 叶子完成后串行 |

---

## 五、建议波次（执行投影）

```text
Wave 0（随时可做，挡信任 / 挡 CI）
└─ [x] 修 shared_memory 名长：契约 + unit + process 测同绿（本机亲测 PASS）

Wave 1（边界卫生 — 低风险）
├─ [x] 删除 orphan `services/frontend.rs`；ARCHITECTURE + boundary_tests 闸
├─ [ ] 清掉无根因的 allow(dead_code)/unused_imports
├─ [x] 文档：ARCHITECTURE SSOT；boundary-tree superseded；parity 指 SSOT
└─ [ ] frontend_server //! 与 CLI 委托关系复核

Wave 1b（分层主线 — 用户 UI/UX 微重构目标）
├─ [x] 去掉 frontend.rs 对 adapter 的 #[path] 虚树；固定 `platform::adapters` 声明
├─ [x] Win launcher 经 `windows::remote_frontend` 正规 sibling（非 path 魔法）
├─ [ ] 共享：parse/handoff/wake 结果码、snapshot 字段、geometry/hit-test
├─ [ ] 分叉：仅 PixelWindow vs ControlWindow 主机机制
└─ [ ] 每条可见 UX 差异 → 矩阵一行（Supported/Unsupported/Failed + 证据）

Wave 2（平台失败语义）
├─ paths 无静默 temp fallback
├─ CC screenshot 单一提供方（strategy 勿双源）
└─ Capability 映射去重；platform/mod 产品表瘦身

Wave 3（与 0.1.12 协作，不抢授权）
├─ 协助记录 sealed Candidate + Promotion 彩排所需的 tree 纪律
└─ 用户批准后的 0.1.12 Release 不由本 plan 触发

Wave 4（可选体验）
└─ REPL 编辑 / pointer 诊断 / Cockpit 小步
```

---

## 六、完成定义（0.1.13）

- §二目标树中接受的叶子均有证据；状态回写 owning PRD（若产品可见）。
- `agenterm-platform` **all-feature 含跨进程测** 全绿；主 crate Quick 全绿。
- 无半迁 orphan、无无说明的行为启发式进入 main。
- shared-memory（及同类）fixture 与 contract 上限一致。
- **跨平台分层**：无 `#[path]` 虚树；文档与模块树一致；Win/Unix 策略分叉
  可在证据矩阵逐条解释，而非散落 `if windows`。
- 不把 net/WebView/大 CC 写成 0.1.13 shipped。
- **不**因本文创建 `v0.1.13` tag；Candidate/Release 仍要独立 exact-SHA 授权。

---

## 七、与其它文档的关系

| 文档 | 关系 |
|------|------|
| **`plan/ARCHITECTURE.md`** | **现行结构 SSOT**（分层/bins/热文件/禁令/已知债务）；版本 plan 不重画全树 |
| `plan/plan-v0.1.12.md` | 0.1.12 收口与 Wave D；发布闭环权威执行记录 |
| `prd/PRD_02_17_delivery_quality.md` | Candidate/Promotion 合同 |
| `prd/PRD_02_18_roadmap.md` | M11/M12 路线状态 |
| `plan/platform-ui-ux-boundary-tree.md` | **历史过程文**（superseded）；只作叙事，不权威 |
| `plan/plan-unix-gui-win-parity.md` | Win↔Unix **可见行为**对齐地图（差距，非结构 SSOT） |
| `plan/platform-ux-parity-evidence-matrix.md` | 缺口矩阵模板 |
| `prd/PRD_*.md` | 仅当能力状态变化时回写；不写模块搬家流水账 |
| `src/platform/boundary_tests.rs` | 结构漂移闸（services 孤儿 + `#[path]` 预算） |

---

## 八、跨平台封装分层专项 review（`fa92bc8..1c582c7`）

> 动机：三 OS 的 UI/UX 差过大，用户要求微重构分层。目标不是「一个
> frontend 文件跑三端」，而是 **机制一层、产品语义一层、主机实现可替换**，
> 行为差只能落在可命名的能力缺口上。

### 8.1 目标分层（验收尺）

```text
agenterm-platform          机制：窗口/输入/截图/进程/IPC/字体…
                           typed Unsupported/Failed，无 AgenTerm 产品名

src/platform/*             产品平台 glue：目录名、实例布局、shell 标签、
                           CC 截图策略选择、快捷键 primary 策略表
                           （应是表驱动/薄，不是第三套 OS adapter）

src/frontend/mod.rs            产品入口：参数解析、handoff、统一结果码、
                           按 FrontendHost 分发到主机

src/frontend_server.rs     server 拉起/恢复（非 IPC 代理）

adapters/windows/*         Win 主机：replaceable UI + control window
adapters/unix/frontend/*   Unix 主机：embedded pixel window + 产品状态机

共享产品语义               ui_geometry / control_dispatch / ui_bridge /
                           ui_snapshot 字段 / 选区契约
```

**妥当**：分叉停在「主机如何画/如何收事件」。  
**不妥当**：分叉停在「点了 Tab 算不算选中」这类产品规则各写一份。

### 8.2 1c582c7 做成了什么（正向）

| 点 | 评价 |
|----|------|
| 启动参数 / help / 错误文案收敛到 `src/frontend/mod.rs` 共享策略 + policy 差异（Win `ui-client`/地址校验 vs Unix 关闭） | **对**：产品语义一处 |
| Win/Unix launcher 都 `use crate::frontend::{parse…, attempt_gui_handoff…}` | **对**：入口不再各解析一套 |
| `frontend_host()` 统一 host 判定；`run_gui_entry` / `request_gui_wake` 分发 | **对**：能力路由形状 |
| `GuiLaunchResult` / `GuiWakeResult` / `FrontendContractState` 统一失败类 | **对**：证据可归并 |
| `frontend_server` 抽 server 生命周期；CLI 委托 | **对**：减少 remote 内策略 |
| Unix adapter 内减少直接 `platform_kind` 分支（输入策略走 platform 表） | **对**：策略上移 |
| `platform/mod.rs` 集中一批 `is_windows_host` 产品表（字体默认、目录名、CC 截图策略等） | **方向对**，但是 **glue 过肥**（见 8.3） |
| adapters 目录内 **无** `platform_kind` 字符串匹配（抽查） | **好**：主机少产品 OS 枚举 |

### 8.3 分层问题清单（0.1.13 主攻）

#### L1 — `#[path]` 虚树（**高** → **已收本刀**）

~~`frontend.rs` 三处 `#[path]`~~ → `platform::adapters::{windows,unix}` 正规声明；
`frontend` 只 `use` host `frontend`。Win `super::remote_frontend` 现为真实 sibling。
`boundary_tests`：`FRONTEND_PATH_ATTR_BUDGET=0`。  
残留：~~`unix/frontend` 内对 `terminal_selection.rs` 的嵌套 `#[path]`~~ → 已收为 `src/frontend/selection` 共享模块（2026-08-03）。

#### L2 — 双 GUI 架构未收敛语义，仅收敛了启动皮（**高，UX 根因**）

| | Windows | Unix |
|--|---------|------|
| 形态 | replaceable **remote** UI client ↔ 独立 `agenterm-server` | **embedded** 窗口主机 + 同树巨石状态机 |
| 主机 | ControlWindow / GDI 路径（crate） | PixelWindow / winit+softbuffer（crate） |
| 产品状态 | remote_frontend 控制器 | `unix/frontend/mod.rs` 大状态机 |

启动参数对齐 **不够** 消掉「三 OS UX 差远」：差在 **主机 + 状态机双轨**。
`plan-unix-gui-win-parity.md` 几何/snapshot 多项已 `[x]`，仍有字体像素等与
能力缺口矩阵未填满项。

**0.1.13 目标**（不强迫一日合并双主机）：

- 强制 **共享管线**：事件 → 归一化 key/pointer → 同一 `ui-action`/selection/
  scroll 策略 → 同一 snapshot 字段。
- 主机只提供：present frame、native wake、IME/文本框能力、typed Unsupported。
- 任何 Win/Unix 可见差 → evidence matrix 一行，禁止 adapter 里安静的产品 if。

#### L3 — `platform/mod.rs` 变成产品策略垃圾桶（**中高**）

同文件混有：`FrontendHost`、workspace 布局、primary shortcut、empty-copy 抑制、
CC screenshot strategy、hosted script worker、atomic path、long-running fixture、
目录名大小写、默认字号……且大量 `#[allow(dead_code)]`。

- **好**：比散落三个 adapter 的 `if macos` 可测。
- **坏**：与 `agenterm-platform` 边界模糊；fixture 与用户默认同级；allow 掩盖
  未接线 API。

**0.1.13 目标**：拆 `policy/{input,paths,control_center,runtime,test_fixtures,workspace,host,script_http}`；八个 policy 表已落地（2026-08-03），每表单测；禁止新的顶层 `is_windows_host()` 蔓延。

#### L4 — 文档与代码漂移（**中** → 文档包已收）

| 曾写 | 现行 |
|------|------|
| boundary-tree 当结构权威 + `services/frontend` 路由 | **SSOT=`plan/ARCHITECTURE.md`**；boundary-tree=历史文 |
| unix-win-parity O1A：`services/frontend` | 实际 `src/frontend/mod.rs` + `platform::frontend_host` |

**剩余**：parity 正文里旧交付句可随叶改写；禁再开第二棵现行树。

#### L5 — orphan / 编译卫生（**中**）

- ~~`services/frontend.rs` orphan~~ → **已删**；`boundary_tests` 防再长。
- `frontend.rs` 对三个 `#[path]` 模块 `#[allow(dead_code)]`——跨 target「用
  不到另一套」用 allow 捂住；正牌是 `cfg` 只编当前主机或统一 trait。
  `#[path]` 数量闸在 budget=3（L1 完成后改为 0）。

#### L6 — 夹带的非分层改动（**中，纪律**）

同 commit 混入：macos cache EINVAL、host_memory count 放宽、unix window
**负 y 取反**、shared_memory 单测缩名但 process 测未修、qualification 微调等。

- 与 UI/UX 分层 **正交或危险**。
- 纪律：**分层 PR 禁止夹带 OS 行为补丁**；行为补丁单独叶 + 证据。

#### L7 — 测试门与分层证明不足（**中**）

- Quick 530 ≠ 三 OS UX 对齐。
- parity-smoke 矩阵带 platform 字段方向对，未替代宿主 journey。
- `shared_memory_process` 仍红 → 平台契约不完整。

### 8.4 分层是否「妥当」——总判

| 层级 | 妥当度 | 一句话 |
|------|--------|--------|
| `agenterm-platform` 机制下沉 | **较好** | 大迁移多在 0.1.12；本 commit 夹带需审 |
| 产品入口（参数/handoff/结果码） | **明显进步** | 微重构该做的「皮」 |
| 主机 adapter 物理/逻辑归属 | **不妥当** | `#[path]` 虚树 + `super` 魔法 |
| Win vs Unix **交互语义** 单一事实 | **仍 partial** | 双架构仍在；对齐靠另一 plan |
| `platform/mod` 产品策略 | **半妥当** | 集中了，但肥且 allow 多 |
| 文档/孤儿/测试契约 | **不妥当** | 三套真相 + process 测红 |

**给下一轮 agent 的一句话**：  
不要再「unify ingress」大包大揽；按 **L1 去 path → L4 文档/孤儿 → L2
场景矩阵一条** 做窄叶。UX 差远的根因在 **双主机语义未单点化**，不在
usage 字符串有没有共用。

### 8.5 建议的目标模块树（0.1.13 收口后）

```text
src/frontend/mod.rs                 # 仅：parse, handoff, result types, dispatch
src/frontend/action.rs              # canonical action identities
src/frontend/toolbar.rs             # 产品 toolbar action 映射
src/frontend/window.rs              # 产品窗口语义（client-size / semantic state）
src/frontend/control_center.rs    # Control Center 产品 facade（native 能力仍走 platform services）
src/frontend_server.rs          # server autostart/recovery only

src/platform/
  mod.rs                        # re-export 薄 + FrontendHost
  policy/                       # 产品策略表（cfg-free）
    input.rs                    # 已拆：shortcut / empty-copy
    control_center.rs           # 已拆：CC screenshot strategy
    runtime.rs                 # 已拆：hosted worker / test host
    test_fixtures.rs           # 已拆：long-running fixtures
    paths.rs                   # 已拆：目录/workspace/IPC workspace
    workspace.rs               # 已拆：workspace directory layout policy
    host.rs                    # 已拆：host predicates / shell command
    script_http.rs             # 已拆：Script Runtime HTTP TLS policy
  adapters/
    windows/
      mod.rs                    # 正规 mod，无 #[path] 从 frontend 刺入
      launcher.rs
      remote_frontend.rs
    unix/
      frontend/
  services/                     # 无 orphan；无第二套 frontend 路由

crates/agenterm-platform/       # 只机制
```

证据：boundary regression 断言「frontend 不 path 进 adapters」「services 无
frontend 死文件」；parity smoke 按场景 ID 出 Supported/Unsupported。

