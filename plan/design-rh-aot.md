# rh：并行 AOT 编译轨（Rhai 能力对齐后切换）

| 字段 | 值 |
|------|-----|
| **文档** | pack 专用 **rh** 语言 + AOT 到机器码；与 upstream Rhai **并行**，能力对齐后 **薄切换** |
| 日期 | 2026-08-06 |
| 状态 | 在制小项 rh-0（并行，不挡 server/CLI 主轨） |
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
| M5 | CC in-process `dlopen` + `cc_lines` 原生呈现 | [x] |
| M6 | 六 cell CI + qualification hash | [ ] |

---

## 4. rh-0 语言子集

**允许：** `fn`、 `let`/`const`、 `if`/`else`、 `return`、字面量、二元运算、`()` block。  
**禁止（rh-0）：** `eval`、`import`/`export`、循环、`try/catch`、闭包捕获、动态模块。

校验：`agenterm-rh check`；失败给出 rh 原因码。

---

## 5. 编译管线（目标）

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

## 6. 开放项（RH-*）

| ID | 问题 |
|----|------|
| RH-1 | rh-0 是否启用 `no_module` 依赖裁剪？ |
| RH-2 | native artifact 是否独立于 Base PE qualification？ |
| RH-3 | 切换窗口：`AGENTERM_SCRIPT_BACKEND=rh` 默认化版本？ |

---

## 7. 非目标（rh-0）

- 不替换 `agenterm-rhai` 自动化/task manifest 路径
- 不在 rh-0 实现 parser/grid/server 内嵌
- 不阻断近程 server/CLI 主轨
