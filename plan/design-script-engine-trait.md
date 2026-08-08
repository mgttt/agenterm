# 脚本引擎调用适配层统一设计：`try_execute_*` 三件套的下一刀

| 字段 | 值 |
|------|-----|
| **文档** | 根 crate `src/script_backend.rs` 三个 `try_execute_{rh,lua,qjs}_invocation` 的收敛设计 |
| 日期 | 2026-08-08 |
| 状态 | 设计稿 rev1（未实现） |
| 关联 | `plan/plan-v0.1.16.md` §1「Rh. 脚本引擎矩阵」、`plan/design-scripting-boundary-comparison.md`、`crates/agenterm-script-common`（library-level 已统一层） |
| 范围声明 | **只读 + 设计文档任务**；本文档不修改任何 `.rs` 文件，不改 `try_execute_*` 本身 |

---

## 0. 背景与边界

`crates/agenterm-script-common`（`Common-M1`，见 plan §1 表尾）已经把三引擎 library 级的
`check_many` / `corpus_scan` / manifest hex 助手收敛成一份实现——那一刀切在
**crate 边界**（`agenterm-rh`/`agenterm-lua`/`agenterm-qjs` 各自的薄适配层调用共享 driver）。

本文档处理的是**根 crate 内的下一道缝**：`src/script_backend.rs` 里手镜像的
`try_execute_rh_invocation` / `try_execute_lua_invocation` / `try_execute_qjs_invocation`，
以及它们在 `src/script_worker.rs::execute_inner` 里被调用的方式。这三个函数不共享任何代码，
只共享「形状」——靠人读注释维持一致（`try_execute_qjs_invocation` 的文档注释里明确写了
"Structurally mirrors `try_execute_lua_invocation`"，`script_qjs_host.rs` 也写了
"Same shape as `script_lua_host::LuaFleetBridgeFn`"）。这是典型的**约定漂移风险**：下一个改
lua 分支的人不一定会同步改 qjs 分支。

---

## 1. 现状盘点

### 1.1 三个 `try_execute_*` 函数的签名对照

| | `try_execute_rh_invocation` | `try_execute_lua_invocation` | `try_execute_qjs_invocation` |
|---|---|---|---|
| 位置 | `src/script_backend.rs:90-157` | `src/script_backend.rs:232-296` | `src/script_backend.rs:320-379` |
| `operation` 参数 | `ScriptOperation` | 同 | 同 |
| `source` 参数 | `&str` | 同 | 同 |
| Options 类型 | `RhInvocationOptions { project_root, arguments, budgets }` | `LuaInvocationOptions`（**逐字段同形**） | `QjsInvocationOptions`（**逐字段同形**） |
| fleet_bridge 参数类型 | `Option<crate::script_rh_host::FleetBridgeFn>`<br>= `Option<Box<dyn Fn(&str,&str)->Result<String,String> + Send + Sync>>` | `Option<crate::script_lua_host::LuaFleetBridgeFn>`<br>= `Option<Arc<dyn Fn(&str,&str)->Result<String,String> + Send + Sync>>` | `Option<crate::script_qjs_host::QjsFleetBridgeFn>`<br>= `Option<Arc<dyn Fn(&str,&str)->Result<String,String> + Send + Sync>>` |
| 返回 `Result<Option<_>, E>` 的 `E` | `agenterm_rh::RhError`（typed enum） | `String` | `String` |
| Result 类型的 `value` 字段 | `Option<serde_json::Value>` | `Option<i64>` | `Option<serde_json::Value>` |
| "未启用返回 `Ok(None)`" 早退 | `if !rh_backend_enabled() { return Ok(None); }`（`:96-98`） | 同（`:238-240`） | 同（`:326-328`） |
| `Api` 分支 | `Ok(None)`（不处理，留给 `execute_inner` 顶层短路） | 同 | 同 |
| `Check` 分支 | 调用 `rh_check_with_project_validation` **或**（source 为空时）检查 `cached_rh_pack()` 是否存在——两条路径（`:114-127`） | 单路径：`engine.check(source)`（`:246-251`） | 单路径：`agenterm_qjs::check(source, "invocation.js")`（`:332-337`） |
| `Run`/`Eval` 分支的引擎构造 | `resolve_rh_pack` 解析/加载**已编译的 native pack**（`.dll`/`.so`），走 C ABI 入口（`:128-135`） | `agenterm_lua::LuaEngine::new()` 后 `engine.eval(source, &host)`——**每次调用重新解释 source**（`:242, 286-288`） | 直接 `agenterm_qjs::eval_entry_with_host(source, ..., &host)`——同样每次重新解释（`:370-371`） |
| host 构造方式 | 无独立 host struct；host 函数通过 `extern "C"` + thread-local `FLEET_BRIDGE`/`HOST_ERROR` 挂进 native module（`script_rh_host.rs:37-47, 795-800`） | `agenterm_lua::LuaHostFunctions { fleet_call, args_len, arg, print }`，`Default` 后逐字段赋值（`:254-284`） | `agenterm_qjs::QjsHostFunctions { fleet_call, args_len, arg }`（无 `print` 字段，`:340-368`；`print` 由 `agenterm_qjs::eval_entry_with_host` 内部单独接管） |
| args_len/arg 接线 | 不在 `try_execute_rh_invocation` 里接——走 `RhRunContext.arguments` + `host_args_len_call`/`host_arg_call`（extern "C"，`script_rh_host.rs:415-473`），**不经过这里的闭包** | 逐字段用 `Arc::new(move ...)` 包一层从 `serde_json::Value` 数组读取字符串（`:270-283`），**与 qjs 分支逐字符相同** | 与 lua 分支**逐字符相同**（`:352-367`，唯一差异是变量名） |
| stdout 累积 | `output_capture`（`RhOutputCapture`，budget-checked byte 累加器）+ 追加 `pack.cc_lines`（AOT 编译期常量折叠输出，`:137-146`） | `result.stdout`（引擎 eval 返回值，已在 `agenterm_lua::LuaEngine::eval` 内部按 buffer 累积） | `result.stdout`（同上，`agenterm_qjs::eval_entry_with_host` 内部累积） |
| 输出预算校验位置 | **在 `try_execute_rh_invocation` 内部**手动校验 `output_limit`（`:138-143`），因为要把 `cc_lines` 追加进去后再次越界检查 | 无——委托给 `agenterm_lua` crate 内部（未在此函数内二次校验） | 无——委托给 `agenterm_qjs` crate 内部 |

