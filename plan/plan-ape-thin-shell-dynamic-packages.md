# ape + thin shells + dynamic packages: 架构与落地计划

状态：draft（2026-08-10，2026-08-10 全量 src/ 文件审计后更新）
版本：计划期 v0.1.18，执行期 v0.1.19+（不在 v0.1.18 内施工）
目标：根治构建时间问题——将频繁变更的产品逻辑与极少变更的平台薄壳分离，
使一次典型改动只重编译目标 crate 而非整个 workspace。

## 0. 核心发现（2026-08-10 全量 165 文件审计）

### F1: 真正的靶心是 agenterm.exe 链接的大 library（165 文件）

`agenterm-con.exe` 是 conhost 对标物，故意保持极简独立——不在本次重构靶心内。
`agenterm-com.exe` 是 255 行 `#![no_std]` 零壳，`agenterm-cc.exe` 是 7 行入口——
提取 ape 对它们完全不影响。

本次重构的靶心是拆分 **agenterm.exe 依赖的那个 165 文件的 monolithic library**：

```
当前:
  agenterm.exe ──→ agenterm 库 (165 .rs, 单 crate, 内部互相 crate:: 引用)
                       改一行 terminal parser → 整个 crate 重编译

目标:
  agenterm.exe ──→ agenterm 根 crate (薄壳, ~55 .rs)
                       │
                       └──→ agenterm-ape (产品逻辑, ~110 .rs)
                               改一行 terminal parser → 只重编 ape
```

### F2: 165 文件精确分类

| 类别 | 文件数 | 去向 |
|------|--------|------|
| BIN（4 个 binary + agenterm-con 子模块） | 8 | 留在根 crate |
| BUILD（build_identity.rs） | 1 | 留在根 crate |
| TERMINAL（parser/screen/selection/lifecycle） | 4 | → ape |
| PROTOCOL（wire types/IPC/control contract） | 4 | → ape |
| SCRIPT（engine host/fleet/task/stdlib/worker） | 31 | → ape |
| SERVER（authority/dispatch/workspace/client） | 8 | → ape |
| FRONTEND（dialog states/actions/geometry） | 21 | → ape |
| PLATFORM_ADAPTER_WINDOWS | 4 | 留在根 crate（薄壳） |
| PLATFORM_ADAPTER_UNIX | 10 | 留在根 crate（薄壳） |
| PLATFORM_GLUE（contracts/policy/services） | 46 | 分裂：产品策略 → ape，平台胶水 → 根 |
| OTHER（product logic） | 28 | 大部分 → ape |

### F3: 已有 crate 边界是三层架构

```
Tier 0: agenterm-platform        (OS contracts, 零 workspace 内依赖)
Tier 1: agenterm-script-common   (共享脚手架, 零 workspace 内依赖)
Tier 2: agenterm-rh/lua/qjs/sql/wasmcore  (引擎, 仅依赖 script-common)
Tier 3: agenterm (根)            (产品, 依赖 platform + 所有引擎)
```

`agenterm-ape` 放在 Tier 1.5：依赖 `agenterm-platform` + `agenterm-script-common`，
不依赖根 crate。根 crate 反过来依赖它。

## 1. 问题量化

当前 `cargo build --workspace` 冷编译 wall-clock 约 20 分钟（Windows x86_64 CI，
无缓存）。主要耗时分布：

