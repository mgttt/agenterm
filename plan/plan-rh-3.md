# rh-3：AOT 扩面 + agenterm-rh 运行时成长

| 字段 | 值 |
|------|-----|
| **前置** | rh-0→rh-2 已合并 `main`（试切换、`./rh-check.sh`、M15 PRD） |
| **日期** | 2026-08-07 |
| **状态** | **进行中 M42f6**（cg29 + g13–g20）：`target-report`/`package-qualified`/`package-release-qualified` 已切 `.rh`。余叶 `prune`/`candidate-*` 与 M42f7 smoke/orch 仍属后续 |
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
| M18 | rh-3c：`agenterm-rh check-many`（bounded manifest，对齐 lint.rh） | [x] |
| M19 | rh-3c：bootstrap / CI 默认构建 `agenterm-rh` 二进制 | [x] |
| M20 | rh-3d：worker 路径 `Run`/`Eval` 黑盒 parity（rh_backend 扩展） | [x] |
| M21 | rh-4：task corpus 扫描器（62 脚本 rh-2/3 校验报告，不强制迁移） | [x] |
| M22a | M22 预备：`caller-inventory` + `corpus-scan --tasks` 机器可读报告 | [x] |
| M22b | worker parity：`RhRunContext` args/project_root、`host_eval`/`host_run_script` 注入、framed-worker 黑盒 | [x] |
| M22c | check-many 薄转发兼容：rhai CLI/manifest kind、bootstrap.cmd 对称、forward 黑盒 | [x] |
| M22d | lint.rh 优先 `agenterm-rh` check-many；artifacts/stage-build 纳入 dev CLI | [x] |
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
| M42f5 | 无损迁移 `stage-build`：复用原生 `stage`/`stage_as`/`clean_locked`，收口仍挂 Rhai 的共享 artifact 编排；任务入口切 `.rh` 并归档旧 `.rhai` | [x] |
| M42f5a | 原生 `std::process::command_stdout_file(program, args, timeout_ms, path) -> INT`（utility op 5；无命令白名单；stdout 落盘后由 `read_to_string` 消费）；codegen 18；解锁 git `--show-prefix` 等需 stdout 的检查 | [x] |
| M42f5b1 | pack 原生 `env::get/has`、`sha256_file`（生成 crate 加 sha2）、`atomic_write`、`SystemTime.now().rfc3339`、`Metadata.len`、`to_lower`；codegen 19 | [x] |
| M42f5b2 | JSON 对象构建 / `stringify_pretty`、字符串 `split` 线迭代、`json_array_push`/`json_array_get`；codegen 20；bool 字面量赋值放行 | [x] |
| M42f5b3 | INT-only `scripts/rh/lib/build_metadata.rh`（`write`/`write_platform` 返回 0；过程调用改 `command_stdout_file`/`command_status`；transpile 收紧 JSON 形参推断 + join/read_to_string JSON 子路径；资格门覆盖 stringify+atomic_write+sha256） | [x] |
| M42f5c | `scripts/rh/stage-build.rh`；任务 entry 切 `write-build-metadata`/`stage-build` → `.rh`；归档旧 `.rhai`；`stage`/`stage_as` 只读 INT 0/1，不假设 map；黑盒串起真实写盘 | [x] |
| M42f6 | rh 全面替换剩余 `.rhai` 任务入口（不保留解释兼容为目标）；按可验证切片推进并归档旧脚本 | [ ] |
| M42f6a | INT-only `scripts/rh/prepare-target-clean.rh`；`command_status`/`command_stdout_file`+`atomic_write`；entry 切 `.rh` 并归档 | [x] |
| M42f6b | INT-only `scripts/rh/lib/build_identity.rh` + `build-identity.rh`（写 batch env，返回 0）；entry 切 `.rh` 并归档 | [x] |
| M42f6c | INT-only `scripts/rh/bootstrap-info.rh`（`command_stdout_file`+JSON report）；entry 切 `.rh` 并归档 | [x] |
| M42f6d | INT-only `scripts/rh/timing-summary.rh`（`read_to_string`+JSON validate、`atomic_write` summary）；entry 切 `.rh` 并归档 | [x] |
| M42f6e | 原生 `command_status`/`command_stdout_file` 可选 trailing options map（`current_dir`/`env`/`env_remove`）；codegen 21 | [x] |
| M42f6f | 原生 `std::fs::metadata`（与 `symlink_metadata` 对齐的 `.is_file/.is_dir/.len`）、`std::env::current_dir().display`、`PathBuf::from`+`.is_absolute`/`.display`、`json::parse_file` 糖 → `parse(read_to_string)`；codegen 22；fixture `path-metadata-sugar.rh` | [x] |
| M42f6f2 | 原生 `DirEntry.metadata` + `.len` / `.modified.unix_millis` / `.modified.rfc3339`（及 `std::fs::metadata(path).modified.*` 链）；codegen 23；fixture `direntry-metadata-probe.rh` | [x] |
| M42f6f3 | `type_of` 对缺失/null JSON 路径返回 `"()"`；语句位 `try/catch` 以 `let _ = match` 发射；codegen 24 | [x] |
| M42f6g | 易切叶任务 cutover（依赖 6e/6f）：`mcp-conformance`、`performance-samples`、`agenterm-net-research`、`readme-examples`；INT-only `.rh` + entry 切线 + 归档；全改 `command_*`+options，禁止假兼容 | [x] |
| M42f6g2 | INT-only `scripts/rh/performance-summary.rh`；entry 切线 + 归档；sccache map 用 `.Rust` 点取（完整 `keys()` 仍属 M42f6h） | [x] |
| M42f6g3 | INT-only `scripts/rh/build-releases-index.rh`；entry/workflow 切线 + 归档；可选 checksum/sbom/build_log 用 `type_of == "string"` | [x] |
| M42f6g4 | INT-only `scripts/rh/rh-aot-smoke.rh`；`command_status`/`command_stdout_file`+cwd options；entry 切线 + 归档 | [x] |
| M42f6g5 | 原生 JSON `obj.keys()` 迭代 + `obj.keys().len`、MapSet `.keys()`；codegen 25；fixture `json-keys-probe.rh` | [x] |
| M42f6g6 | 原生 INT→String：`s += n`、`"prefix-" + n` 链式 concat；codegen 26；fixture `int-string-concat-probe.rh`（解锁 path 拼 millis/序号） | [x] |
| M42f6g7 | 原生 JSON 路径下标字符串：`obj.field[i]` / `obj.a.b[i]` → `rh_json_string_path_index`；DirEntry `file_name` stringish；codegen 27；fixture `json-path-index-probe.rh`；解锁 `client-smoke` | [x] |
| M42f6g8 | INT-only `scripts/rh/client-smoke.rh`；entry 切线 + 归档 | [x] |
| M42f6g9 | INT-only `scripts/rh/preflight-benchmark.rh`；entry 切线 + 归档 | [x] |
| M42f6g10 | INT-only `scripts/rh/cross-platform-automation-audit.rh`；entry 切线 + 归档 | [x] |
| M42f6g11 | INT-only `scripts/rh/artifact-verification.rh`；entry 切线 + 归档 | [x] |
| M42f6g12 | codegen 28：`for` 内 `array.push`（既有 emit 固化单测）、异构 JSON 数组字面量 `[doc.a, doc.b]`、`parts[0].len` 误解析恢复、MapSet `seen[doc.id]=true` stringish 键；fixture `json-array-*-probe.rh` / `string-list-index-probe.rh` | [x] |
| M42f6g13 | INT-only `scripts/rh/lint.rh`；entry 切线 + 归档；回归 `lint_*` | [x] |
| M42f6g14 | INT-only `scripts/rh/supply-chain.rh`（去 string helper；`let stored = arr[i]` 绑定再比较；pack compile 通过）；entry 切线 + 归档；回归 `supply_chain_*` | [x] |
| M42f6g15 | INT-only `scripts/rh/powershell-migration-audit.rh`；`migration-audit` entry 切线 + 归档；回归 `migration_audit_*` | [x] |
| M42f6g16 | INT-only `scripts/rh/prd-alignment.rh`；entry 切线 + 归档；回归 `prd_alignment_*` | [x] |
| M42f6g17 | INT-only `scripts/rh/preflight.rh`；entry 切线 + 归档；回归 `preflight_*`（非 preflight-benchmark） | [x] |
| M42f6h | `release_candidate`/`qualification`/`package_qualified` lib 原生移植 + 剩余叶（`candidate-*`、`migration-audit`、Wave3：`prd-alignment`/`prune`/`powershell`/`preflight`/`target-report`）；`target-report` 仍缺 `pop`/float。**codegen 28 后**：`array.push`/JSON 数组字面量已解；`path.parent` 与 Child/sleep 仍属后续；入口仍停 `.rhai` 直至叶达 Native | [ ] |
| M42f6i | INT-only `scripts/rh/finalize-macos-provenance.rh`（`symlink_metadata`+`parse(read_to_string)`、重建 JSON 设 `notarized:true`）；candidate workflow 切 `.rh` 并归档 | [x] |
| M42f7 | `test_harness` + smoke/orch：Child+sleep + 全量 `*-smoke`；`switch`/`do` 编排体仍延后 | [ ] |
| M42f7a | codegen 30：`std::process::id` → INT；fixture `process-id-probe.rh` | [x] |
| M42f7b | codegen 31：`std::path::parent(…).display`；fixture `path-parent-probe.rh` | [x] |
| M42f7c | codegen 32：`rhai::json::stringify`（compact）+ `rhai::runtime::append_sync` + `String.sub_string`；fixture `append-sync-probe.rh` | [x] |
| M42f7d | codegen 33：`std::fs::remove_dir_all`；fixture `remove-dir-all-probe.rh` | [x] |
| M42f7e | codegen 34：`Command` builder + `Command.output`/`Output`（success/exit_code/stdout_text/stderr_text/require_success）；timeout 用 INT ms（不引 Duration）；fixture `process-output-probe.rh` | [x] |
| M42f7f | codegen 35：`Command.start`/`Child`（id/state/kill/wait_with_output）；fixture `child-lifecycle-probe.rh` | [x] |
| M42f7g | INT-only `scripts/rh/lib/test_harness.rh` + `harness-cleanup-selftest` native+pack；entry 切线 + 归档；回归 | [x] |
| M42f8 | **硬切换收口（无兼容）**：Rhai 准备归档；不保留 `.rhai` 运行面。Phase A 清零 manifest/工作流/测试中的 `.rhai` 入口并全部改 `.rh`；Phase B 删除或归档 `scripts/rhai/**` 与 `agenterm-rhai` 业务路径，痕迹清扫（AGENTS/PRD/tests/workflows）；Phase C 移除主库 Rhai `Engine`/`script_rh_host` compat 与 `rh_host_run_script` 整脚本回退。compat-delegating 只是迁移期诊断，不是产品兼容承诺 | [ ] |
| M42f8a | 剩余 lib：`qualification`/`release_candidate`/`bootstrap_timing`/`script_smoke_helpers` → `scripts/rh/lib/*.rh` | [~] |
| M42f8b | orch：`build`/`check`/`release`/`fresh-clone-rehearsal` → `.rh` + entry 切线 + 归档 | [~] |
| M42f8c | selftest：`qualification-selftest`/`diagnostic-bundle-selftest`/`harness-cleanup-selftest` flip | [~] |
| M42f8d | 全量 `*-smoke` → `.rh` + entry 切线 + 归档（依赖 M42f7e/f + test_harness） | [~] |
| M42f8e | 仓库痕迹清扫：`scripts/rhai` 引用、`agenterm-rhai` 运营串、测试 pin、AGENTS/PRD | [~] |