### 1.2 "镜像 by 约定" vs "engine 本质差异"

**镜像 by 约定（drift risk——结构相同但没有共享代码强制同步）：**

1. `Options` struct 的三份定义（`project_root: Option<PathBuf>`, `arguments: Option<Value>`,
   `budgets: Option<ScriptBudgets>`）——`RhInvocationOptions`/`LuaInvocationOptions`/
   `QjsInvocationOptions`，`:78-83`, `:219-224`, `:298-303`，逐字段完全相同，纯粹是
   Rust 不允许无痛复用匿名 struct 形状。
2. "未启用 → `Ok(None)`" 早退分支——三处逐字符相同的 `if !xxx_backend_enabled() { return Ok(None); }`。
3. `Api` 操作分支——三处逐字符相同的 `Ok(None)`。
4. lua/qjs 的 `args_len`/`arg` 闭包接线——`:270-283` 与 `:352-367` **逐字符相同**（连变量命名模式
   `args_for_len`/`args_for_arg` 都一样），是明确的 copy-paste。这是**当前风险最集中**的一段：
   如果以后要改「index 越界时该 fail 还是回退空字符串」这类语义，必须记得改两处。
5. `script_worker.rs::execute_inner` 里 lua/qjs 两段的 `fleet_bridge` 包装闭包——`:641-659`
   与 `:682-700`，把 `BrokerClient::call_json("fleet.call", ...)` 包成
   `Fn(&str,&str)->Result<String,String>`，**逐字符相同**（唯一区别是类型标注
   `LuaFleetBridgeFn` vs `QjsFleetBridgeFn`）。
6. `script_lua_host.rs`/`script_qjs_host.rs` 两个类型别名文件——本身就是"故意对齐"的产物
   （`script_qjs_host.rs:3-4` 的文档注释明确写"Same shape as `script_lua_host::LuaFleetBridgeFn`
   / `script_rh_host::FleetBridgeFn` by design"）。**但对齐并不完整**：rh 用 `Box`，
   lua/qjs 用 `Arc`（见下方"未被文档记录的不对称"）。

**genuinely engine-specific（不该、也不能被强行抽象掉）：**

1. rh 的 AOT/native-pack 加载路径：`resolve_rh_pack` → `cached_rh_pack()` 或
   `script_rh_cache::loaded_pack_for_source_with_project` → 拿到已编译的 native 模块路径 →
   `call_pack_entry_with_host_result` 通过 `RhNativeModule::load` + C ABI 加载 `.dll`/`.so`
   （`script_rh_host.rs:83-104`）。lua/qjs 完全没有这一层——它们每次调用都从 source 字符串
   重新解释（`LuaEngine::eval`/`eval_entry_with_host`），没有"缓存的已编译产物"概念传进
   `try_execute_*`。
2. rh 的 `Check` 分支有**两条路径**（有 source 走 `rh_check_with_project_validation`；无
   source 但有 `AGENTERM_RH_PACK` 缓存包时直接判定通过），这是因为 rh 支持"预编译 pack + 空 source
   调用"的部署形态；lua/qjs 没有这个概念，`Check` 恒定要求非空 source。
3. rh 的 host 绑定机制是 **extern "C" 函数指针 + thread-local**（`register_native_module`
   按 `host_api_version` 选择 v2–v10 不同数量参数的注册函数，`script_rh_host.rs:106-171`），
   这是 native codegen 的必然形态——ABI 边界必须是 C 函数指针，不能是 Rust 闭包。
   lua/qjs 是**同进程解释器里的 Rust 闭包**（`Arc<dyn Fn>`），可以直接捕获环境。这两种
   host 绑定机制在**类型系统层面不可统一**——一个是过 FFI 边界的裸函数指针表，一个是
   Rust trait object。
4. rh 的输出预算二次校验（`:138-143`，因为 `cc_lines`——AOT 编译期常量折叠产生的额外
   输出行——要在 `try_execute_rh_invocation` 内部追加并再次越界检查）；lua/qjs 没有
   "编译期常量折叠输出"这个概念，budget 校验完全在各自 crate 内部完成。
5. rh 的 `value` 字段来源有双通道：`entry_result.host_value`（host 侧显式写回的
   `RhHostEntryValue::Value`）优先于 `json_value_from_entry(entry_value)`（把 native 入口的
   `i64` 直接转 JSON 数字，`:149-153`）——这是因为 native ABI 的入口函数签名固定返回
   `i64`，"返回复杂 JSON 值"需要一条旁路（host 写回）。lua 的 `value` 就是单纯的
   `Option<i64>`（`LuaEvalResult.value` 本身就是 `i64`，`:227`）；qjs 的 `value` 是
   `Option<serde_json::Value>`（`agenterm_qjs::eval_entry_with_host` 内部已经把 JS 返回值
   `JSON.stringify` 成 typed JSON，见 `try_execute_qjs_invocation` 上方文档注释
   `:312-319`，该注释称这是"strict superset, not a divergence"——审阅后认为这个判断是对的：
   任何 lua 的 i64 值都能装进 `serde_json::Value::Number`，但反向不成立）。

### 1.3 未被现有文档记录的不对称（本次审阅新发现）

以下两点在读代码前**没有在任何 plan/design 文档里被提及**，值得单独指出：

1. **`FleetBridgeFn` 的智能指针类型不统一**：`script_rh_host.rs:6`
   `pub type FleetBridgeFn = Box<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>;`
   是 `Box`（不可克隆，单一所有权），而 `script_lua_host.rs:6` 和 `script_qjs_host.rs:9`
   都是 `Arc`（可克隆，共享所有权）。`script_qjs_host.rs` 的模块文档说三者"Same shape... by
   design"，但这句话**不完全准确**——参数签名形状相同，指针类型不同。这个不对称目前
   没有引发 bug（因为三条调用路径各自只构造一次、消费一次，不需要克隆），但如果统一到
   trait 里、且 trait 方法要求 `Clone`（例如给 `BrokerClient` 场景多次分发），rh 分支就要
   多包一层或者把 `Box` 换成 `Arc`——这是本设计方案 §2 里必须显式处理的一个真实差异点，
   不能假装它不存在。
