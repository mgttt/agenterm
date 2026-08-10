# ape + thin shells + dynamic packages: 架构与落地计划

状态：draft（2026-08-10）
目标：根治构建时间问题——将频繁变更的产品逻辑与极少变更的平台薄壳分离，
使一次典型改动只重编译目标 crate 而非整个 workspace。

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

## 4. 分阶段落地

### Phase A: 拆 ape crate（1-2 天）

**目标**：把根 crate 的 `src/` 产品逻辑搬进 `crates/agenterm-ape/`，
保持 rlib，不引入 C ABI。

```
crates/agenterm-ape/
  Cargo.toml         # [lib] crate-type = ["rlib"]（Phase A 只用 rlib）
  src/
    lib.rs           # re-export 所有子模块
    terminal/        # 从 src/ 搬入
      mod.rs
      parser.rs      # vt100 wrapper
      screen.rs      # terminal state
      selection.rs   # 选区逻辑
    protocol/        # 从 src/ 搬入
      mod.rs
      types.rs       # wire types
      contracts.rs   # capability/error contracts
      ipc_wire.rs    # IPC message format
    script_host/     # 从 src/ 搬入
      mod.rs
      engine_registry.rs  # ScriptEngine enum + dispatch
      fleet_bridge.rs     # FleetCallFn passthrough
      task_dispatch.rs    # Rhai task runner
    server/          # 从 src/ 搬入
      mod.rs
      authority.rs   # workspace/tab authority
      workspace.rs   # workspace lifecycle
      pty_orch.rs    # PTY management
    frontend/        # 直接搬 src/frontend/*
      mod.rs
      action.rs
      ui_action_catalog.rs
      toolbar.rs
      window.rs
      interaction.rs
      composer.rs
      cwd_editor.rs
      input.rs
      new_terminal.rs
      settings.rs
      close_confirmation.rs
      tab_editor.rs
      window_close.rs
      selection.rs
      control_center.rs
    ui_shared/       # 直接搬 src/ui_*.rs
      mod.rs
      geometry.rs
      snapshot.rs
      clipboard.rs
      dispatch.rs
```

**这一步不做任何逻辑改动，只搬文件 + 修 import 路径。**

搬运后，根 crate 的 4 个 `[[bin]]` 改为依赖 `agenterm-ape`（rlib）：

```toml
# 根 Cargo.toml
[dependencies]
agenterm-ape = { path = "crates/agenterm-ape" }
```

4 个 binary 的 `src/bin/agenterm.rs` 等只保留平台窗口创建、输入事件循环、
渲染 surface 代码，其余全 delegate 给 ape。

**验收**：
- `cargo build --workspace` 成功，所有 4 个 binary 功能不变
- `cargo test --workspace` 全绿
- `check.cmd --quick` 通过
- 改 `crates/agenterm-ape/src/terminal/parser.rs` 一行 →
  只重编译 agenterm-ape + 4 binary 链接（不再编译整个 src/ 单体）

**预期收益**：
- 增量构建：改动在 ape 内 → 只重编译 ape crate，4 binary 只重链接
  （链接 4 个 thin binary 比链接整个 monolithic library 快得多，因为 thin binary 自身代码极少）
- 冷构建：Cargo 可并行编译 ape、platform、rh 等独立 crate
- CI：crate 级缓存粒度更细

### Phase B: C ABI 薄壳化（2-3 天）

**目标**：给 ape 加 cdylib 输出，给 4 个 binary 加动态加载路径。

```
crates/agenterm-ape/
  Cargo.toml         # crate-type = ["rlib", "cdylib"]
  src/
    lib.rs           # 已有 rlib re-export
    ffi.rs           # [NEW] C ABI exports（feature-gated: "cdylib"）
    ffi_types.rs     # [NEW] JSON schema for FFI boundary
```

**C ABI 面设计**（最小化！只暴露薄壳真正需要的）：

