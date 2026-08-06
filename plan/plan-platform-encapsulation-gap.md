# platform 封装漏点表（机制债）

状态：active（2026-08-06 · goal-crate-platform P1）  
范围：**仅 OS 机制**（应进 `crates/agenterm-platform` 的能力）。  
产品工作台差距见 [`plan-unix-gui-win-parity.md`](plan-unix-gui-win-parity.md)。  
边界 SSOT：[`ARCHITECTURE.md`](ARCHITECTURE.md) §1.0、[`goal-crate-platform.md`](goal-crate-platform.md)。

## 证据基线

| 检查 | 结果 | 路径 |
|------|------|------|
| 产品 `src/**` 禁止 `windows_sys`/winit/x11/… | **PASS**（本机） | `platform::boundary_tests::production_sources_use_platform_crate_as_the_only_native_boundary` |
| platform 无 `AGENTERM_` / `agenterm::` 产品耦合 | 既有 boundary 测 | `boundary_tests` product-coupling markers on crate |
| 散装 breakaway / ACCESS_DENIED=5 | **已收（本轮）** | 见下表 G1 |

## 漏点 / 收口表

| ID | site | should live in | priority | notes / parity-gap? | status |
|----|------|----------------|----------|---------------------|--------|
| G1 | `src/control_center.rs` 手写 `raw_os_error()==Some(5)` + 二次 `Command::spawn`；`remote_frontend` 仅 `configure_breakaway_visible` 无 denial 回落 | `process`：`spawn_breakaway_visible_command` / `is_breakaway_denied` | **P0** | 产品不应认识 `ERROR_ACCESS_DENIED` 数值；可见 GUI 回落不能用 `CREATE_NO_WINDOW` 的 `spawn_detached_*` | **closed 2026-08-06**：platform 新增 `spawn_breakaway_visible_child/command` + `configure_visible_in_caller_job`；CC 与 `spawn_gui_for_instance` 改走 facade；`spawn_server_instance` 改 `spawn_detached_command`（无窗 authority，与 `autostart_server` 一致） |
| G2 | `std::process::Command` 直接 spawn 的产品路径（script/worker/rhai） | 视语义：`process-spawn` 或保留产品策略 | P2 | Script Runtime / worker 的 argv/env 是产品；仅当需要 Job breakaway/继承事务时必须走 platform | open — 非本轮；勿误把 Rhai 策略塞进 crate |
| G3 | Unix embedded 截图走 softbuffer 像素再 `png` 编码 | `screenshot` 已有编码；capture 在 host present | P3 | 编码可共享；像素来源属 present 合法 | open — 非散装 OS API 泄漏 |
| G4 | Win remote 大量 control-window / GDI 绘制 | `window` control-window host（platform 已有 Win 实现） | P2 | **巨石 present**，不是「漏调 windows_sys 在产品层」；拆分属 L2 结构债 | open — 阻塞：双拓扑 + 巨石；不本轮空转拆 |
| G5 | IME / clipboard / activation 产品侧 | 已经 `agenterm_platform::{ime,clipboard,activation}` | — | 抽查 remote_frontend 已走 facade | **no leak found** |

## 书面结论（P1）

- **并非**「产品层到处直接调 Win32」：boundary 闸已绿。  
- **真实机制漏点**是 **平台语义散落在产品里**（G1：硬编码 ACCESS_DENIED、可见 breakaway 回落未进库）。  
- 本轮 **≥1 收口 = G1**。剩余 open 项不阻塞 goal 成功标准。

## Agent 执行句式（跨平台任务强制）

1. 判定：platform **机制** / frontend **产品语义** / host **present**？  
2. 机制 → 改 `crates/agenterm-platform`，feature 与 typed Unsupported 诚实更新。  
3. 产品 → 改 `src/frontend/*` + `ui_action_catalog`，再改 **两端** adapter。  
4. 仅 host → 只动对应 adapter，并登记 catalog allowlist 或本表。  
5. 证据：`cargo test -p agenterm-platform`（相关）+ `cargo test --lib ui_action_catalog` + 直接单测；无证据不宣称三端手感已齐。

## 下一刀候选（不自动开工）

1. G2 审计：哪些 `Command::spawn` 需要 breakaway/继承事务。  
2. L2：remote/embedded 收敛或 action 表驱动（版本 plan §九 刀5）。  
3. 从 `WINDOWS_ONLY` 提升单个 SHARED 手势（产品叶，见 catalog）。