2. **`ScriptOperation::Api` 在三个 `try_execute_*` 里都返回 `Ok(None)`，从不在这里处理**，
   真正的 `Api` 短路发生在 `script_worker.rs:601-603`（`execute_inner` 顶层，早于任何
   `try_execute_*` 调用）。也就是说三个函数里的 `ScriptOperation::Api => Ok(None)` 分支
   **是死代码**——`execute_inner` 永远不会带着 `ScriptOperation::Api` 走到
   `try_execute_rh_invocation` 这些调用点。这一点在设计 trait 时值得注意：不应该把
   "每个 backend 都要处理 Api 分支"当成一个真实的接口契约点。

### 1.4 `script_worker.rs::execute_inner` 的调用形态（`:565-711`）

三次调用严格按 rh → lua → qjs 顺序、`if let Some(result) = try_execute_*(...) { return Ok(...); }`
链式短路（第一个返回 `Some` 的赢）。关键结构点：

- rh 调用**没有** `#[cfg(not(test))]`（`:605`），lua/qjs 调用**都有**（`:629`, `:670`）。
  这意味着 `cargo test --lib`（不加 `--features`）编译时，`execute_inner` 里只有 rh 分支的
  调用代码会被编译，lua/qjs 两段调用代码**根本不存在于测试二进制里**——`script_backend.rs`
  里的 `try_execute_lua_invocation`/`try_execute_qjs_invocation` 单测能过，但那是在
  `script_backend.rs` 自己的 `#[cfg(test)] mod tests` 里**直接调用**这两个函数，不经过
  `execute_inner`。`plan-v0.1.16.md` §1 QJS-M2 那一叶原文已经明确记录了这件事："`cargo test
  --lib script_backend` 14/14 绿只覆盖 `try_execute_qjs_invocation` 单元层...`execute_inner`
  那段真实分支在 `cargo test` 里根本不编译，rh/lua 同样如此，不是 qjs 独有"——本次审阅确认
  这段记录与代码一致。
- 每次调用都各自把 `broker.as_ref().map(|broker| {...})` 重新包装一次
  `LuaFleetBridgeFn`/`QjsFleetBridgeFn`（`:641-659`, `:682-700`）——这两段闭包体逐字符相同
  （构造 `serde_json::from_str(params)` → `broker.call_json("fleet.call", ...)` →
  `.map(|v| v.to_string())`），rh 那段则调用 `crate::script_rh_host::broker_fleet_bridge`
  （一个已经存在的、封装了同样逻辑的辅助函数，`script_rh_host.rs:20-35`）——**rh 已经把
  这段逻辑抽成函数了，lua/qjs 没有**，是三者里 fleet_bridge 包装最不一致的一处。
- `arguments: serde_json::to_value(&invocation.arguments).ok()` 三处逐字符相同
  （`:613`, `:638`, `:679`）。
- `project_root: invocation.project_root.as_ref().map(std::path::PathBuf::from)` 三处逐字符
  相同（`:609-611`, `:634-636`, `:675-677`）。
- `budgets: Some(invocation.budgets.clone())` 三处逐字符相同。
- 错误映射：rh 用 `.map_err(|error| configuration_error("rh_backend", error.to_string()))`
  （`error` 是 `agenterm_rh::RhError`，需要 `.to_string()`）；lua/qjs 用
  `.map_err(|error| configuration_error("lua_backend", error))`（`error` 已经是 `String`，
  不需要 `.to_string()`）——这是 §1.1 里"返回 `Result<_, E>` 的 `E` 不统一"在调用点的直接
  后果。

---

## 2. 方案：`trait ScriptEngineBackend` + 枚举静态分发注册表

### 2.1 为什么是 trait，不是别的

考虑过的替代方案：

- **纯函数表（无 trait，只是把现有三个函数塞进一个 `[fn; 3]` 数组）**：拒绝。参数列表本身
  三份不同（`RhInvocationOptions` vs `LuaInvocationOptions` vs `QjsInvocationOptions`），
  函数指针数组要求签名一致，等于先做 trait 要做的归一化工作，却拿不到 trait 的方法分组
  和文档承载能力。
- **宏生成三份特化代码（保持零抽象开销但消除手抄）**：可行，但会把"三份不同签名"这个
  真问题藏进宏展开里，出错时的报错信息更差，且不利于后续给 sql 后端"照着抄一份 impl"
  这种最常见的扩展路径——trait 的 `impl Trait for NewType` 比宏调用更容易被新贡献者模仿。
- **trait + `Box<dyn Trait>` 动态分发注册表**：本文档最终选择的方案的一个变体。需要先判断
  object-safety。

**结论：用 trait，但不强制 `dyn`——枚举静态分发是默认，`dyn` 是可选的运行时注册表扩展点。**
理由见 §2.4。

### 2.2 归一化后的公共类型

先解决 §1.1 里"三份 Options struct 只是重复"和"$E$ 不统一"两个问题：

```rust
// src/script_backend.rs（或拆到新文件 src/script_engine.rs——见 §4 分期）

/// 三引擎共享的调用选项。此前 RhInvocationOptions/LuaInvocationOptions/
/// QjsInvocationOptions 三份完全同形，此处合一。
#[derive(Clone, Debug, Default)]
pub struct ScriptInvocationOptions {
    pub project_root: Option<PathBuf>,
    pub arguments: Option<Value>,
    pub budgets: Option<ScriptBudgets>,
}

/// 统一的调用结果。`value` 用 `Option<serde_json::Value>`（qjs 已经是；
/// lua 的 i64 通过 `serde_json::Value::from` 无损装入；rh 的双通道
/// entry_value/host_value 收窄仍在 rh 适配层内部完成，trait 边界只看到
/// 最终 JSON 值——见 §2.3 关于"trait 吸收差异 vs 拒绝吸收"的判断）。
pub struct ScriptInvocationResult {
    pub stdout: String,
    pub value: Option<serde_json::Value>,
}

/// 统一错误类型。三引擎当前分别是 RhError（typed enum）/String/String。
/// trait 边界收敛到 String——理由见下方"哪里不吸收"。
pub type ScriptEngineError = String;