| 阶段 | 占比估计 | 根因 |
|------|---------|------|
| 依赖 crate 编译 (platform, rh, qjs, lua, wasmcore, …) | ~40% | 已有 crate 边界，可并行 |
| **根 crate library 编译** (src/*.rs, 166 文件) | **~45%** | 单体大 library，无并行 |
| 4 个 binary 链接 | ~15% | 每个 binary 重链接整个 library |

改动 `src/ui_geometry.rs` 一行 → 整个 library 重编译 → 4 个 binary 重链接。
改动 `src/script_engine.rs` 一行 → 同上。

**目标**：让一次"只改产品逻辑"的增量构建下降到秒级（只重编译目标 crate），
CI 冷编译通过 crate 级并行 + 缓存降低到 3-5 分钟。

## 2. 目标架构

```
┌──────────────────────────────────────────────────────────┐
│ 薄壳层 (thin shells) — 每个 ~50-200KB，极少改动            │
│                                                          │
│  agenterm.exe    agenterm.com   agenterm-cc.exe          │
│  (Win32 GUI)     (CLI fwd)      (Control Center)         │
│  ┌──────────┐   ┌──────────┐   ┌──────────┐             │
│  │窗口+渲染  │   │console   │   │CC 投影   │             │
│  │输入+IME  │   │attach    │   │          │             │
│  │LoadLibrary│   │forward   │   │          │             │
│  └────┬─────┘   └────┬─────┘   └────┬─────┘             │
│       │              │              │                    │
│       └──────────────┼──────────────┘                    │
│                      │ LoadLibrary / dlopen               │
├──────────────────────┼──────────────────────────────────┤
│  ape (Agenterm Platform Engine) — cdylib, ~3MB            │
│                                                          │
│  ┌─────────────────────────────────────────────────┐    │
│  │ agenterm-ape (cdylib + rlib)                     │    │
│  │                                                  │    │
│  │ ┌──────────┐ ┌──────────┐ ┌──────────────────┐  │    │
│  │ │terminal  │ │protocol  │ │script-host       │  │    │
│  │ │parser    │ │types     │ │(engine registry) │  │    │
│  │ │screen    │ │contracts │ │fleet bridge      │  │    │
│  │ │selection │ │IPC wire  │ │task dispatch     │  │    │
│  │ └──────────┘ └──────────┘ └──────────────────┘  │    │
│  │ ┌──────────┐ ┌──────────┐ ┌──────────────────┐  │    │
│  │ │server    │ │frontend  │ │ui-shared         │  │    │
│  │ │authority │ │semantics │ │geometry/snapshot │  │    │
│  │ │workspace │ │dialogs   │ │clipboard model   │  │    │
│  │ │PTY orch  │ │actions   │ │focus gates       │  │    │
│  │ └──────────┘ └──────────┘ └──────────────────┘  │    │
│  │                                                  │    │
│  │ C ABI exports:                                   │    │
│  │   ape_init(config_json) -> ApeHandle             │    │
│  │   ape_create_window(handle, config) -> WindowId  │    │
│  │   ape_process_input(window, input_json) -> json  │    │
│  │   ape_get_snapshot(window) -> json               │    │
│  │   ape_shutdown(handle)                           │    │
│  └─────────────────────────────────────────────────┘    │
│                                                          │
│                      │ dlopen / LoadLibrary               │
├──────────────────────┼──────────────────────────────────┤
│ 动态包 (plugins) — 可选加载，独立更新                       │
│                                                          │
│  agenterm-rh.dll    agenterm-qjs.dll   agenterm-lua.dll  │
│  (Rhai runtime)     (QuickJS runtime)  (Lua runtime)     │
│                                                          │
│  agenterm-sql.dll   agenterm-wasmcore.dll                 │
│  (SQLite runtime)   (Wasmtime JIT)                       │
│                                                          │
│  agenterm-platform-win.dll  agenterm-platform-unix.so    │
│  (ConPTY backend)           (Unix PTY backend)           │
└──────────────────────────────────────────────────────────┘
```

### 关键设计决策

1. **ape 是 cdylib + rlib 双输出**
   - `rlib`：开发期用，Rust 类型安全完整保留，无 C ABI 开销
   - `cdylib`：发布期用，薄壳通过 C ABI 动态加载，支持热替换

2. **开发期薄壳仍 static-link ape (rlib)，不付出 C ABI 代价**
   - 日常 dev loop：`cargo build --bin agenterm`，Cargo 自动复用 incremental
   - 只有 CI/release 才走 cdylib 路径

3. **动态包的接口是一个 trait + 一个 C fn pointer table**
   - 每个动态包导出一个 `get_plugin_vtable() -> *const PluginVTable` 函数
   - ape 在启动时扫描 `plugins/` 目录，LoadLibrary 每个 `.dll`/`.so`，注册 vtable

4. **C ABI 面用 JSON 序列化跨边界，而非手工 FFI struct**
   - 避免手工维护 Rust↔C struct 布局一致性
   - serde_json 是 ape 的已有依赖，零新增
   - 代价是序列化开销——但 ape 的边界调用频率很低（init、每帧 input、snapshot），
     不在热路径上

## 3. 现有地基（已就绪，不需新建）

| 组件 | 状态 | 在新架构中的角色 |
|------|------|-----------------|
| `crates/agenterm-platform` | ✅ 已封装，feature-gated | 机制层，被 ape 引用 |
| `crates/agenterm-rh/qjs/lua/sql/wasmcore` | ✅ 独立 crate，feature-gated | 动态包候选 |
| `crates/agenterm-script-common` | ✅ trait 定义 | 插件接口的参考模式 |
| `crates/agenterm-dynacore` | ✅ fleet_call 窄接口 | "动态包只用单一 host-call" 的证明 |
| `src/frontend/*` | ✅ 已分离产品语义 | 直接进 ape |
| `src/ui_*.rs` | ✅ 共享语义 | 直接进 ape |
| `src/platform/adapters/{windows,unix}` | ✅ 已按 host 分目录 | 薄壳的原材料 |

## 4. Phase A 精确搬移序列（8 step，按依赖从少到多）

**策略**：每个 step 搬文件 + 修 import + `cargo check`，不在一大坨里找 bug。
每一步搬完立即验证，出问题范围小。全程不改逻辑，只搬文件。

### Step 0: 前置准备

1. 创建 `crates/agenterm-ape/` 目录和 `Cargo.toml`
2. 在根 `Cargo.toml` 的 workspace members 添加 `"crates/agenterm-ape"`
3. 在 `[dependencies]` 添加 `agenterm-ape = { path = "crates/agenterm-ape" }`
4. 创建空的 `crates/agenterm-ape/src/lib.rs`

**验收**：`cargo check --workspace` 通过。

### Step 1: terminal 模块（最独立，依赖最少）

先搬三个零内部依赖的"叶子"文件：
- `src/terminal_cursor.rs` → `crates/agenterm-ape/src/terminal/cursor.rs`
- `src/terminal_lifecycle.rs` → `crates/agenterm-ape/src/terminal/lifecycle.rs`
- `src/terminal_observation.rs` → `crates/agenterm-ape/src/terminal/observation.rs`

`terminal_runtime.rs` 有跨模块依赖（`crate::pty`、`crate::SCROLLBACK_LINES`、
`crate::frontend`、`crate::wake_signal`、`crate::working_context`、`crate::workspace`），
等依赖到位后再搬。

**验收**：`cargo check` 通过，terminal cursor/lifecycle/observation 从 ape 提供。

### Step 2: protocol 模块

- `src/protocol.rs` → `crates/agenterm-ape/src/protocol/types.rs`
- `src/ipc_endpoint.rs` → `crates/agenterm-ape/src/protocol/ipc_endpoint.rs`
- `src/ipc_transport.rs` → `crates/agenterm-ape/src/protocol/ipc_transport.rs`
- `src/ui_bridge.rs` → `crates/agenterm-ape/src/protocol/ui_bridge.rs`

### Step 3: frontend 模块（21 文件，ARCHITECTURE.md 标注为"产品语义"）

全部 `src/frontend/*.rs` 直接搬入 `crates/agenterm-ape/src/frontend/`。
大多数只依赖 `agenterm_platform` 或纯 std，搬移风险最低。

### Step 4: ui 共享模块

`src/ui_*.rs`（geometry, snapshot, clipboard model, bridge, client, command, lease, interaction）。

### Step 5: product 模块

`src/settings.rs`, `theme.rs`, `locale.rs`, `tab_tree.rs`, `instances.rs`,
`working_context.rs`, `wake_signal.rs`, `operations.rs`, `commands.rs`,
`agent_tools.rs`, `event_journal.rs`, `frontend_server.rs`, `upgrade_identity.rs`,
`webview_host.rs`, `control_center.rs` —— 逐个分析依赖，有循环依赖的留到后续 step。

### Step 6: script 模块（31 文件，最重一批）

先搬"叶子"（被依赖但不依赖别人的）：
- `script_error.rs`, `script_protocol.rs` → 先搬（纯类型，零内部依赖）
- 再逐步搬 host/run/cli 等

### Step 7: server 模块（8 文件）

最后一批，因为 server 依赖几乎所有其他模块：
`server_app.rs`, `control_authority.rs`, `control_dispatch.rs`,
`control_contract.rs`, `workspace.rs`, `named_buffer.rs`, `client/mod.rs`。

### Step 8: 精简薄壳

搬完后根 crate 的 `src/lib.rs` 只剩：
- 薄壳模块的 `mod` 声明（platform/adapters/*, build_identity, tui, incremental_wrapper）
- 对 `agenterm_ape` 的 `pub use` 重导出（保持对外 API 兼容）

### 搬移期间 API 兼容策略

根 crate 当前对外暴露了大量 `pub` 类型。搬移期间在根 crate 保留
`pub use agenterm_ape::xxx` 重导出，binary 和 test 的 `use agenterm::...`
不需要立即改动。全部搬完后逐步迁移 import。

### 搬移期间测试策略

每个 step 后立即：
```powershell
cargo check --workspace          # 编译通过
cargo test --lib                 # 根 crate lib 测试
```
全量门禁（`check.cmd --quick`）只在 Step 8 后跑一次。

## 5. 构建时间预期（基于 2026-08-10 文件审计）

| 场景 | 当前 | Phase A 后 | 原理 |
|------|------|-----------|------|
| 改 terminal cursor 一行 | ~14s | ~4s | 只重编 ape crate（~4 文件）+ 2 binary 重链接 |
| 改 frontend dialog 一行 | ~14s | ~3s | 只重编 ape crate + binary 重链接 |
| 改 Win32 window 一行 | ~14s | ~2s | 只重编根 crate 薄壳模块 |
| 改 Rhai stdlib | ~14s | ~5s | 重编 ape（含 script 模块）+ agenterm-rh crate |
| CI 冷编译 | ~20min | ~10min | ape 与其他 crate 并行编译 |
| CI 热编译（缓存全命中） | ~5min | ~2min | 增量粒度为 crate 级 |

**核心收益不在绝对数值，而在改动影响面**：
- 当前：改任何 `src/*.rs` → 整个 workspace 重编译
- Phase A 后：改动局限在单一 crate
- Phase B/C 后：改动脚本引擎完全不影响 shell 和 ape

## 6. 版本定位

| 版本 | ape 相关工作 |
|------|-------------|
| **v0.1.18** | 计划期——本文件完善、依赖分析、CI 缓存止血（`success()` → `!cancelled()`）等可并行前置 |
| **v0.1.19+** | 执行期——Phase A 搬文件（需 v0.1.17 收口 + v0.1.18 QJS App Pack 完成后授权开工） |
| **v0.2.x** | Phase B/C/D（C ABI 薄壳化 + 动态包插件化） |

v0.1.18 的主题是 Portable App Substrate（QJS App Pack），参见 `plan/plan-v0.1.18.md`。
本文件中的 Phase A 不在 v0.1.18 执行范围内。

## 6. 风险与缓解

| 风险 | 缓解 |
|------|------|
| C ABI 引入的 JSON 序列化开销 | ABI 边界调用频率极低（init、每帧 input、snapshot），不在热路径 |
| cdylib 路径与 rlib 路径行为不一致 | CI 测试两种路径；`AGENTERM_USE_CDYLIB` env 控制切换 |
| 插件版本不兼容 | VTable version 字段；不匹配 → 跳过 + 日志告警，不崩溃 |
| LoadLibrary 失败（找不到 ape.dll） | 薄壳 fallback 到 static-link 路径；CI 验证两种路径 |
| 4 个 binary 共享 ape.dll 的单实例问题 | 每个 binary 启动时各自加载自己的 ape 实例；IPC 仍走现有 server 模型 |
| `libloading` 新增依赖 | 已经是 wasmcore 路径上会碰到的依赖；或直接手写 `LoadLibrary`/`dlopen`（~20 行） |

## 7. 不可退让的约束

1. **rlib 路径永远保留**——dev loop 不走 C ABI，不吃序列化开销
2. **Phase A 只搬文件不改逻辑**——每个 commit 都是纯 move + fix import
3. **所有现有测试继续通过**——`cargo test --workspace` 不被削弱
4. **check.cmd --quick / --skip-smoke 门禁不变**
5. **4 个 binary 的功能行为零变化**——这是纯架构重构，不是功能迭代

## 8. 与缓存修复的配合

本方案与 `plan/claude-analyze-ci.md` §7 的缓存止血（`if: success()` → `if: !cancelled()`）
是互补关系，不是替代关系：

- **缓存修复**：治标，让 CI 不再每轮冷编译，预计 CI 20min→5min
- **ape 拆分**：治本，让增量构建下降到秒级，让 CI 冷编译通过 crate 并行再降一半

推荐顺序：先修缓存（1 行改动，立刻见效），再拆 ape（结构手术，持续收益）。
