# AgenTerm architecture map（现行结构 SSOT）

状态：active（2026-08-05；对齐机制/工具边界见 §8）  
权威范围：**代码分层、入口、所有权、禁令、结构如何被勾住**。  
非权威：发版资格、能力 shipped 状态（见 `prd/`）、波次任务列表（见 `plan/plan-v0.1.*.md`）、封装/复用改进建议的执行排期（版本 plan 记叶，不在本文重画）。

> **抗漂规则**：全仓库只维护 **这一份** 现行结构图。其它 `plan/*` 只链到本文，禁止再画第二棵「现行」树。  
> 结构变更与本文冲突时：同批改本文，或改代码；禁止第三现实。  
> 自动闸（**局部**，非全文双向）：`src/platform/boundary_tests.rs`。  
> 历史过程文 `plan/platform-ui-ux-boundary-tree.md` = **superseded**，不得当现行权威。

---

## 1. 分层（验收尺）

```text
crates/agenterm-platform     机制：窗口/输入/截图/进程/IPC/PTY/字体/shm…
                             typed Unsupported / Failed；无 AgenTerm 产品名

src/platform/                产品平台 glue：FrontendHost、目录名、快捷键/CC、能力/IPC 命名
  policy/                    host 无关产品策略表
    input.rs                 shortcut / empty-copy 输入策略（Win/Unix 共用）
    control_center.rs         CC screenshot 策略（Win/Unix 共用）
    paths.rs                 product path naming / workspace / IPC workspace
    workspace.rs             workspace directory layout policy
    host.rs                   host predicates / shell command routing
    capability.rs             product capability status / platform_info JSON
    ipc.rs                    native IPC endpoint naming policy
    script_http.rs            Script Runtime HTTP TLS provider/root policy
    runtime.rs               hosted worker / test host / new-terminal shell argv 默认
    test_fixtures.rs         long-running process fixtures
                             策略表、services facade（应薄，勿第三套 OS adapter）

src/frontend/                产品 GUI 入口 + UI/UX 语义
  mod.rs                     parse / handoff / 统一结果码 / dispatch
  action.rs                  canonical action identities（toolbar/shortcut 共用）
  toolbar.rs                 toolbar action 映射（Win/Unix 共用）
  window.rs                  client-size / window semantic state（Win/Unix 共用）
  interaction.rs             focus navigation / wheel accumulation / wheel routing / scrollbar thumb drag / modal/focus state + modal surface priority/snapshot naming + FocusSurface canonical names/IPC aliases（FocusState + adapter focus_gate() + ModalSurface/modal_surface_from_gate() + FocusSurface::as_str()/from_ipc()，Win/Unix 共用）；raw-mouse arbitration/report outcome 策略与 xterm mouse report 编码器（Unix embedded 与 Windows remote 共用）；alternate-screen wheel fallback 用 commands::alternate_screen_wheel_bytes 单点编码
  composer.rs                ComposerWriteMode（empty-only/append/replace）单点定义，embedded、remote UI、server dispatch 共用
  cwd_editor.rs             CWD editor modal 状态/action/snapshot 单点；Unix embedded 与 Windows remote 共用 CwdEditorDialog，adapter 只保留原生编辑控件/焦点与命令执行
  input.rs                  keyboard/composer/tab-editor/terminal-shortcut 输入语义单点；Unix embedded adapter 经 `frontend::input` 引用，Windows remote 保留原生控件映射
  new_terminal.rs           new-terminal modal 状态/校验/action 单点；Unix embedded 使用共享 dialog，Windows remote 仍用原生控件呈现，状态/校验/action/argv 与 Unix 共用共享 dialog
  settings.rs              settings modal 状态/校验/action 单点；Unix embedded 与 Windows remote 共用 SettingsDialog，adapter 只负责原生呈现/事件映射
  close_confirmation.rs    live-tab close confirmation 状态/快照单点；Unix embedded 与 Windows remote 共用 CloseConfirmation，adapter 只保留原生确认控件与关闭执行
  tab_editor.rs            inline tab editor 状态/校验/快照单点；Unix embedded 与 Windows remote 共用 TabEditorDialog，adapter 只保留原生编辑控件/IME/事件映射
  window_close.rs          window-close 状态/choice/snapshot 单点；Unix embedded 与 Windows remote 共用 WindowCloseDialog/WindowCloseChoice，adapter 只保留原生窗口执行与按钮呈现
  selection.rs               线性选区 / autoscroll / word-boundary 语义（SelectionGesturePhase + 泛型 SelectionGestureState<TabId, Point> 单份定义；TerminalCellSource + word_selection_bounds 让 vt100 与 snapshot cell grid 共用；Unix embedded 与 Windows remote 共用状态机、autoscroll_step）
  control_center.rs         Control Center 产品 facade（native 能力仍走 platform services）

src/frontend_server.rs       server 拉起 / 恢复（非 IPC 代理）

src/ui_*.rs + control_*      共享产品语义：geometry / snapshot / bridge /
                             clipboard / dispatch（terminal selection 语义已归 src/frontend/selection.rs）

src/platform/adapters/       主机实现（物理目录）
  windows/                   replaceable remote UI ↔ agenterm-server
  unix/frontend/             embedded 窗口 + 产品状态机
  linux|macos/               契约/manifest 等（非第二套业务策略）
```