/// fleet_bridge 统一为 Arc（吸收 rh 的 Box→Arc 差异，见 §1.3 发现 1）。
/// rh 适配层内部把 Arc 解包传给仍然要 Box 的 script_rh_host::FleetBridgeFn
/// （FFI 边界只消费一次，`Arc::as_ref()` 或包一层闭包即可，不需要改
/// script_rh_host.rs 本身的 Box——见 §3 非目标）。
pub type ScriptFleetBridgeFn = std::sync::Arc<dyn Fn(&str, &str) -> Result<String, String> + Send + Sync>;
```

**这里发生的差异吸收，逐条对账 §1.1/§1.3 的表：**

- `Options` 三份 → 一份：**吸收**，因为逐字段本来就相同，纯语法重复。
- fleet_bridge `Box` vs `Arc` → **吸收**，trait 边界统一用 `Arc`；rh 内部适配层
  （不是 `script_rh_host.rs` 本身）做一次 `Arc→Box`-shaped 闭包转换。选 `Arc` 而不是
  `Box` 是因为 lua/qjs 两家已经是 `Arc`（2:1 多数），且 `Arc` 严格更通用（`Box` 调用点
  不需要 clone 时 `Arc` 用法和 `Box` 完全一样）。
- `value: Option<i64>`（lua）vs `Option<Value>`（rh/qjs）→ **吸收**，收窄到
  `Option<serde_json::Value>`，lua 适配层内部做 `Value::from(i64)`。
- 错误类型 `RhError`（typed）vs `String`（lua/qjs）→ **部分吸收，但明确降级**：trait 方法
  签名统一返回 `Result<_, ScriptEngineError>`（`= String`）。rh 适配层内部把
  `agenterm_rh::RhError` `.to_string()` 掉。**这是有损的**——`RhError` 目前是结构化 enum，
  调用方（`script_worker.rs`）目前也只是 `.to_string()` 后塞进 `configuration_error`，
  所以现状下没有实际信息损失；但如果未来 `script_worker.rs` 想按 `RhError` 的 variant
  做不同的 `ScriptFailureCategory` 分类（目前它不做——所有 backend 错误统一映射成
  `configuration_error`，见 `execute_inner:623, 661, 702`），trait 化会把这条路堵死，
  除非把 `ScriptEngineError` 换成一个新的跨引擎 enum（本设计不做这一步，标为 §3 非目标，
  因为 lua/qjs 目前根本没有结构化错误可对齐）。

### 2.3 trait 本体

```rust
/// 单个脚本引擎在“调用”这一层的统一接口。不覆盖 check-many/corpus-scan
/// （那一层已经在 crates/agenterm-script-common 统一，见该 crate 文档）、
/// 不覆盖 pack/qualify CLI 动词（各引擎 pack 形状本质不同，见 §3）。
pub trait ScriptEngineBackend {
    /// 对应的 ScriptBackend 变体，用于日志/错误码前缀等场景。
    fn backend_id(&self) -> ScriptBackend;