**主控在 main 硬切换（2026-08-07）：** tip `f4a66d4` 已合入并推送 `origin/main`（rev40–42 flips）。并行背景任务推进 qualification JSON mutate 与 smoke/orch 探针。

**M42f7g / M42f8 进度（2026-08-07 tip +rev42）：** `[~]` = INT `.rh` 草稿已在树且 `check` 过，**未** flip（仍约 28 条 `.rhai`）。**rev 42**：`path.display.to_lower`（含嵌套 parent/absolute）；`split` 变量分隔符 + JSON 路径 + `.len`；`for` 本地 Json/StringList 返回；`String += stringish`→`push_str`；Child/Command 形参 `mut`；显式字符串表达式优先于 host-surface。**已 flip**：`harness-cleanup-selftest`、`build-identity`、`diagnostic-bundle-selftest`（Native he=1+pack）。**仍 Compat/阻塞**：`qualification-selftest`（JSON 字段/下标赋值：`timing.gates[i].status=…` 等，需原生 JSON mutate）。M42f8a–d 草稿齐。M42f8e 审计已落盘；Phase A 切线门禁 = Native+pack。


**M42f6 编号说明：** 设计稿曾把「process capture」叫做 M42f6a，但仓库已用 M42f6a–e 承接 prepare-target / build-identity / bootstrap-info / timing-summary / command options；后续缺口从 **M42f6f** 起编号。M42f6e 已覆盖 `command_*` 的 cwd/env；完整 `Command.output()` 文本捕获若仍缺，并入 6g 叶任务改写（`command_stdout_file`+`read_to_string`）而非再开权限向 allowlist。

