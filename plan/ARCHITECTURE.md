# AgenTerm architecture map（现行结构 SSOT）

状态：active（2026-08-03）  
权威范围：**代码分层、入口、所有权、禁令**。  
非权威：发版资格、能力 shipped 状态（见 `prd/`）、波次任务列表（见 `plan/plan-v0.1.*.md`）。

> **抗漂规则**：全仓库只维护 **这一份** 现行结构图。其它 `plan/*` 只链到本文，禁止再画第二棵「现行」树。  
> 结构变更与本文冲突时：同批改本文，或改代码；禁止第三现实。  
> 自动闸：`src/platform/boundary_tests.rs`（含 services 孤儿检测；`frontend` `#[path]` 债务计数）。

---

## 1. 分层（验收尺）

```text
crates/agenterm-platform     机制：窗口/输入/截图/进程/IPC/PTY/字体/shm…
                             typed Unsupported / Failed；无 AgenTerm 产品名

src/platform/                产品平台 glue：目录名、快捷键/CC、能力/IPC 命名（FrontendHost 见 src/frontend/host.rs）
  policy/                    host 无关产品策略表
    input.rs                 shortcut / empty-copy 输入策略（Win/Unix 共用）
    control_center.rs         CC screenshot 策略（Win/Unix 共用）
    paths.rs                 product path naming / workspace / IPC workspace
    workspace.rs             workspace directory layout policy
    host.rs                   host predicates / shell command routing
    capability.rs             product capability status / platform_info JSON
    ipc.rs                    native IPC endpoint naming policy
    script_http.rs            Script Runtime HTTP TLS provider/root policy
    runtime.rs               hosted worker / test host 默认
    test_fixtures.rs         long-running process fixtures
                             策略表、services facade（应薄，勿第三套 OS adapter）

src/frontend/                产品 GUI 入口 + UI/UX 语义
  mod.rs                     parse / handoff / 统一结果码 / dispatch
  action.rs                  canonical action identities（toolbar/shortcut 共用）
  host.rs                    GUI host selection（Win/Unix/Unsupported）
  toolbar.rs                 toolbar action 映射（Win/Unix 共用）
  window.rs                  client-size / window semantic state（Win/Unix 共用）
  interaction.rs             focus navigation / wheel accumulation / wheel routing / raw-mouse arbitration / scrollbar thumb drag / modal focus gate（Win/Unix 共用）
  control_center.rs         Control Center 产品 facade（native 能力仍走 platform services）

src/frontend_server.rs       server 拉起 / 恢复（非 IPC 代理）

src/ui_*.rs + control_*      共享产品语义：geometry / snapshot / bridge /
                             clipboard / dispatch / terminal_selection（Win/Unix 应对齐字段）

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
| `agenterm` | `src/bin/agenterm.rs` | GUI 启动器 |
| `agenterm-server` | `src/bin/agenterm-server.rs` | 工作区/PTY/事件权威（可替换 UI 的头less） |
| `agenterm-cli` | `src/bin/agenterm-cli.rs` | 控制平面 CLI |
| `agenterm-cc` | `src/bin/agenterm-cc.rs` | Control Center 投影 |
| `agenterm-rhai` | `src/bin/agenterm-rhai.rs` | 本地 Rhai 运行时（无权限策略） |
| `agenterm-mcp` | `src/bin/agenterm-mcp.rs` | 只读 MCP sidecar |
| `agenterm-mux` | `src/bin/agenterm-mux.rs` | Fleet multiplexer 前端 |

Cargo 版本号见根 `Cargo.toml`（与公开 tag 可能暂时脱节——发版以 Candidate/Release 链为准）。

---

## 3. 热文件（改前先认主）

| 区域 | 路径 | 备注 |
|------|------|------|
| GUI ingress | `src/frontend/`, `src/frontend_server.rs` | 参数/唤醒/结果码 |
| 共享 UX | `src/ui_geometry.rs`, `src/ui_snapshot.rs`, `src/ui_bridge.rs`, `src/control_dispatch.rs` | 对齐契约 |
| 产品策略表 | `src/platform/mod.rs` | 已知偏肥；0.1.13 目标拆 `policy/` |
| Win 主机 | `src/platform/adapters/windows/{frontend,remote_frontend}.rs` | remote 客户端 |
| Unix 主机 | `src/platform/adapters/unix/frontend/` | embedded 状态机 |
| 机制 crate | `crates/agenterm-platform/src/{selected,window,input,ipc,pty,process,shared_memory}.rs` | 无产品名 |
| 边界闸 | `src/platform/boundary_tests.rs` | 原生边界 / 结构漂移 |

---

## 4. 已知结构债务（勿当「已修好」）

摘自 `plan/plan-v0.1.13.md` 审查；**修债务时更新本节与对应叶**。

| ID | 现状 | 目标 |
|----|------|------|
| L1 | ~~`frontend.rs` `#[path]` 虚树~~ | **已收**：`platform::adapters::{windows,unix}` 正规 mod；`frontend` 只 `use` |
| L1b | ~~`windows/frontend` 靠 sibling `#[path]`~~ | **已收**：同目录 `windows::{frontend,remote_frontend}` |
| L2 | Win remote vs Unix embedded 双主机（selection/focus/wheel/scrollbar-drag 已共享） | 共享交互语义单点；主机只 present/wake/IME |
| L3 | `platform/mod.rs` 策略过肥（input/paths/control_center/runtime/test_fixtures/workspace 已拆 `policy/`；FrontendHost 与 facade 是剩余薄层）+ `allow(dead_code)` | `policy/{input,paths,control_center,runtime,test_fixtures,workspace,host,script_http,capability,ipc}` 全拆；禁新顶层 `is_windows_host` 蔓延 |
| D1 | shared_memory 名长 ≤31 | **本机已绿**：unit + `shared_memory_process` 名式 `apm-…` ≤31 |