    /// 该引擎认领的入口文件扩展名，供 ScriptBackend::from_entry_path 使用。
    /// rh: &["rh", "rhai"]；lua: &["lua"]；qjs: &["js", "mjs"]。
    fn entry_extensions(&self) -> &'static [&'static str];

    /// 该引擎是否通过 AGENTERM_SCRIPT_BACKEND 环境变量被选中/启用。
    /// 默认实现读全局 ScriptBackend::from_env()==self.backend_id()；
    /// 保留为方法（而非直接调用自由函数)是为了 §2.6 sql 后端注册表扩展。
    fn enabled(&self) -> bool {
        ScriptBackend::from_env() == self.backend_id()
    }

    /// Check 操作。source 可能为空（仅 rh 的 cached-pack 部署形态合法，
    /// 见 §1.2 genuinely-specific 条目 2）——非 rh 引擎的实现应在 source
    /// 为空时返回 Err，不需要在 trait 层强制这条规则。
    fn check(
        &self,
        source: &str,
        options: &ScriptInvocationOptions,
    ) -> Result<(), ScriptEngineError>;

    /// Run/Eval 操作的统一入口。operation 只会是 Run 或 Eval——Api 分支
    /// 由 execute_inner 顶层短路吸收（见 §1.3 发现 2），trait 不需要
    /// 处理 ScriptOperation::Api，方法签名里干脆不传 operation 由调用方
    /// 在 Run/Eval 之间转发的语义差异（若某引擎将来需要区分，方法拆两个
    /// 而不是继续传 enum 靠 match 分叉——YAGNI，目前三引擎的 try_execute_*
    /// 内部 Run/Eval 分支本来就是同一段代码，见 script_backend.rs:128,253,339
    /// 的 `ScriptOperation::Run | ScriptOperation::Eval =>` 合并匹配）。
    fn execute(
        &self,
        source: &str,
        options: &ScriptInvocationOptions,
        fleet_bridge: Option<ScriptFleetBridgeFn>,
    ) -> Result<ScriptInvocationResult, ScriptEngineError>;
}
```

**逐点回应任务要求的"where 三个函数签名不同，trait 如何吸收/拒绝吸收"：**

| 原差异 | trait 处理 |
|---|---|
| `Options` 三型 → 一型 | 吸收（§2.2） |
| 返回 `Result<Option<_>, E>` 的 "None = 未启用" 语义 | **拒绝原样吸收**——改为 `enabled()` 单独查询 + `check`/`execute` 假定"已确认启用才调用"。理由：把"是否启用"编码进返回值的 `Option` 层，意味着每个调用点都要重复"如果 None 就试下一个"的链式短路逻辑（`execute_inner` 现在正是这样），这段链式短路本该由**注册表**统一做（见 §2.5），不该是每个 backend 自己关心的事。trait 方法因此更窄：`execute`/`check` 只在"这个 backend 已经被选中"的前提下调用，不需要自证。 |
| Check 分支 rh 的双路径 vs lua/qjs 单路径 | **不吸收**——`check` 方法体内部允许分支，trait 不规定"必须支持空 source"，只规定签名。rh 的 impl 内部做 `if source.is_empty() { ... cached_rh_pack ... }`；lua/qjs 的 impl 直接调用各自 `check`，对空 source 自然返回其引擎的语法错误（无需特殊 case）。 |
| rh 的 native-pack 加载 vs lua/qjs 重新解释 | **不吸收，也不该吸收**——`execute` 方法体内部实现自由决定"加载已编译产物"还是"每次重新解释"，trait 不感知。见 §3 非目标第一条。 |
| host 绑定机制（FFI 函数指针表 vs Rust 闭包） | **不吸收**——`execute` 方法只接收 `fleet_bridge: Option<ScriptFleetBridgeFn>`（一个 Rust 闭包）和隐含在 `ScriptInvocationOptions.arguments` 里的 args；rh 的 `RhEngineBackend::execute` 内部把这个 `Arc<dyn Fn>` 转换成 `script_rh_host::FleetBridgeFn`（`Box<dyn Fn>`）、写入 thread-local、调用 `call_pack_entry_with_host_result`——这一层 FFI 转换**留在 rh 适配层内部**，trait 边界只看到统一的闭包类型。 |
| args_len/arg 的重复闭包接线（lua/qjs 逐字符相同） | **吸收，但吸收点在 `ScriptInvocationOptions.arguments: Option<Value>` 这个字段本身，不在 trait 方法签名**——lua、qjs 各自的 `execute` 实现内部仍然要各自把 `Option<Value>` 转换成各自引擎的 `args_len`/`arg` host 函数（因为 `LuaHostFunctions`/`QjsHostFunctions` 是两个不同 crate 里的不同类型，trait 无法跨 crate 强行合并它们）。**能消掉的重复是"两段逐字符相同的闭包构造代码"——做法是在 `agenterm-script-common` 里加一个共享的 helper（例如 `fn args_accessors(args: &Value) -> (impl Fn()->i64, impl Fn(i64)->Result<String,String>)`），lua/qjs 的 `execute` 都调用它，而不是各自手写。这一步不需要 trait 就能做，属于 §4 分期里可以先做的低风险子任务。** |
| `configuration_error("rh_backend", ...)` vs `("lua_backend", ...)` 前缀 | **吸收**——注册表统一用 `backend.backend_id().as_str()` 生成前缀（`"rh"`/`"lua"`/`"qjs"`），不再需要三处手写字符串常量。 |

### 2.4 Object-safety：dyn 可行，但默认走枚举静态分发

逐条检查 `ScriptEngineBackend` 的 object-safety：

- `backend_id(&self) -> ScriptBackend`：`ScriptBackend` 是 `Copy` 具体类型，安全。
- `entry_extensions(&self) -> &'static [&'static str]`：具体类型，安全。
- `enabled(&self) -> bool`：具体类型，安全，且有默认实现（默认方法不影响 object-safety，
  只要它不依赖 `Self: Sized`）。
- `check`/`execute`：参数/返回值都是具体类型（无泛型参数、无 `impl Trait`、无
  `where Self: Sized` 之外的约束），安全。

**结论：`ScriptEngineBackend` 是 object-safe 的，`Box<dyn ScriptEngineBackend>` 可行。**

但默认分发机制建议用**枚举静态分发**而不是 `Vec<Box<dyn ScriptEngineBackend>>`，原因：

1. 三个引擎（未来加 sql 是四个）是**编译期已知的封闭集合**，不是运行时插件——`ScriptBackend`
   本身已经是一个三变体枚举（`:18-22`），`match` 静态分发在这种封闭集合上比 `dyn` 更符合
   Rust 惯例（没有虚表开销，编译器能做穷尽性检查——加一个新引擎变体时，所有忘记处理它的
   `match` 会在编译期报错，`dyn` 注册表则不会）。
2. `execute_inner` 当前的调用链是"按固定顺序尝试三个，第一个 enabled 的赢"——这个顺序本身
   是产品语义的一部分（虽然目前 `AGENTERM_SCRIPT_BACKEND` 保证同时只有一个 enabled，顺序
   在正常情况下不影响结果，但 entry-path 扩展名映射 `ScriptBackend::from_entry_path` 已经
   是精确匹配、不需要尝试链——顺序依赖只在"没有显式 entry 时全靠 env var"的场景存在）。

**因此本设计选择：**

```rust
/// 静态分发注册表——不是 dyn trait object 列表，是一个枚举 + match，
/// 但复用同一个 trait 定义 impl 的方法体（避免手写第四份 match 分支）。
pub enum ScriptEngine {
    Rh(RhEngineBackend),
    Lua(LuaEngineBackend),
    Qjs(QjsEngineBackend),
    // Sql(SqlEngineBackend),  // 见 §2.6
}

impl ScriptEngine {
    pub fn all() -> [ScriptEngine; 3] {
        [Self::Rh(RhEngineBackend), Self::Lua(LuaEngineBackend), Self::Qjs(QjsEngineBackend)]
    }

    pub fn for_backend(id: ScriptBackend) -> Self {
        match id {
            ScriptBackend::Rh => Self::Rh(RhEngineBackend),
            ScriptBackend::Lua => Self::Lua(LuaEngineBackend),
            ScriptBackend::Qjs => Self::Qjs(QjsEngineBackend),
        }
    }
}