```rust
// crates/agenterm-ape/src/ffi.rs

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

/// Opaque handle to an initialized APE instance.
pub struct ApeHandle(/* internal */);

/// Opaque handle to a window managed by the APE.
pub struct ApeWindow(/* internal */);

/// Initialize the APE engine. Returns null on failure (error written to stderr).
#[no_mangle]
pub extern "C" fn ape_init(config_json: *const c_char) -> *mut ApeHandle;

/// Create a terminal window. The shell owns the native window handle
/// and passes input events through ape_process_input().
#[no_mangle]
pub extern "C" fn ape_create_window(
    handle: *mut ApeHandle,
    window_config_json: *const c_char,
) -> *mut ApeWindow;

/// Process user input for a window. Returns JSON describing state changes
/// (cursor update, selection change, bell, title change, etc.).
/// Caller must free the returned string with ape_free_string().
#[no_mangle]
pub extern "C" fn ape_process_input(
    window: *mut ApeWindow,
    input_json: *const c_char,
) -> *mut c_char;

/// Get a full snapshot of window state (for rendering, IPC, etc.).
/// Caller must free with ape_free_string().
#[no_mangle]
pub extern "C" fn ape_get_snapshot(
    window: *mut ApeWindow,
) -> *mut c_char;

/// Free a string returned by any ape_* function.
#[no_mangle]
pub extern "C" fn ape_free_string(s: *mut c_char);

/// Destroy a window.
#[no_mangle]
pub extern "C" fn ape_destroy_window(window: *mut ApeWindow);

/// Shut down the APE engine.
#[no_mangle]
pub extern "C" fn ape_shutdown(handle: *mut ApeHandle);
```

**总共 7 个函数**。这不是一个"把整个 Rust API 翻译成 C"的工程——这是
一个"薄壳只需要知道这些"的极窄接口。薄壳的职责：

1. 创建原生窗口（Win32 `CreateWindowEx` / winit `Window`）
2. 收到输入事件 → 序列化为 JSON → `ape_process_input()`
3. 收到状态变更 → 驱动渲染
4. 窗口关闭 → `ape_destroy_window()`

**薄壳代码量估算**（以 Windows 为例）：

```
src/bin/agenterm.rs  (当前) → ~2000 行 Win32 + 产品逻辑混合
                          → 目标 ~300 行纯 Win32 胶水
```

**验收**：
- `cargo build --features cdylib` 产出 `agenterm-ape.dll` (~3MB)
- 4 个 binary 仍可 static-link（rlib 路径），`cargo test` 全绿
- 有 `AGENTERM_USE_CDYLIB=1` 环境变量时，thin shell 走 LoadLibrary 路径
- CDYLIB 路径下的 startup smoke 通过

### Phase C: 动态包插件化（2-3 天）

**目标**：script engines + 可选组件变成 `.dll`/`.so`，ape 启动时扫描并加载。

#### Plugin VTable 设计

```rust
// crates/agenterm-script-common/src/plugin.rs (新增)

/// Every dynamic package exports exactly one function:
///   extern "C" fn agenterm_plugin_get_vtable() -> *const PluginVTable;
#[repr(C)]
pub struct PluginVTable {
    pub version: u32,              // API version (1)
    pub plugin_id: *const c_char,  // e.g. "agenterm-rh"
    pub plugin_kind: *const c_char, // "script-engine" | "platform-backend" | ...
    
    /// Initialize the plugin. Returns null on success, error message on failure.
    pub init: extern "C" fn(ape_bridge: *const ApeBridge) -> *const c_char,
    
    /// Shut down the plugin.
    pub shutdown: extern "C" fn(),
}

/// Bridge from plugin back to the APE.
#[repr(C)]
pub struct ApeBridge {
    /// Call a fleet operation. plugin_data is an opaque pointer the plugin
    /// passed to init(); it's passed back here for context.
    pub fleet_call: extern "C" fn(
        plugin_data: *mut c_void,
        operation_id: *const c_char,
        params_json: *const c_char,
    ) -> FleetCallResult,
}
```

