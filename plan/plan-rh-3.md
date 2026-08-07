# rh-3：AOT 扩面 + agenterm-rh 运行时成长

| 字段 | 值 |
|------|-----|
| **前置** | rh-0→rh-2 已合并 `main`（试切换、`./rh-check.sh`、M15 PRD） |
| **日期** | 2026-08-06 |
| **状态** | **M27 rh 前门切换完成**（根包构建；task/worker 直承载；supervisor 默认解析 rh） |
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
| M22f | **默认 rh 后端**（`AGENTERM_SCRIPT_BACKEND=rh`）；bootstrap/worker 注入；删除 Rhai check-many 回退 | [x] |
| M22 | 替换轨：`agenterm-rhai` 薄壳 + rh 默认执行（Candidate 六 cell 改名仍待人审） | [x] |
| M23a | for-loop 纯 int / `.len` range 原生 AOT（`for x in 1..5`、`for i in 0..arr.len()`） | [x] |
| M23b | rh `check` parity：`import`/project root + API catalog 对齐 rhai lint 语义 | [x] |
| M23c | caller wave 1：CI / bootstrap 运营引用清单化迁移（`caller-inventory` 基线 guard） | [x] |
| M23d | `agenterm-rhai` shim 硬化：剩余 dev forward 路径（check/eval/run/version/worker） | [x] |
| M24a | 原生 `break`/`continue` in for/while（reject try 内与带值 break） | [x] |
| M24b | check-many host 校验：project imports + shipped API catalog（`api_validate`/`project_import`） | [x] |
| M24c | bootstrap wave 1：`AGENTERM_BOOTSTRAP_RH_CLI` 注入；check.rhai 优先 rh CLI | [x] |
| M25a | `agenterm-rh task` 前门：显式转发未迁移 task 引擎到相邻兼容 PE，保留退出码 | [x] |
| M25b | bootstrap 默认通过 rh task 前门启动；`AGENTERM_RHAI_COMPAT_CLI` 明示兼容边界 | [x] |
| M25c | task 前门黑盒：成功列出 manifest；兼容 PE 缺失时硬失败 | [x] |
| M25d | framed-worker 捕获 compat fallback `print`，按输出预算封入结果帧，禁止协议 stdout 污染 | [x] |
| M26a | project import 编译校验统一到 `agenterm-rh::project_import` SSOT，主库仅留 resolver 与薄适配 | [x] |
| M26b | artifact verification / client smoke manifest 驱动验证 rhai + rh 双 PE offline probe | [x] |
| M26c | worker / framed / REPL / execute 从 `agenterm-rhai` bin 下沉 `script_worker` 主库模块 | [x] |
| M26d | worker check 直接保留 typed API validator failure；迁移后 22 个 worker 单测全绿 | [x] |
| M27a | 根包拥有并构建 `agenterm-rh` binary，解除 rh library ↔ 主库的 Cargo 环依赖 | [x] |
| M27b | `agenterm-rh` 直接承载 task、legacy worker 与 framed-worker，共享主库实现 | [x] |
| M27c | one-shot / persistent supervisor 默认解析 `agenterm-rh`，显式兼容回退 `agenterm-rhai` | [x] |
| M27d | supervisor 默认注入 `AGENTERM_SCRIPT_BACKEND=rh`；诊断报告实际 worker 与候选名称 | [x] |

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

- ~~不默认 `AGENTERM_SCRIPT_BACKEND=rh`~~ → **M22f 已默认 rh**；显式 `=rhai` 可回退
- 不迁移 62 task manifest 文件名（compat-delegating 继续跑 `.rhai`）
- 不引入 Cranelift / 字节码 JIT
- ~~不替换 `agenterm-rhai` worker/repl/task~~ → **pack 热路径已 rh**；REPL/复杂语句仍 Rhai 回退
- 不移除 `rhai` crate 依赖（AST 解析 + host_eval 桥）

---

## 5. M23 扩面轨（rh-3 后续）

相对 M22 默认 rh 后端，M23 把 **原生 AOT 覆盖面**、**check 语义 parity**、**caller 清单 wave 1**、**薄壳 forward 硬化** 拆成四条可独立验收的叶。

| ID | 用户问题 | 交付 | 验收 | 非目标 |
|----|----------|------|------|--------|
| **M23a** | `for` range 仍部分 host eval | 纯 int 字面/`..` range 与 `.len()` 上界原生 emit | `fixtures/rh/for-range.rh` qualify；`rh_regression` 含 `for … in` 机器码 | 任意 host 表面迭代器；`for-in` 对象/map |
| **M23b** | rh `check` 与 rhai lint 对 import/catalog 不一致 | `agenterm-rh check` / check-many 校验 project imports + `script_api` catalog 可见性 | `./rh-check.sh`；与 rhai check-many 同 manifest 零 diff（允许 rh-only 扩展字段） | 重写 catalog；改 broker 权限 |
| **M23c** | CI/bootstrap 仍大量 `agenterm-rhai` 字符串 | wave 1：`.github/workflows/**`、`scripts/bootstrap.*` 运营引用改指向 `agenterm-rh` 或 env 中性名 | `caller-inventory` ≥400 hits 基线 guard；bootstrap+ci 类非零；wave 1 diff 可审 | 一次删光 432 引用；改 task manifest 文件名 |
| **M23d** | 薄壳 forward 边角仍漏 dev 路径 | `agenterm-rhai` 剩余 check/eval/run/version/worker 转发与错误码对齐 | `rh_cli_forward` + framed-worker 黑盒；无静默 Rhai 回退（除显式 `=rhai`） | 移除 `agenterm-rhai` PE；Candidate 六 cell 改名 |

**顺序：** M23a ∥ M23b（热文件不同）→ M23c（依赖 inventory 基线）→ M23d（整合 forward 面）。M23c 的 read-only guard 已落 `tests/rh_corpus` + `fixtures/rh/caller-inventory-baseline.json`。

---

## 6. 依赖与顺序

```text
rh-3a (while + eval) → rh-3b (assign + try) → rh-3c (check-many + worker parity)
        ↓
rh-4 corpus 报告 → M22 默认 rh + 薄壳
        ↓
M23a/b (AOT + check parity) → M23c (caller wave 1) → M23d (shim hardening)
        ↓
Candidate 六 cell 改名 / 全量 caller 清单（待人审）
```