impl ScriptEngineBackend for ScriptEngine {
    fn backend_id(&self) -> ScriptBackend {
        match self {
            Self::Rh(b) => b.backend_id(),
            Self::Lua(b) => b.backend_id(),
            Self::Qjs(b) => b.backend_id(),
        }
    }
    // ...execute/check 同样 match-delegate...
}
```

`execute_inner` 改成：

```rust
let engine = ScriptEngine::for_backend(ScriptBackend::from_env());
if engine.enabled() {
    return engine
        .execute(&invocation.source, &options, fleet_bridge)
        .map(|result| (result.stdout, result.value))
        .map_err(|error| configuration_error(format!("{}_backend", engine.backend_id().as_str()), error));
}
```

这一步**吸收掉**了 `execute_inner` 里三段逐字符相同的
`project_root.as_ref().map(PathBuf::from)` / `serde_json::to_value(&invocation.arguments).ok()`
/ `budgets: Some(...)` 构造（现在只写一次 `ScriptInvocationOptions` 构造），也吸收掉了
§1.4 提到的"rh 用 `broker_fleet_bridge` 辅助函数、lua/qjs 没有"的不一致——`fleet_bridge`
的包装（`BrokerClient::call_json("fleet.call", ...)` → `Arc<dyn Fn>`）现在只在
`execute_inner` 里写一次，三个 backend 的 `execute` 实现内部各自决定怎么消费这个统一
的 `ScriptFleetBridgeFn`。

**`dyn` 仍然保留一个真实用途**：如果 §2.6 的 sql 后端需要**运行时**注册（例如通过某种
插件发现机制，而不是编译期 `match` 穷尽），`Box<dyn ScriptEngineBackend>` 仍然可用，因为
trait 本身是 object-safe 的——本设计不排除这条路，只是不作为默认机制，因为当前没有
运行时插件发现的需求（YAGNI）。

### 2.5 `AGENTERM_SCRIPT_BACKEND` env + 扩展名路由如何折叠进去

现状 `ScriptBackend::from_env()`（`:25-38`）和 `ScriptBackend::from_entry_path()`
（`:49-63`）**保持不变**——它们是选择"用哪个 `ScriptEngine` 变体"的路由逻辑，本身就是
枚举上的纯函数，不需要变成 trait 方法。变化的只是：`entry_extensions()` 这个 trait
方法可以反过来**验证** `from_entry_path` 的映射表和各 `EngineBackend::entry_extensions()`
是否一致（写一个测试：`for engine in ScriptEngine::all() { for ext in
engine.entry_extensions() { assert_eq!(ScriptBackend::from_entry_path(&format!("x.{ext}")),
engine.backend_id()); } }`），把"扩展名到 backend 的映射"从"两处手写、靠人眼对齐"
变成"一处权威（`entry_extensions()`），一处派生校验（测试）"。`from_entry_path` 函数体
本身是否直接用 `entry_extensions()` 重写是次要的实现选择（可以直接重写成遍历
`ScriptEngine::all()`，也可以保留手写 match、只用测试校验一致性——本设计建议前者，
因为它彻底消除了"新增引擎要记得改两个地方"的风险，但不是本设计的强制要求）。

### 2.6 第四个后端（sql）需要实现的最小方法集

如果/当 `sql` 后端加入（`plan-v0.1.16.md` §1 Rh 节和 `agenterm-script-common` 的模块文档
都提到过"用户已提 sql"，但明确"未开工，仅记录以防撞车"），它需要：

1. `backend_id()` 返回一个新的 `ScriptBackend::Sql` 变体（需要先扩展 `ScriptBackend` 枚举，
   这本身是一处**不可避免的改动点**——trait 化不能让"加一个后端"变成零改动，只能让
   "加一个后端"局部化到几个明确的地方：`ScriptBackend` 枚举 + 一个新 `SqlEngineBackend`
   struct + `ScriptEngine` 枚举多一个变体 + 少数几个 match 补全）。
2. `entry_extensions()` —— 例如 `&["sql"]`。
3. `check(source, options)` —— sql 的"check"大概率是"能否 parse 成合法语句/查询计划"，
   不要求有可执行的 runtime。
4. `execute(source, options, fleet_bridge)` —— 这是 sql 后端**最值得在设计阶段想清楚**的
   一点：sql 语句本身没有"调用 fleet.* host 函数"这种脚本语言概念，`fleet_bridge` 参数
   对 sql 可能永远是 `None`/未使用；`ScriptInvocationResult.value` 大概率是查询结果集
   序列化成的 JSON 数组，而不是单个标量。trait 签名**不需要为 sql 现在就改**——
   `Option<serde_json::Value>` 已经能装下"一个数组值"，`fleet_bridge: Option<...>` 本身
   允许为 `None`。这正是"trait 边界够宽容，具体差异留给 impl 内部"这条设计原则的验证：
   sql 不需要 trait 新增方法，只需要 `execute` 内部把 `fleet_bridge` 参数忽略掉。
5. **不需要** sql 实现 `enabled()`——用默认实现（读 `ScriptBackend::from_env() == Sql`）
   即可，除非 sql 有自己的额外启用条件（目前没有已知需求）。

也就是说 sql 后端的最小方法集和 rh/lua/qjs 完全一样——**四个方法**（`backend_id`,
`entry_extensions`, `check`, `execute`），`enabled` 有默认实现可以不覆盖。这是这个 trait
设计"值得做"的直接证据：如果 sql 需要的方法集和现有三个不一样，说明 trait 边界画错了地方。

---

## 3. 不做什么（非目标）

1. **不强行统一 rh 的 native-pack 加载路径**。`resolve_rh_pack` / `RhNativeModule::load` /
   `register_native_module` 的 v2–v10 host API 版本分派（`script_rh_host.rs:106-171`）
   完全留在 rh 的 `execute` 实现内部，trait 不定义"加载已编译产物"这个概念、不定义
   "native pack 缓存"这个概念。如果未来 lua/qjs 也想要类似的字节码缓存加载路径（
   `plan-v0.1.16.md` QJS-M3 那一叶已经记录了"qjs 的 pack 目前是 real-but-unused
   bytecode + 重新解析 source"），那是各自 crate 内部的优化，不通过本 trait 暴露。
2. **不统一 `check-many`/`corpus-scan`/`pack`/`qualify` CLI 动词**——那一层已经在
   `crates/agenterm-script-common` 处理（library 级），本 trait 只覆盖
   `src/script_backend.rs` 里"单次调用"（check 单个 source / execute 单个 source）这一层，
   两层职责不重叠，不应该合并（`agenterm-script-common` 的模块文档已经明确写了
   "pack/qualify（rh 是 native-codegen pack，与 lua/qjs 的 bytecode-指纹形状真不同，
   硬套一个 schema 是埋雷）"——同样的判断适用于本 trait）。
3. **不把 `script_rh_host.rs`/`script_lua_host.rs`/`script_qjs_host.rs` 三个类型别名文件
   合并成一个**。它们各自贴着不同 crate（`agenterm_rh`/`agenterm_lua`/`agenterm_qjs`）的
   host 函数类型，合并需要三个 crate 互相依赖或者依赖一个新的共享 crate 定义 host 函数
   trait——这是比"调用适配层"更深一层的改动（会碰到 `agenterm-rh` 的 FFI 边界只能是
   `Box`、不能轻易换成别的这类约束），本设计不做，留给 rh 的 native-pack 那条轨自己决定
   是否值得再抽象。
4. **不改变 `RhError` 的结构化程度**——不新增一个跨引擎的结构化错误 enum
   （§2.2 里已经讨论过这个取舍：`ScriptEngineError = String` 是有损收敛，但目前
   `execute_inner` 对三个 backend 的错误处理本来就已经拍平成同一种
   `ScriptFailureCategory::Configuration`，没有信息在这一步被真正丢失）。
5. **不改变 args_len/arg 的 per-engine host 类型**（`LuaHostFunctions`/`QjsHostFunctions`
   仍然是两个不同 crate 里的不同 struct）——trait 只统一"调用层"看到的
   `ScriptInvocationOptions.arguments: Option<Value>`，不下潜到"每个引擎怎么把这个 Value
   转换成自己的 host 绑定"。§2.3 表格里提到的"用 `agenterm-script-common` helper 消掉
   lua/qjs 重复闭包"是一个独立的、可选的后续优化，不是本 trait 设计的必要部分。
6. **不在本次改动 `ScriptBackend` 枚举本身**（不新增 `Sql` 变体）——§2.6 只是说明
   sql 加入时需要做什么，本设计不预先添加未使用的变体。

---

## 4. 分期（M-叶风格）

每叶独立可落地、可测试，互不阻塞对方 review。

| 叶 | 内容 | 落地文件 | 测试保障 |
|----|------|----------|----------|
| **Trait-M1** | 定义 `ScriptEngineBackend` trait + `ScriptInvocationOptions`/`ScriptInvocationResult`/`ScriptEngineError`/`ScriptFleetBridgeFn` 公共类型（§2.2/§2.3）。**不改动任何现有 `try_execute_*` 函数体或调用点**——先让新类型和旧类型并存，新类型只在新增的单测里被构造和断言字段形状。 | 新文件 `src/script_engine.rs`（避免继续膨胀已经 736 行的 `script_backend.rs`），`src/lib.rs` 加 `pub mod script_engine;` | 新增单测：object-safety 编译期断言（`fn _assert_object_safe(_: &dyn ScriptEngineBackend) {}`）、`ScriptInvocationOptions::default()` 字段形状。不动现有 `cargo test --lib script_backend` 的 20 个测试——它们必须逐字节保持绿。 |
| **Trait-M2** | 实现 `RhEngineBackend`/`LuaEngineBackend`/`QjsEngineBackend`（各自 `impl ScriptEngineBackend`，方法体是把现有 `try_execute_rh_invocation`/`try_execute_lua_invocation`/`try_execute_qjs_invocation` 的 `Run\|Eval`/`Check` 分支**原样搬过来**，剥掉"未启用返回 None"这层——那层现在由调用方先查 `enabled()` 再调用）。旧的 `try_execute_*` 函数**保留不删**，标记 `#[deprecated]` 或直接留作 `pub(crate)` 内部实现细节，避免一次性大改动破坏 `#[cfg(not(test))]` 编译期属性（见 §5 风险）。 | `src/script_engine.rs` 或按引擎拆 `src/script_engine_rh.rs` 等（依实现时体量决定，不预先定死） | 每个 `EngineBackend::execute`/`check` 补对应单测，复用 §Trait-M1 已有的测试夹具风格（`ENV_LOCK` mutex 保护环境变量，参照现有 `script_backend.rs:388` 的 `ENV_LOCK` 模式）。目标：`cargo test --lib script_engine` 全绿，且不影响 `cargo test --lib script_backend` 原有 20 个测试。 |
| **Trait-M3** | 引入 `ScriptEngine` 枚举（§2.4）+ `ScriptEngine::for_backend`/`all`/`enabled`；**切换 `script_worker.rs::execute_inner` 的调用点**改用 `ScriptEngine`，删除 `execute_inner` 里三段逐字符重复的 `project_root`/`arguments`/`budgets` 构造和 fleet_bridge 包装闭包（§2.4 末尾示例）。**关键约束**：迁移后必须显式验证 `#[cfg(not(test))]` 的编译期属性去留（rh 分支现在没有这个 cfg，lua/qjs 有）在新代码里如何处理——见 §5 风险第一条,这里不能悄悄丢掉这条属性的效果，否则要么测试构建时间暴涨（如果去掉 cfg 导致 lua/qjs 引擎在测试构建里也被链接进来）要么测试覆盖率暴跌得更隐蔽。 | `src/script_worker.rs`（`execute_inner` 函数体，`:565-711`） | 现有 `script_worker.rs` 里 `#[cfg(test)] mod tests`（`:877` 起）的所有测试必须保持绿（`framed_worker_runs_multiple_invocations_without_stdout_corruption` 等——这些测试驱动的是走 rh 分支的 eval，因为测试构建里只有 rh 分支存在）。新增一条集成级测试验证 `ScriptEngine::for_backend` 路由和旧 `ScriptBackend::from_env()` 行为一致。 |
| **Trait-M4（可选，视 M1–M3 实际工作量决定是否单独立叶）** | 删除旧的 `try_execute_rh_invocation`/`try_execute_lua_invocation`/`try_execute_qjs_invocation` 三个公开函数（此时它们的调用者只剩 `script_engine.rs` 内部，且逻辑已经完全等价搬入 `EngineBackend::execute`/`check`），清理 `RhInvocationOptions`/`LuaInvocationOptions`/`QjsInvocationOptions`/`RhInvocationResult`/`LuaInvocationResult`/`QjsInvocationResult` 六个旧 struct。**这一叶有破坏性**（`script_backend.rs` 现有的 20 个测试直接调用这些函数，`:390-736`）——必须先把这 20 个测试改写成调 `ScriptEngine`/`EngineBackend` 等价路径，逐条核对断言不变，再删除旧代码。 | `src/script_backend.rs` | 删除后 `cargo test --lib script_backend` + `cargo test --lib script_engine` 合计条数应 ≥ 删除前的 20 条（允许合并同类项减少条数，但每条旧断言必须能在新测试里找到等价覆盖——不是"数字对齐"而是"断言对齐"，需要逐条人工核对，不能只看 `cargo test` 总数）。 |