已清理：`src/platform/services/frontend.rs` 孤儿 re-export（无人 `mod`）——删除；入口以 `src/frontend/` 为准。

---

## 5. 文档谁说了算

| 问题 | 看哪里 |
|------|--------|
| 代码现在怎么分层？ | **本文** |
| 本版要修哪些叶？ | `plan/plan-v0.1.13.md`（及后续版本 plan） |
| 能力是否 shipped / 验收？ | owning `prd/PRD_*.md` + `prd/alignment-contract.json` |
| Win↔Unix 可见行为差距？ | `plan/plan-unix-gui-win-parity.md` + evidence matrix（**差距地图，不是结构 SSOT**） |
| Agent 操作纪律？ | `AGENTS.md` |
| 产品总树？ | `PRD.md` |

历史过程文（如旧 boundary-tree 长叙事）若与本文冲突：**以本文 + 代码 + boundary_tests 为准**。

---

## 6. Agent 禁令（短）

1. 不要在 adapter 里新写产品策略 `if windows` / `if unix`；策略进共享管线或 `platform` 表。  
2. 不要静默把 `Failed`/`Unsupported` 改成 temp 路径或「假装可用」。  
3. 不要在 `agenterm-platform` 引入 `agenterm::` / `AGENTERM_` 产品耦合（已有测）。  
4. 不要新增第二套 GUI 启动解析或第二套 server autostart 决策。  
5. 不要把 net / WebView / 大 Control Center 内容写进「已 shipped」除非 owning PRD 已改。  
6. 结构变更：更新本文；版本 plan 只记叶与证据，不重画全树。

---

## 7. 验证入口（本地）

```text
.\check.cmd --quick          # lint + 主 crate 单测（含 boundary_tests）
cargo test -p agenterm-platform --all-features   # 含跨进程；shm 名长已知红见 D1
```

Quick 绿 ≠ 六平台 CI / Candidate。
