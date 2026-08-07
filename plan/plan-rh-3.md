# rh-3：AOT 扩面 + agenterm-rh 运行时成长

| 字段 | 值 |
|------|-----|
| **前置** | rh-0→rh-2 已合并 `main`（试切换、`./rh-check.sh`、M15 PRD） |
| **日期** | 2026-08-06 |
| **状态** | **M42 推进中；M42d1–d5 + M42e1 已就绪；validate-artifact-manifest 原生 `.rh` 迁移落地中** |
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
| M28a | incremental RUSTC wrapper 下沉主库，rh/rhai 双 PE parity；权威黑盒改测 rh | [x] |
| M28b | bootstrap 仅构建、缓存并执行 `agenterm-rh`，移除无消费者的 compat 环境接线 | [x] |
| M28c | CI 与 dist task caller wave 2 改用 rh；caller inventory 保持单调下降 guard | [x] |
| M28d | rh check/check-many 保持既有 typed JSON、退出码与项目根路径完整性契约 | [x] |
| M29a | isolated `agenterm-rh` CLI 套件：无相邻 rhai 的 help/check/check-many/task 契约 | [x] |
| M29b | check-many 全 fixture 与 per-file/aggregate/wall-time 预算 typed limit 矩阵 | [x] |
| M29c | for range/dynamic range/break-continue 真实 AOT qualify；span 超界 fallback | [x] |
| M29d | rhai shim 仅转发 `.rh` eval/run，保留 inline eval 与 `.rhai` 解释执行 | [x] |
| M29e | crate 外部 public API contract 套件纳入 `rh-check` | [x] |
| M30a | migration-audit 对齐 rh-only bootstrap 与跨平台 `rh-check` 入口；失败保持非零 gate | [x] |
| M30b | fresh-clone/startup/script smoke 观测 rh primary worker；兼容 REPL/framed/north-star 明确保留 | [x] |
| M30c | Candidate/performance 的 manifest task caller 改走 rh；密封 artifact 身份与 Promotion 路径不变 | [x] |
| M30d | compat unit/非整数结果保留类型，host callback 错误返回 typed failure；黑盒与后续健康调用覆盖 | [x] |
| M30e | caller inventory 降至 399，CI 19→12、rhai-script 39→32；继续以分类下限防扫描器静默失效 | [x] |
| M31a | 生成 native/compat pack 使用自有 `i64` ABI，生成 crate 删除 Rhai runtime 依赖；parser/host compat 明确保留 | [x] |
| M31b | host API v4 为字符串字面量 `std::fs::exists` 提供 typed 快路径；保留 v2/v3 pack 注册兼容 | [x] |
| M32a | task manifest/corpus 接受 `.rh` entry；公共 CLI 执行首个原生 named-task，生成代码资格门禁止 `rh_host_run_script`/`rh_host_eval_int` | [x] |
| M33a | host API v5 暴露 typed `args.len`；native task 以两个真实调用参数返回 `12`，旧 v2-v4 pack 注册兼容保留 | [x] |
| M34a | host API v6 以 bounded UTF-8 callback 暴露 `args[index]`；原生字符串长度按 Unicode scalar 计数，越界返回 typed host failure | [x] |
| M35a | `std::fs::exists` 接受 native UTF-8 参数绑定并直接调用 typed Rust callback；named task 以真实 `Cargo.toml` 路径资格验证 | [x] |
| M36a | host API v7 提供 bounded UTF-8 文件读取；native 字符串绑定支持字面量 `contains`，named task 验证真实 manifest 内容 | [x] |
| M37a | native pack 直接使用 Rust `Path::join` 生成 UTF-8 路径；组合结果可供 exists/read callback 使用且不触发解释器 | [x] |
| M38a | host API v8 提供 typed native failure 与 case-exact 文件检查；`verify-docs-site` 从活跃 `.rhai` 迁至零回退 `.rh` 并归档旧实现 | [x] |
| M39a | Candidate、Promotion 与发布索引步骤统一通过 `agenterm-rh` 执行脚本；工作流静态门禁止恢复 `agenterm-rhai` 活跃入口 | [x] |
| M40a | host API v9 通过通用 utility ABI 提供无命令白名单、带超时和进程树清理的 `std::process::command_status`；`internal-version-policy` 零回退迁移并归档旧实现 | [x] |
| M41a | 无显式 `fn entry()` 的顶层 `.rhai` 强制整脚本 compatibility execution，禁止生成返回 0 的 Native stub；无 entry 的 `.rh` named task 由资格门 fail-closed 拒绝，codegen cache revision 同步失效旧包 | [x] |
| M42a | native pack 直接解析通用 JSON Value，并原生读取、比较整数对象属性；资格测试执行真实 native pack，静态门证明零 `host_eval` / `run_script`，codegen cache revision 同步失效旧包 | [x] |
| M42b | JSON 对象属性链原生读取数组长度，`for` 原生遍历数组 Value 并读取元素整数属性；fixture 真实编译、加载、执行且静态门证明零 `host_eval` / `run_script` | [x] |
| M42c | 原生 `type_of`、JSON 字符串属性绑定、字符串比较与字面量拼接；fixture 真实执行且静态门证明零 `host_eval` / `run_script` | [x] |
| M42d1 | 原生字符串方法（`starts_with`/`ends_with`/`contains` 动态 needle、`trim`、`replace`）与 `for character in string` 字符遍历；`string-validate.rh` fixture 真实执行且静态门证明零 `host_eval` / `run_script` | [x] |
| M42d2 | 原生动态 `rh::fail`/`throw`/`require(cond, msg)`，消息可为字符串拼接表达式；`fail-dynamic.rh` fixture 真实执行且静态门证明零 `host_eval` / `run_script` | [x] |
| M42d3 | 原生 bool-keyed MapSet：空 `#{}`、`.contains(string)`、`names[key]=true` 插入；`map-set-membership.rh` fixture 真实执行且静态门证明零 `host_eval` / `run_script` | [x] |
| M42d4 | 原生 `std::path::absolute(...).display` 与 `std::fs::symlink_metadata` + `Metadata.is_file/is_symlink/is_reparse_point`；`path-metadata-probe.rh` fixture 真实执行且静态门证明零 `host_eval` / `run_script` | [x] |
| M42d5 | 项目相对 `import "…" as alias` 扁平化为单脚本，改写 `alias::fn` 为本地 INT 函数调用；`import-bundle-probe.rh` fixture 真实执行且静态门证明零 `host_eval` / `run_script` | [x] |
| M42e1 | Json 字符串绑定可走 `.starts_with`/`.trim`/`MapSet` key；本地 fn 按体推断 `String` 形参；原生 `print`（utility op 4）；任务编译缓存可按 `project_root` 打包 import；`string-fn-bundle.rh` 资格门 | [x] |
| M42d | 无损迁移 `validate-artifact-manifest`；不得用 substring 或任务专用宿主校验器替代脚本不变量 | [x] |
| M42e2 | `project_import` / corpus 原生门优先解析 `.rh` 模块，并对任务入口使用 `transpile_cdylib_with_project` | [x] |
| M42e3 | 语句位置 `if` 不再强制分支尾 `return`；`require` 在 `Stmt::Expr` 下按语句发射；codegen revision 14；原生嵌套 `json::parse(read_to_string(...))`；`validate-artifact-manifest` 真实执行返回可执行文件计数 | [x] |
| M42f0 | `internal-version-policy` 已原生 `.rh` 任务入口并真实执行（print + process_status + string contains） | [x] |
| M42f1 | 原生 `read_dir` / `remove_file` / `try_remove_file` + 链式 metadata 标志；`clean-locked-artifacts` 任务入口切到 `.rh`；codegen revision 15 | [x] |
| M42f2 | 原生 `copy` / `create_dir_all` / `rename`（及 try_*）；codegen revision 16；解锁 `stage-artifact` | [x] |
| M42f3 | 原生 `std::time::SystemTime::now().unix_millis`；codegen revision 17 | [x] |
| M42f4 | 无损迁移 `stage-artifact`（INT-only `stage`/`stage_as`，try_copy/try_rename，无 try 内 return） | [x] |

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
