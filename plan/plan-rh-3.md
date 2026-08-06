# rh-3：AOT 扩面 + agenterm-rh 运行时成长

| 字段 | 值 |
|------|-----|
| **前置** | rh-0→rh-2 已合并 `main`（试切换、`./rh-check.sh`、M15 PRD） |
| **日期** | 2026-08-06 |
| **状态** | **进行中** |
| **SSOT** | [`design-rh-aot.md`](design-rh-aot.md) |

---

## 1. 目标（相对 agenterm-rhai）

1. **`agenterm-rh`** 从「AOT 工具链」成长为 **可独立 dev 的 rh 运行时 CLI**（check / eval / pack / qualify），最终 **薄替换** `agenterm-rhai` 的 pack 热路径；worker / repl / task manifest 仍分阶段。
2. **AOT 扩面**：减少 `rh_host_eval` 回退，把更多控制流与表达式 **原生 codegen**（transpile→rustc，非 Cranelift）。
3. **「JIT」产品定义**：本轨 **不做字节码 JIT**；采用 **分层执行**：
   - **T0** 源码 hash AOT 缓存（rh-2，已 ship）
   - **T1** 子集原生机器码扩面（rh-3）
   - **T2** 可选进程内增量 AOT / 模块图（rh-4，待 0.1.15 后）
   - **T3** Cranelift 直出（研究轨 RH-4，非 0.2.0 阻塞项）

边界不变：Fleet 权威、broker、预算、catalog 仍在宿主；rh 只换 **执行后端**（见 `design-scripting-boundary-comparison.md` §6.1）。

---

## 2. 里程碑

| ID | 交付 | 状态 |
|----|------|------|
| M14 | rh-3a：`while` 纯 int 条件原生 AOT + fixture | [x] |
| M15 | rh-3a：`agenterm-rh eval`（AOT + dlopen 一键 dev） | [x] |
| M16 | rh-3b：赋值/复合赋值 + `while` 可变异计数 | [x] |
| M17 | rh-3b：`try`/`catch` 子集 + 原生 throw 路径 | [x] |
| M18 | rh-3c：`agenterm-rh check-many`（bounded manifest，对齐 lint.rhai） | [x] |
| M19 | rh-3c：bootstrap / CI 默认构建 `agenterm-rh` 二进制 | [x] |
| M20 | rh-3d：worker 路径 `Run`/`Eval` 黑盒 parity（rh_backend 扩展） | [x] |
| M21 | rh-4：task corpus 扫描器（62 脚本 rh-2/3 校验报告，不强制迁移） | [x] |
| M22a | M22 预备：`caller-inventory` + `corpus-scan --tasks` 机器可读报告 | [x] |
| M22b | worker parity：`RhRunContext` args/project_root、`host_eval`/`host_run_script` 注入、framed-worker 黑盒 | [x] |
| M22c | check-many 薄转发兼容：rhai CLI/manifest kind、bootstrap.cmd 对称、forward 黑盒 | [x] |
| M22d | lint.rhai 优先 `agenterm-rh` check-many；artifacts/stage-build 纳入 dev CLI | [x] |
| M22e | CLI 薄转发黑盒（check/eval/run/version）；framed-worker entry fixture；`for` 整型 range 原生 AOT | [x] |
| M22 | 替换轨：`agenterm-rhai` → 薄转发或 rename（需全量 caller 清单 + Candidate 证据） | [ ] |

---

## 3. rh-3a 技术切片（本迭代）

### 3.1 `while`（纯 INT 条件）

- **允许**：`while <pure-int-expr> { ... }`，条件与 `if` 相同规则（`is_pure_int_expr`）。
- **禁止**：`do`/`switch`/`try`、host 表面条件（走 host eval 或 reject，rh-3a 先 reject 非 pure int）。
- **emit**：`while cond != 0 { ... }`（cdylib INT 语义）。

### 3.2 `agenterm-rh eval <file.rh>`

- check → temp pack dir → qualify → `load_and_call_entry` → 打印 `entry` 值与 `cc_lines`。
- 不启动 framed worker；供本地 rh dev 与 CI 快路径。

### 3.3 验收

- `./rh-check.sh` 全绿
- `fixtures/rh/while.rh` qualify entry=42
- `tests/rh_regression` 断言 transpile 含 `while`

---

## 4. 非目标（rh-3）

- 不默认 `AGENTERM_SCRIPT_BACKEND=rh`
- 不迁移 62 task manifest
- 不引入 Cranelift / 字节码 JIT
- 不替换 `agenterm-rhai` worker/repl/task（M18–M22）

---

## 5. 依赖与顺序

```text
rh-3a (while + eval) → rh-3b (assign + try) → rh-3c (check-many + worker parity)
        ↓
rh-4 corpus 报告 → 0.1.15 完成后 M15 全量迁移决策
        ↓
agenterm-rhai 薄替换 / rename（Candidate 六 cell + caller 清单）
```
