# rh：并行 AOT 编译轨（Rhai 能力对齐后切换）

| 字段 | 值 |
|------|-----|
| **文档** | pack 专用 **rh** 语言 + AOT 到机器码；与 upstream Rhai **并行**，能力对齐后 **薄切换** |
| 日期 | 2026-08-06 |
| 状态 | **rh-3 compat 轨**：58/58 task check + delegating pack（host v4） |
| 关联 | `plan/research-rhai-kernel-depth.md` §11、`plan/plan-v0.1.18.md`（product App 使用 QJS）、`plan/design-rhai-rust-boundary.md` |

---

## 1. 目标

1. **并行** 建设 `crates/agenterm-rh` + `agenterm-rh` CLI，不替换现有 `agenterm-rhai`。
2. rh 语法 **首版 Rhai 兼容子集**；能力对齐后宿主只换 **backend**，catalog/Facade **不改**。
3. 终态：pack 发布物含 **signed native artifact**（六 cell）；解释执行仅 dev 路径。

---

## 2. 切换策略（少改代码）

```text
script_stdlib / script_fleet / script_*   ← 不变（L2 Facade 注册）
        │
        ▼
script_backend.rs   ← 唯一切换点（AGENTERM_SCRIPT_BACKEND=rhai|rh）
        │
   ┌────┴────┐
   Rhai      rh AOT (.so / 进程内 blob)
 Engine     dlopen + rh_host_eval → 同一 Engine API 表
```

| 层 | Rhai 期 | rh 切换后 |
|----|---------|-----------|
| Facade 注册 | `configure_engine` | native 热路径 + **host eval** 复用同一表 |
| pack 入口 | `Engine::eval` | `rh_entry()` 机器码 |
| catalog / smoke | script_api 2 | **不变** |
| broker / 预算 | script_protocol | **不变** |

**原则：** rh 是 **执行后端替换**，不是重写 `script_fleet.rs`。

---

## 3. 里程碑

| ID | 交付 | 状态 |
|----|------|------|
| M0–M7 | rh-0：check/transpile/compile/pack/worker 切换 | [x] |
| M8–M9 | rh-1：fleet shim + broker 派发 | [x] |
| M10 | rh-2：`rh_host_eval` + Rhai 引擎宿主复用 | [x] |
| M11 | rh-2：源码 hash 缓存 AOT（`script_rh_cache`） | [x] |
| M12 | rh-2：`AGENTERM_SCRIPT_BACKEND=rh` 可无 pack 跑 source | [x] |
| M13 | 试切换：stdlib fixture + `std::fs::exists` native 验收 | [x] |

### rh-3（AOT 扩面 + agenterm-rh 成长）

| ID | 交付 | 状态 |
|----|------|------|
| M14 | `while` 纯 int 条件原生 AOT | [x] |
| M15 | `agenterm-rh eval` dev 命令 | [x] |
| M16 | 赋值/复合赋值 + while-count | [x] |
| M18 | `agenterm-rh check-many` | [x] |
| M19–M21 | bootstrap 构建、Run parity、fixture corpus | [x] |
| M17 | `try`/`catch` 子集 | [x] |
| M21 | `corpus-scan` on scripts/rhai + `--tasks` | [x] |
| M22a | `caller-inventory` operational reference report | [x] |

执行计划：[`plan-rh-3.md`](plan-rh-3.md)。**JIT** 在本轨指 **T0–T1 分层 AOT**（源码缓存 + 原生扩面），Cranelift 仍在 RH-4。

---

## 4. rh-2 / rh-3 语言与 host eval

**允许（rh-3 在 rh-2 基础上）：** rh-2 全部 + **`while`（纯 INT 条件）** 原生 AOT。  
**允许（rh-2）：** rh-1 全部 + `for`、字符串、`throw`、任意 `std::`/`rhai::`/对象链（经 host eval）。  
**允许（compat 轨）：** 子集/AOT emit 失败时整脚本经 `rh_host_run_script` 走完整 Rhai worker（import、对象、复杂度与 rhai 等价）。

**机制：**
- 纯 `INT` 控制流/算术 → 原生机器码
- `fleet.*` → `rh_fleet_call` → broker（快路径）
- 字符串字面量 `std::fs::exists` → host v4 typed filesystem callback，
  不构造 Rhai Engine；动态路径暂留 host eval
- 其余 Rhai 表达式 → `rh_host_eval_int(snippet, scope)` → 宿主 **同一** `configure_engine` Rhai 引擎

**fixture：** `fixtures/rh/stdlib.rh`（`std::fs::exists` → entry 42）

---

## 5. 试切换步骤

1. 设置 `AGENTERM_SCRIPT_BACKEND=rh`
2. 提供 rh 源码（`fn entry() { ... }`）或 `AGENTERM_RH_PACK`
3. worker `execute_inner` → `try_execute_rh_invocation` → AOT 缓存 → dlopen → `rh_entry()`
4. 含 fleet 时自动注册 broker bridge

**回退：**  unset `AGENTERM_SCRIPT_BACKEND` 或设为 `rhai`（默认）。

---

## 6. 编译管线

```text
pack/*.rh  →  parse (Rhai AST)
          →  subset validate (rh-2)
          →  transpile → generated.rs (native + host eval calls)
          →  rustc → rh_pack.so (owned i64 ABI; no Rhai crate dependency)
          →  manifest native_hash
          →  dlopen @ load / script_rh_cache
```

The parser and host compatibility callbacks still use Rhai during migration,
but neither native subset packs nor compatibility-delegating pack stubs embed
the Rhai runtime.

---

## 7. 开放项（RH-*）

| ID | 问题 |
|----|------|
| RH-1 | rh-0 是否启用 `no_module` 依赖裁剪？ |
| RH-2 | native artifact 是否独立于 Base PE qualification？ |
| RH-3 | **试切换已可用**；全量 task manifest（62 脚本）需逐脚本 rh-2 校验 |
| RH-4 | import/模块图、Cranelift 直出、签名 OTA、gateway PE |

---

## 8. 验收

| 能力 | 证据 |
|------|------|
| **rh 专用测试套件** | `./rh-check.sh`（或 `scripts/rh-check.cmd`） |
| rh crate 单元 | `cargo test -p agenterm-rh` |
| AOT 集成 | `tests/rh_aot_smoke` |
| 回归/子集 | `tests/rh_regression` |
| 后端试切换 | `tests/rh_backend`（`AGENTERM_SCRIPT_BACKEND=rh`） |
| CI 策略 | `tests/rh_aot_ci_policy` |
| host eval | `script_rh_host` lib tests |
| 源码缓存 | `script_rh_cache` lib tests |

**后续轨：** 全 task `.rhai`→`.rh` 迁移、gateway Logic Pack、`llm.*`、Cranelift、签名 OTA。

---

## 9. 非目标（当前）

- 不在此轨替换 task manifest 全部 62 脚本（需逐脚本验收）
- 不阻断近程 server/CLI 主轨