**M42f5 依赖说明：** `stage-build.rhai` 本身编排（clean → stage/stage_as 循环 → metadata → clean）在 M42f4 后已大半可复用原生 `artifact_files`；真正阻塞是 (1) `git rev-parse --show-prefix` 需要 stdout（仅有 `command_status` 不够），(2) 内联 `build_metadata::write` 需要哈希/原子写/环境/RFC3339/JSON 序列化。禁止用 shell 包装或 `host_eval` 假迁移；禁止把 metadata 改成子进程调 Rhai 任务冒充原生入口。

**M42f4 park 后缀说明：** M42f3 已接线 `unix_millis`，但 `park_running_destination` 仍用 **`0..4096` 的 `try_rename` 序号**（`stem.locked-<n>.exe`），不取 wall-clock millis。原因：序号在同一目录内对冲突做确定性探测、不依赖时钟单调/并发同毫秒碰撞，且 `while` + INT 计数保持纯原生；millis API 留给其它需要墙钟戳的调用方。`stage`/`stage_as` 成功返回 **0**（直拷）/ **1**（park 后替换）；`stage-artifact` 任务入口丢弃该 INT。旧 Rhai 返回 `#{ destination, parked }` map——原生 INT 边界下，依赖 map 形返回的调用方（如未来的 `stage-build`）必须改为读 INT 或 print 侧效应，不得假设 map。

**manifest cutover 约定：** 任务入口切换只改 `agenterm.tasks.json` 对应 `entry` 行；禁止整表 JSON 重排（`fe645201` 曾误排，已由 `f9842005` 收回）。

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
