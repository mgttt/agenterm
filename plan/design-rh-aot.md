# rh：并行 AOT 编译轨（Rhai 能力对齐后切换）

| 字段 | 值 |
|------|-----|
| **文档** | pack 专用 **rh** 语言 + AOT 到机器码；与 upstream Rhai **并行**，能力对齐后 **薄切换** |
| 日期 | 2026-08-06 |
| 状态 | rh-1 闭环完成（AOT + 宿主回调 + fleet shim + broker 派发） |
| 关联 | `plan/research-rhai-kernel-depth.md` §11、`plan/agenterm-rhai-app.md`、`plan/design-rhai-rust-boundary.md` |

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
 Engine     dlopen + 同一 register_* 表
```

| 层 | Rhai 期 | rh 切换后 |
|----|---------|-----------|
| Facade 注册 | `configure_engine` | native shim 调同一 Rust fn |
| pack 入口 | `Engine::eval` | `rh_entry()` 机器码 |
| catalog / smoke | script_api 2 | **不变** |
| broker / 预算 | script_protocol | **不变** |

**原则：** rh 是 **执行后端替换**，不是重写 `script_fleet.rs`。

---

## 3. rh-0 里程碑（当前）

| ID | 交付 | 状态 |
|----|------|------|
| M0 | `agenterm-rh check` — 子集校验 | [x] |
| M1 | `agenterm-rh transpile` — AST → Rust 源 | [x] 纯函数子集 |
| M2 | `script_backend` + `AGENTERM_SCRIPT_BACKEND` | [x] |
| M3 | AOT compile → `.so` + manifest + dlopen smoke | [x] |
| M5 | CC in-process `dlopen` + `cc_lines` 原生呈现 | [x] |
| M6 | 六 cell CI + qualification hash | [x] |
| M7 | `AGENTERM_SCRIPT_BACKEND=rh` worker 原生 entry 派发 | [x] |
| M8 | Fleet native shim：`rh_register_host` + `fleet.*` transpile | [x] |
| M9 | worker broker 派发 + fleet fixture 验收 | [x] |

---

## 4. rh-1 语言扩展（fleet）

**允许：** rh-0 全部 + `fleet.protocol.info()` 等 **零参/单整数** fleet 查询/变更。  
**实现：** transpile → `rh_fleet_call("protocol.info", "{}")` → 宿主 C ABI → 同一 `fleet.call` broker。  
**fixture：** `fixtures/rh/fleet.rh`；`rh_aot_smoke` + `script_rh_host` 黑盒。

---

## 5. rh-0 语言子集

**允许：** `fn`、 `let`/`const`、 `if`/`else`、 `return`、字面量、二元运算、`()` block。  
**禁止（rh-0）：** `eval`、`import`/`export`、循环、`try/catch`、闭包捕获、动态模块。

校验：`agenterm-rh check`；失败给出 rh 原因码。

---

## 6. 编译管线（目标）

```text
pack/*.rh  →  parse (Rhai AST, 临时)
          →  subset validate
          →  transpile → generated.rs
          →  rustc / cc  →  rh_pack.so
          →  签名 + .agp manifest native_hash
          →  Base dlopen @ pack load
```

首版后端：**transpile → Rust → rustc**（最贴现有栈）。Cranelift 直出为 M6+。

---

## 7. 开放项（RH-*）

| ID | 问题 |
|----|------|
| RH-1 | rh-0 是否启用 `no_module` 依赖裁剪？ |
| RH-2 | native artifact 是否独立于 Base PE qualification？ |
| RH-3 | 切换窗口：`AGENTERM_SCRIPT_BACKEND=rh` 默认化版本？ |

---

## 8. rh-0 闭环验收

| 能力 | 证据 |
|------|------|
| 子集校验 | `agenterm-rh check` |
| AOT 编译 | `agenterm-rh compile` / `pack build` |
| native_hash | manifest + `verify_native_hash` on load |
| dlopen entry | `rh_entry()` + `run-smoke` |
| cc_lines C ABI | `rh_cc_line_*` 静态导出 |
| CC / gateway 观测 | `AGENTERM_RH_PACK` + `protocol-info.script` |
| CLI 诊断 | `agenterm-cli rh-pack --path` |
| 六 cell | CI: host `cargo test -p agenterm-rh` + cross `AGENTERM_RH_QUALIFY_TARGET` |
| worker 切换 | `execute_inner` → `try_execute_rh_invocation` when backend=rh |
| fleet shim | native `rh_fleet_call` → host → `fleet.call` broker |

**未纳入 rh-1（后续轨）：** 全 fleet 表覆盖、gateway 独立 PE、`llm.*` Logic Pack、签名 OTA、Cranelift 直出。

---

## 9. 非目标（rh-0）

- 不替换 `agenterm-rhai` 自动化/task manifest 路径
- 不在 rh-0 实现 parser/grid/server 内嵌
- 不阻断近程 server/CLI 主轨