**为什么不是一步做完**：`execute_inner` 是共享工作树里活跃改动的文件（§5 风险第二条），
`script_backend.rs` 的 20 个测试是当前唯一验证"三引擎调用契约没有跑偏"的机制——分四叶
落地，每叶都能独立 `cargo test` 验绿再推进，比一次性大改动更容易在并发改动的仓库里
安全合并。Trait-M1/M2 甚至可以在不碰 `script_worker.rs` 的情况下完成和验证，把和其他
agent 的冲突面压到最小（Trait-M3 才需要碰 `script_worker.rs`，这是当前 git status 里
已经显示被修改的文件之一——见 §5）。

---

## 5. 风险

1. **`#[cfg(not(test))]` 属性的编译期覆盖率陷阱**（已在 §1.4 详细描述现状）。
   `try_execute_rh_invocation` 调用点没有这个 cfg，lua/qjs 调用点有。原因合理推测：
   rh 是默认后端（`ScriptBackend::from_env()` 的 `_ => Self::Rh` 兜底，`:36`），
   `execute_inner` 的单测（`script_worker.rs:877` 起）大量依赖 rh 分支真实跑通
   （例如 `framed_worker_runs_multiple_invocations_without_stdout_corruption` 用
   `print("inside-result"); 21 * 2` 这种 rh 语法），如果去掉 rh 调用点没有意义；lua/qjs
   分支在 `cargo test` 默认构建里被跳过，大概率是为了控制测试构建时间/减少测试构建对
   `mlua`/`rquickjs` FFI 编译依赖的暴露（两者都需要 C 编译工具链）。
   **trait 化的风险点**：如果 `ScriptEngine::execute` 把三个引擎的 `match` 分支写在
   同一个函数体里、且这个函数体本身没有 cfg 保护，Rust 的 `match` 语义要求所有分支
   **在同一次编译里全部类型检查**——`LuaEngineBackend`/`QjsEngineBackend` 的 `impl` 代码
   即使运行时走不到，也会被**编译**（这本身不是坏事，`#[cfg(not(test))]` 现状下达成的
   效果是"lua/qjs 分支的代码在 test profile 下连编译都不发生"，而不是"编译了但不运行"）。
   如果 Trait-M3 把 `#[cfg(not(test))]` 简单地套在整个 `if engine.enabled() { ... }` 块外面，
   等效于现状；但如果 `ScriptEngine::for_backend`/`ScriptEngine::all()` 这些辅助函数
   本身不加 cfg（它们大概率不该加，因为路由逻辑本身是纯函数、值得被测试覆盖），
   要小心不要在**这些辅助函数的实现里**间接触发 lua/qjs `impl` 代码在 test cfg 下被编译
   ——例如 `ScriptEngine::all()` 返回 `[Rh(..), Lua(..), Qjs(..)]` 这个数组构造本身
   不涉及调用 `execute`/`check`，只是构造 marker struct，是安全的；但如果哪个测试
   不小心调用了 `ScriptEngine::Lua(..).execute(...)` 而没有外层 cfg 保护，测试构建会
   尝试链接 `agenterm_lua` 的完整 eval 路径——这在当前 `Cargo.toml` 里 `agenterm-lua`
   本来就是普通（非 dev-only）依赖，所以**编译层面不会失败**，只是失去了"test profile
   跳过 lua/qjs 分支"这条现状约定。Trait-M3 落地时必须明确决定：是**保留**现状（新代码
   继续在等价位置套 `#[cfg(not(test))]`），还是**主动改变**这条约定（让测试也覆盖
   lua/qjs 分支，代价是测试构建时间变长）——这是一个需要人工决策的点，不是可以在
   trait 设计阶段替用户拍板的细节，本文档只标注风险，不预设答案。