**妥当**：分叉停在「主机如何画 / 如何收事件」。  
**不妥当**：分叉停在「点了 Tab 算不算选中」——产品规则只应有一份。

---

## 2. 可执行入口（bins）

| 二进制 | 路径 | 角色 |
|--------|------|------|
| `agenterm` | `src/bin/agenterm.rs` | GUI 启动器；子命令 `server` 进入无窗权威 |
| `agenterm-server` | `src/bin/agenterm-server.rs` | Windows 镜像隔离别名（同 `agenterm server`）；autostart 仍用此 PE |
| `agenterm-cli` | `src/bin/agenterm-cli.rs` | 控制平面 CLI |
| `agenterm-cc` | `src/bin/agenterm-cc.rs` | Control Center 投影 |
| `agenterm-rhai` | `src/bin/agenterm-rhai.rs` | 本地 Rhai 运行时（无权限策略） |
| `agenterm-mcp` | `src/bin/agenterm-mcp.rs` | 只读 MCP sidecar |
| `agenterm-mux` | `src/bin/agenterm-mux.rs` | Fleet multiplexer 前端 |

Authority entry plan: [`plan/plan-agenterm-server-mode.md`](plan-agenterm-server-mode.md)。

Cargo 版本号见根 `Cargo.toml`（与公开 tag 可能暂时脱节——发版以 Candidate/Release 链为准）。

---

## 3. 热文件（改前先认主）

| 区域 | 路径 | 备注 |
|------|------|------|
| GUI ingress | `src/frontend/`, `src/frontend_server.rs` | 参数/唤醒/结果码 |
| 共享 UX | `src/ui_geometry.rs`, `src/ui_snapshot.rs`, `src/ui_bridge.rs`, `src/control_dispatch.rs` | 对齐契约 |
| 产品策略表 | `src/platform/mod.rs` + `policy/` | policy 已拆；facade/`allow(dead_code)` 半迁移见 L3 |
| Win 主机 | `src/platform/adapters/windows/{frontend,remote_frontend}.rs` | remote 客户端；`remote_frontend` 巨石见 L2 |
| Unix 主机 | `src/platform/adapters/unix/frontend/` | embedded 状态机；`mod`/`render` 巨石见 L2 |
| 机制 crate | `crates/agenterm-platform/src/{selected,window,input,ipc,pty,process,shared_memory}.rs` | 无产品名 |
| 边界闸 | `src/platform/boundary_tests.rs` | 规则见 §8.2；**不**解析本文全文 |

---

## 4. 已知结构债务（勿当「已修好」）

摘自 `plan/archive/plan-v0.1.13.md` 审查；**修债务时更新本节与对应叶**。

