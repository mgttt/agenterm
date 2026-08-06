# platform 封装漏点表（机制债）

状态：**goal-crate-platform 封装收口完成**（2026-08-06；以 goal 成功标准为准，非 UX 全齐）  
范围：**仅 OS 机制**（应进 `crates/agenterm-platform`）+ catalog **归属**（产品闸）。  
产品可见行为差距见 [`plan-unix-gui-win-parity.md`](plan-unix-gui-win-parity.md)。  
边界 SSOT：[`ARCHITECTURE.md`](ARCHITECTURE.md) §1.0、[`goal-crate-platform.md`](goal-crate-platform.md)。

## 封装完结定义（contract）

| 完结 | 非目标（故意不完） |
|------|-------------------|
| OS 差异停在 platform crate；产品 boundary/breakaway 闸绿 | 三端工作台像素级手感齐 |
| catalog SHARED 诚实；`control_dispatch` 不误标 WINDOWS_ONLY | remote vs embedded 单拓扑 |
| 机制高价值漏点收口或书面证伪 | strip/picker/settings-scope 全量 Unix |
| residual host-only 显式 `parity-gap:` | 工作台剧本进 platform（伤 wbox） |

## 证据基线

| 检查 | 结果 | 路径 |
|------|------|------|
| 产品 `src/**` 禁止 `windows_sys`/winit/x11/… | **PASS** | `platform::boundary_tests::production_sources_use_platform_crate_as_the_only_native_boundary` |
| platform 无 `AGENTERM_` / `agenterm::` 产品耦合 | **PASS** | `boundary_tests` product-coupling on crate |
| 散装 breakaway / ACCESS_DENIED=5 | **closed G1** + 回归测 | `spawn_breakaway_visible_*`；`process::tests::product_sources_do_not_hardcode_breakaway_access_denied` |
| catalog 误标 shared dispatch | **closed G6** | 24 id 升 SHARED；`windows_only_is_not_implemented_in_shared_dispatcher` |
| `open-new-terminal` | **closed G7** | 两端 SHARED |
| toolbar font/locale 仅 Win ui-action | **closed G8** | Unix 已有方法；补 `ui-action` 臂并升 SHARED |

## 漏点 / 收口表

| ID | site | should live in | priority | notes / parity-gap? | status |
|----|------|----------------|----------|---------------------|--------|
| G1 | breakaway denial 硬编码 | `process`：`spawn_breakaway_visible_*` | P0 | 产品不得认识 `ERROR_ACCESS_DENIED` | **closed** |
| G2 | script/worker `Command::spawn` | 已 `configure_*` + `ProcessTreeGuard` | P2 | 非 Job-flags 泄漏 | **no leak / 证伪** |
| G3 | Unix softbuffer → PNG | screenshot 编码 + present | P3 | present 合法 | **out-of-goal residual**（低） |
| G4 | Win remote control-window 巨石 | window control host | P2 | L2 结构债 | **out-of-goal residual**（双拓扑） |
| G5 | IME/clipboard/activation | `agenterm_platform::*` | — | 已走 facade | **no leak** |
| G6 | 24 verbs 误标 WINDOWS_ONLY | catalog | P0 | 真共享在 `control_dispatch` | **closed** |
| G7 | `open-new-terminal` | 两端 ui-action | P1 | Win 接线 | **closed** |
| G8 | `font-*` / `toggle-locale` | 两端 ui-action | P1 | Unix 方法已在；补 ui-action 臂 | **closed** |

## 书面结论

1. **goal-crate-platform 成功标准已满足**（边界 SSOT + gap 表 + catalog 诚实闸 + 机制收口/证伪 + 测试）。  
2. 残余 **14** WINDOWS_ONLY = instance/strip + settings-scope chrome（产品叶，非 OS 泄漏）。  
3. 残余 UNIX_ONLY = new-terminal 字段/shell 动词（Win 原生控件 present）。  
4. G3/G4 标为 **out-of-goal residual**，不阻塞封装完结。

## Agent 执行句式（跨平台任务强制）

1. 判定：platform **机制** / frontend **产品语义** / host **present**？  
2. 机制 → 改 `crates/agenterm-platform`，feature 与 typed Unsupported 诚实更新。  
3. 产品 → 改 `src/frontend/*` + `ui_action_catalog`，再改 **两端** adapter（或确认 `control_dispatch` 已覆盖）。  
4. 仅 host → 只动对应 adapter，并登记 catalog allowlist 或本表。  
5. 证据：`cargo test -p agenterm-platform --features process`（相关）+ `cargo test --lib ui_action_catalog` + `cargo test --lib product_sources_do_not_hardcode_breakaway`；无证据不宣称三端手感已齐。

## 下一刀（产品 parity，非封装 crate）

1. instance-picker / server-strip Unix（S′）。  
2. settings scope chrome 升 SHARED 或保持 ONLY。  
3. L2 双拓扑 / ActionId 表驱动。  
4. Unix dialog `create` / shell-* 自动化 id 是否对 Win 暴露。