2. **共享工作树并发改动 `src/script_worker.rs`**。本次审阅读取 `src/script_worker.rs`
   时该文件已经在 git status 里显示为已修改（`M src/operations.rs` 也在改动列表，虽然
   `script_worker.rs` 本身当前 git status 快照未显示为已改，但根据
   `plan-v0.1.16.md` QJS-M2 记录，"本次盘点时在共享工作树发现该改动已在但未提交"这类
   情况在这个仓库里发生过至少一次——`src/script_worker.rs` 是三引擎调用链的汇合点，
   历史上就是高冲突文件）。Trait-M3（唯一需要碰这个文件的分期）落地前必须
   `git pull --ff-only` 并重新读一遍当前 `execute_inner` 的实际内容，不能假设本文档
   §1.4 描述的行号（`:565-711`）在实施时仍然精确——**这是本文档在只读盘点阶段的已知
   时效性限制，不是掩盖，是提前声明**。

3. **`RhInvocationResult`/`LuaInvocationResult`/`QjsInvocationResult` 三个 struct 目前没有
   实现任何 trait**（不是 `Clone`，不是 `Debug`，见 `:85-88`, `:226-229`, `:305-308`）——
   收敛到 `ScriptInvocationResult` 时如果调用方（测试代码里大量用 `.expect("...")` 解构，
   见 `script_backend.rs` 测试模块多处 `.expect("lua result")` 等）依赖了具体类型的某些
   inherent 方法而非字段直接访问，迁移时需要过一遍每个测试断言，不能只做"字段名不变
   就假设兼容"的浅层核对。

4. **rh 的 `Check` 分支双路径（cached-pack 场景）如果被 trait 化时不小心简化掉**，会
   静默破坏"`AGENTERM_RH_PACK` + 空 source 调用"这个已经在生产形态里使用的部署路径
   （`resolve_rh_pack` 里 `cached_rh_pack()` 优先于 source 解析这条逻辑同时服务于 Check
   和 Run/Eval 两个分支，`:112-127` 和 `:129-135` 之间没有代码共享但语义呼应——Trait-M2
   把这段逻辑原样搬进 `RhEngineBackend::check`/`execute` 时必须保留这个呼应关系，不能
   因为"trait 方法看起来应该独立"就把 cached-pack 检测逻辑只留在一边）。

5. **`ScriptEngineError = String` 这一步收敛是不可逆的信息损失点**（已在 §2.2/§3 讨论）——
   如果未来任何一个 backend 需要把结构化错误传回上层做更细的 `ScriptFailureCategory`
   分类（目前不需要，但"目前不需要"不等于"永远不需要"），trait 签名需要改一次
   breaking change。本设计判断：现状三引擎的错误已经统一拍平成
   `ScriptFailureCategory::Configuration`（`execute_inner:623,661,702` 逐一确认），
   这个收敛点不是本次引入的新损失，只是把已经发生的损失从"三处独立发生"变成
   "trait 签名显式承认"——如果后续要恢复结构化错误，那是一个独立的、需要先改
   `execute_inner` 错误处理逻辑的任务，不是本 trait 设计范围内能提前做对的事。

---

*本文档为设计稿，未落地代码；未修改仓库内任何 `.rs` 文件。*