#### 已有的 script engine 改造成 plugin

当前 `ScriptEngineBackend` trait 已定义 `check()` 和 `execute()`。
每个 engine（rh/qjs/lua/sql/wasmcore）只需要加一个 `agenterm_plugin_get_vtable()`
导出函数：

```rust
// crates/agenterm-rh/src/plugin.rs (新增，feature-gated)

#[no_mangle]
pub extern "C" fn agenterm_plugin_get_vtable() -> *const PluginVTable {
    &PluginVTable {
        version: 1,
        plugin_id: c"agenterm-rh".as_ptr(),
        plugin_kind: c"script-engine".as_ptr(),
        init: rh_plugin_init,
        shutdown: rh_plugin_shutdown,
    }
}
```

#### ape 的插件加载器

```rust
// crates/agenterm-ape/src/plugin_loader.rs (新增)

impl ApeEngine {
    fn load_plugins(&mut self, plugin_dir: &Path) {
        for entry in std::fs::read_dir(plugin_dir).unwrap() {
            let path = entry.unwrap().path();
            if path.extension() != Some("dll") && path.extension() != Some("so") {
                continue;
            }
            // SAFETY: we verify the vtable version before calling anything
            unsafe {
                let lib = libloading::Library::new(&path)?;
                let get_vtable: libloading::Symbol<
                    unsafe extern "C" fn() -> *const PluginVTable
                > = lib.get(b"agenterm_plugin_get_vtable")?;
                let vtable = get_vtable();
                if (*vtable).version != 1 {
                    continue; // skip incompatible plugins
                }
                let plugin_id = CStr::from_ptr((*vtable).plugin_id).to_str()?;
                self.plugins.insert(plugin_id, (lib, vtable));
            }
        }
    }
}
```

**验收**：
- `cargo build -p agenterm-rh --features cdylib` 产出 `agenterm-rh.dll`
- ape 启动时扫描 `plugins/` 目录，自动发现并加载
- `agenterm cli script run --engine rh` 仍正常工作
- 新增一个 script engine（如 wasmcore）不需要重编译 ape 或 shell

### Phase D: platform backend 动态化（1-2 天）

**目标**：将 `agenterm-platform` 的 PTY/process 后端变成可选动态包。

Windows 薄壳自带 ConPTY backend 编译进 shell（它不是可选功能）。
但 Unix PTY、ConPTY 诊断工具、特定平台的 process containment 等可以作为动态包。

这一步较小——主要是把 platform 的 feature flags 映射到 plugin kind，
让 ape 在启动时根据当前 OS 选择加载哪个 platform backend plugin。

## 5. 构建时间预期

| 场景 | 当前 (monolith) | Phase A 后 (rlib 拆分) | Phase B 后 (+cdylib) | Phase C 后 (+plugins) |
|------|---------------|----------------------|---------------------|----------------------|
| 改动 terminal parser 一行 | ~14s（增量 hot） | ~3s（只重编 ape crate） | ~3s | ~3s |
| 改动 UI geometry 一行 | ~14s | ~3s | ~3s | ~3s |
| 改动 Win32 shell 一行 | ~14s | ~2s（只重编 thin shell） | ~1s | ~1s |
| 改动 Rhai stdlib | ~14s | ~5s（ape + rh crate） | ~5s | ~2s（只重编 rh.dll） |
| CI 冷编译（全 workspace） | ~20min | ~8min（crate 并行） | ~8min | ~5min（插件并行编译） |
| CI 热编译（缓存命中） | ~5min | ~2min | ~2min | ~1min |

**核心收益不在绝对数值，而在改动影响面**：
- 当前：改任何 `src/*.rs` → 整个 workspace 重编译
- Phase A 后：改动局限在单一 crate
- Phase B/C 后：改动脚本引擎完全不影响 shell 和 ape

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
