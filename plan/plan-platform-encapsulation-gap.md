# platform 封装漏点表（机制债）

状态：active（2026-08-06 · goal-crate-platform 加深）  
范围：**仅 OS 机制**（应进 `crates/agenterm-platform` 的能力）+ catalog **归属**误标（产品闸，非 OS API）。  
产品可见行为差距见 [`plan-unix-gui-win-parity.md`](plan-unix-gui-win-parity.md)。  
边界 SSOT：[`ARCHITECTURE.md`](ARCHITECTURE.md) §1.0、[`goal-crate-platform.md`](goal-crate-platform.md)。

## 证据基线

| 检查 | 结果 | 路径 |
|------|------|------|
| 产品 `src/**` 禁止 `windows_sys`/winit/x11/… | **PASS** | `platform::boundary_tests::production_sources_use_platform_crate_as_the_only_native_boundary` |
| platform 无 `AGENTERM_` / `agenterm::` 产品耦合 | **PASS**（既有） | `boundary_tests` product-coupling on crate |
| 散装 breakaway / ACCESS_DENIED=5 | **closed G1** + 回归测 | `spawn_breakaway_visible_*`；`process::tests::product_sources_do_not_hardcode_breakaway_access_denied` |
| catalog 误把 shared dispatch 标成 WINDOWS_ONLY | **closed G6** | 24 id 升 SHARED；`windows_only_is_not_implemented_in_shared_dispatcher` |
| `open-new-terminal` 仅 Unix 一等动词 | **closed G7**（最小） | Win remote 接线 `open_new_terminal()`；升 SHARED |

## 漏点 / 收口表

| ID | site | should live in | priority | notes / parity-gap? | status |
|----|------|----------------|----------|---------------------|--------|
| G1 | `control_center` / `remote_frontend` 手写 breakaway denial | `process`：`spawn_breakaway_visible_*` | P0 | 产品不得认识 `ERROR_ACCESS_DENIED` 数值 | **closed** |
| G2 | `script_process` / `worker_supervisor` `Command::spawn` | 已用 `configure_command` / `configure_worker_command` + `ProcessTreeGuard` | P2 | **审计结论（2026-08-06）**：spawn 前均走 platform 配置/树守卫；裸 `spawn()` 是 std 出口，**不**算散装 Job/flags 语义。无强制迁 `spawn_detached_*`（stdio 管道语义不同） | **no leak / 文档证伪** |
| G3 | Unix softbuffer → PNG | `screenshot` 编码 + host present 像素 | P3 | present 合法；非产品层 OS API | open（低） |
| G4 | Win remote control-window 巨石 | `window` control host | P2 | L2 结构债，非 boundary 泄漏 | open（阻塞双拓扑） |
| G5 | IME / clipboard / activation | 已 `agenterm_platform::*` | — | 抽查 OK | **no leak** |
| G6 | `ui_action_catalog` 把 `control_dispatch` 已实现的 24 个动词标 WINDOWS_ONLY | catalog 归属 | P0 | 并发 review：Unix 经 `dispatch_shared_command` **先**处理 `ui-action`，真共享 | **closed**：升 SHARED；闸防回退 |
| G7 | `open-new-terminal` UNIX_ONLY | 两端 ui-action | P1 | Win 已有 `open_new_terminal()` / 工具栏 NEW_TAB | **closed 最小**：Win 增加同名 ui-action；shell/create 仍 UNIX_ONLY |

## 书面结论

1. **OS 边界闸有效**：产品层几乎没有直接 native markers。  
2. **机制语义泄漏** 以 G1 为代表（错误码/创建 flags 散落）——已收 + 回归。  
3. **catalog 书账错误** 比「Unix 没实现」更危险（G6）——会低估已共享面、诱使重复移植。  
4. **真·host-only 残余**（instance strip / 部分 settings chrome / font-locale / Unix dialog 字段动词）见 catalog `parity-gap:` 注释。

## Agent 执行句式（跨平台任务强制）

1. 判定：platform **机制** / frontend **产品语义** / host **present**？  
2. 机制 → 改 `crates/agenterm-platform`，feature 与 typed Unsupported 诚实更新。  
3. 产品 → 改 `src/frontend/*` + `ui_action_catalog`，再改 **两端** adapter（或确认 `control_dispatch` 已覆盖）。  
4. 仅 host → 只动对应 adapter，并登记 catalog allowlist 或本表。  
5. 证据：`cargo test -p agenterm-platform --features process`（相关）+ `cargo test --lib ui_action_catalog` + `cargo test --lib product_sources_do_not_hardcode_breakaway`；无证据不宣称三端手感已齐。

## 下一刀候选（不自动开工）

1. settings scope chrome / font-locale 升 SHARED 或诚实保持 ONLY。  
2. instance-picker / server-strip Unix 产品叶（S′ 排期）。  
3. L2 remote/embedded 收敛或 action 表驱动（plan §九 刀5）。  
4. Unix dialog `create` / shell-* 是否要对 Win 暴露自动化 id。