| ID | 现状 | 目标 |
|----|------|------|
| L1 | ~~`frontend.rs` `#[path]` 虚树~~ | **已收**：`platform::adapters::{windows,unix}` 正规 mod；`frontend` 只 `use` |
| L1b | ~~`windows/frontend` 靠 sibling `#[path]`~~ | **已收**：同目录 `windows::{frontend,remote_frontend}` |
| L2 | Win remote vs Unix embedded 双主机（selection/focus/wheel/scrollbar-drag 已共享；`ui-action` 大 match 与巨石 adapter 仍双写） | 共享交互语义单点；主机只 present/wake/IME；action 表驱动记版本 plan 讨论叶 |
| L3 | `platform/mod.rs` 策略过肥（input/paths/control_center/runtime/test_fixtures/workspace 已拆 `policy/`；FrontendHost 与 facade 是剩余薄层）+ `allow(dead_code)` | `policy/*` 全拆收口；禁新顶层 `is_windows_host` 蔓延；半迁移 facade 二选一（全接线或删） |
| L4 | **结构 SSOT 未机读双向**（本文 prose + 局部 `boundary_tests`；目录树/分层文案漂移靠人） | 见 §8.4；版本 plan **S 组**执行；本文只定契约 |
| D1 | shared_memory 名长 ≤31 | **本机已绿**：unit + `shared_memory_process` 名式 `apm-…` ≤31 |

已清理：`src/platform/services/frontend.rs` 孤儿 re-export（无人 `mod`）——删除；入口以 `src/frontend/` 为准。

---

## 5. 文档谁说了算

| 问题 | 看哪里 |
|------|--------|
| 代码现在怎么分层？ | **本文** |
| 结构如何被自动勾住 / 工具边界？ | **本文 §8** |
| 本版要修哪些叶？ | 当前版本 `plan/plan-v0.1.*.md`（结构机读化 → **S 组**） |
| 能力是否 shipped / 验收？ | owning `prd/PRD_*.md` + `prd/alignment-contract.json` + `scripts/rhai/prd-alignment.rhai`（**能力**对齐，**不是**结构树） |
| Win↔Unix 可见行为差距？ | `plan/plan-unix-gui-win-parity.md` + evidence matrix（**差距地图，不是结构 SSOT**） |
| Agent 操作纪律？ | `AGENTS.md` |
| 产品总树？ | `PRD.md` |
| 旧 boundary-tree 叙事？ | `plan/platform-ui-ux-boundary-tree.md`（**superseded**） |

历史过程文若与本文冲突：**以本文 + 代码 + boundary_tests 为准**。

---

## 6. Agent 禁令（短）

1. 不要在 adapter 里新写产品策略 `if windows` / `if unix`；策略进共享管线或 `platform` 表。  
2. 不要静默把 `Failed`/`Unsupported` 改成 temp 路径或「假装可用」。  
3. 不要在 `agenterm-platform` 引入 `agenterm::` / `AGENTERM_` 产品耦合（已有测）。  
4. 不要新增第二套 GUI 启动解析或第二套 server autostart 决策。  
5. 不要把 net / WebView / 大 Control Center 内容写进「已 shipped」除非 owning PRD 已改。  
6. 结构变更：更新本文；版本 plan 只记叶与证据，不重画全树。  
7. 不要把 rust-analyzer / 通用 LSP 当成「结构 SSOT 已对齐」的证据；LSP 不消费本文。  
8. 不要新开第二份「现行结构图」md；扩展对齐能力只加闸/机读清单并回写 **本节/§8**。

---

## 7. 验证入口（本地）

```text
.\check.cmd --quick          # lint + 主 crate 单测（含 boundary_tests）
cargo test -p agenterm --lib platform::boundary_tests   # 结构红线闸（路径以实际 module 为准）
cargo test -p agenterm-platform --all-features   # 含跨进程；shm 名长已知红见 D1
```

Quick 绿 ≠ 六平台 CI / Candidate。  
Quick 绿 **≠** 「ARCHITECTURE.md 与目录树全文一致」（见 §8）。

---

## 8. 结构如何被勾住（对齐机制 · 工具边界 · 升级路径）

> 沉淀自 2026-08-05 结构 review / 工具澄清。**契约在本文**；实现排期在版本 plan **S 组**。

### 8.1 三角关系（今日真相）

```text
plan/ARCHITECTURE.md     人读结构 SSOT（分层/禁令/债务）—— 权威叙述
        │
        │  人维护；无解析器读全文
        ▼
src/** + crates/**       真实模块树与所有权
        │
        │  cargo test 跑局部规则
        ▼
boundary_tests.rs        结构红线闸（不是全文 diff 引擎）
```

| 组件 | 角色 | 是否「双向」 |
|------|------|----------------|
| 本文 | 现行结构叙述 SSOT | 否（人手） |
| `boundary_tests` | 代码侧可机检红线 | **单向：代码规则** |
| `prd-alignment.rhai` | PRD 能力/证据/命令目录 | **另一轴**，非结构树 |
| rust-analyzer (LSP) | 跳转/补全/重命名 | **编辑助手**，不校验分层 |

**结论**：已有「钩」，但是 **局部自动 + 全局靠纪律**；**未能**做到「改 md 自动约束代码 / 改目录自动改 md」的全自动双向对齐。

### 8.2 `boundary_tests` 今日覆盖（勾住了什么）

| 测项（概念） | 勾住的结构意图 |
|--------------|----------------|
| 产品 `src/**` 禁原生 marker / `cfg(target_*)` | 原生边界只在 `crates/agenterm-platform` |
| platform crate 禁产品耦合 marker | 机制 crate 无 AgenTerm 产品名/路径 |
| adapters 同契约 declaration | 三 OS adapter 合同形状一致 |
| `services/*` 无 orphan 源文件 | 防再长已删的 `services/frontend` 类 |
| `frontend` `#[path]` 预算 = 0 | L1 债务不回潮 |

**未覆盖（故会漂）**：§1 目录/分层 prose、§2 bins 表与 `src/bin/*` 一致性、巨石文件行数、Win/Unix `ui-action` 表是否同一 ActionId 集、policy/services 半迁移是否收口、本文债务表 L* 是否过时。

### 8.3 工具地图（别用错层）

| 层级 | 代表工具 | 与结构 SSOT 的关系 |
|------|----------|-------------------|
| LSP | rust-analyzer | 写代码顺手；**不**消费本文、**不**当对齐证据 |
| 构建 | `cargo check` / `cargo test` | 模块能编过；orphan `mod` 会红 |
| **本仓结构闸** | `boundary_tests` | **唯一官方结构红线机闸** |
| 能力对齐 | `prd-alignment.rhai` + alignment-contract | shipped/证据，**非**分层树 |
| 静态分析 | clippy / 可选 semgrep·ast-grep | 可补模式禁令；非 SSOT |
| 依赖图 | `cargo-modules` / depgraph 等 | 发现巨石与环；**辅助**，不替代本文 |
| 文档生成 | 自写 tree 脚本 / rustdoc | 可做 **代码→文档片段** |

结构工作 = **约定文档（本文）+ 测试/脚本闸 +（可选）依赖图**；不是「装个 LSP 插件」。

### 8.4 升级路径（要真·双向时）

自由 prose MD ↔ 任意 Rust **无法**可靠全文双向。可机读路径：

```text
A 扩 boundary_tests（单向规则）     必存在/禁路径、软行数预算、ActionId 完备性…
B 代码→文档围栏（半自动）           扫树生成 ```structure 块；CI diff 本文围栏
C manifest 真源（推荐长期）         architecture.manifest.{toml,json}
                                    → 生成 ARCHITECTURE 可机读块 + 同一清单喂测试
```

| 档 | 做到什么 | 仍靠人 |
|----|----------|--------|
| A | 红线不破 | 叙事/分层解释 |
| A+B | 目录树不静默漂 | 禁令语义措辞 |
| C | 改清单驱动文档+闸 | 清单本身的产品决策 |

**禁止**：再立第二棵「现行结构」md 冒充双向；扩展只加闸/机读清单并回写本文。

### 8.5 与封装/复用 review 的关系

巨石拆分、`ui-action` 表驱动、client 切分等 **改进建议** 不写入本文执行清单（防第二现实）。  
债务钩子：**L2**（双主机/巨石）、**L3**（policy/facade）、**L4**（SSOT 机读）。  
执行叶：当前版本 plan（如 `plan-v0.1.15` **S 组** + **§九 预备树**）；落地后 **同批** 更新本文 §1/§3/§4。  
**HOLD**：多 agent 并行时 S 泳道不写主树；用户通知复审后再按 §九 刀序开工。不必等 S3 全文双向才微重构。
