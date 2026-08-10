# v0.1.16 发布战役交接(2026-08-10 晨)

## 目标(用户授权,不可变)

让 main CI 在某个 sha 上**全绿**,然后走正规发布管线交付 v0.1.16(ISA×2 / OS×3):

1. 全绿后:`gh workflow run candidate.yml -f source_sha=<该 sha>`(preflight 硬性要求该 sha 有全绿 CI run)。
2. candidate 成功后:`gh workflow run release.yml -f candidate_run_id=<id> -f confirmation=publish-v0.1.16`。
3. 版本号已是 0.1.16;**不要**手动打 tag/release(此前手动发布因缺 lnx/osx 产物已撤销,release.yml 自己建 tag)。

已配置的 `gh` CLI 会话可用于 Actions 读取。远程 push 凭据不得查询、打印或复用为 API 凭据。共享 checkout，提交时只暂存已审查路径。

## 战况(截至 e7b8e774,已连推十波修复)

绿:windows-aarch64、linux-aarch64、全部 4 个 platform-contract、linux-x86_64 主质量门(771 单测+quick 链)。
macos-x86_64 一直被 fail-fast 取消,从未独立验证 — 注意它可能藏着与 aarch64 不同的红。

## 剩余问题(按优先级)

### 1. windows unit-tests 门红(最新已知第一失败)

timing artifact(`gh run download <run> --pattern "*timing*"`)显示 first_failure=unit-tests(exit 101)。
本地复刻(`cargo test --all-features` + 12 个 --skip,见 check.rh cargo_unit_primary_spec)只有一个失败:
**tests/fresh_clone_rehearsal.rs** → 任务自测死于 `rh_fail: fresh_clone_untyped_powershell:powershell.exe:101:powershell.exe:202`。

根因:**AOT 跨函数 throw/catch 不解卷**。`scripts/rh/fresh-clone-rehearsal.rh` 的 `run_self_test()`(第 363 行起)
故意让 `exact_terminal_payload()`(内部经 assert_ok throw)抛错并在调用方 try/catch 捕获。AOT 语义:被调函数内
throw = `rh_fail`(首错记录,run 结束必失败)+ 返回占位值继续执行 — catch 永远不触发。同文件还有两处同型
(release_archive_path 的 rejected_archive、以及 not_unique 用例)。

**推荐修法**(改脚本,别改转译器):把自测的否定用例改成直接谓词断言(不经过 throw-catch),正用例照旧调真函数。
注意 tests/fresh_clone_rehearsal.rs 第 45-79 行钉了一堆源码子串合同(含 `debug/agenterm-rh.exe`、
`rhai::runtime::temp_dir()` 等**陈旧钉**),改脚本会破坏这些钉 — 需同步更新该测试的钉清单(陈旧钉本来就该修)。

改转译器(全函数 Result 化)是大工程,不建议在发布前做。

### 2. windows 证据阶段 process_timeout(310s,可能已被缓解)

`process_timeout: <build-cache>\task-...\agenterm.exe after 310000ms` 反复出现且**先于**真失败被记录
(record_host_error 首错保留,见 src/script_rh_host.rs:761),掩蔽 unit-tests 详情。wave 9 已上共享 pack
target 缓存(temp/agenterm-rh-pack-target-cg<rev>,冷编译 30s→5s),wave 10 已给标签加 args 预览 —
下轮 CI 会自报是哪个 task。若仍超时,查 check.rh 里 evidence_list_spec/task_check_spec(现 300s 墙)。

### 3. macos-aarch64:control-center-macos-smoke 运行期

转译错误已修(e7b8e774)。下一层未知 — 该 smoke 从未在 AOT 下跑通过,可能继续暴露解释器/AOT 语义分歧
(本战役已修过的同族:缺席读 0/""、`.len` 字符串、env_remove 顺序、`0+` Bool 化、map 字面量、跨函数 throw)。
诊断组合拳:scratchpad 的 rhdump 工具(transpile_cdylib_with_project dump 生成 Rust)+
`agenterm rh pack build <script> --dir <tmp>`(留 rustc stderr)+ 最小 .rh 探针脚本。

### 4. 流程惯例

- 每轮:提标签 `gh run view --job <id> --log | Select-String rh_backend`;修;本地验证
  (`cargo test -p agenterm-rh` + 根级 rh 测试 + `cargo clippy --all-targets --all-features -- -D warnings`
  + `cargo fmt --all` + prd-alignment AOT 金丝雀);`git commit --only`;push;`gh run watch`。
- 转译器发射变更必须 bump RH_CODEGEN_REVISION(现 87;四处钉:host_api.rs、public_contract.rs、
  codegen_native_pack_fixtures.rs、tests/rh_codegen_fixtures.rs)。
- `rh compile error: rh_fail: X` = AOT 包**运行期** rh_fail,不是编译错。
- 本机 lua 构建前先 `Remove-Item env:NoDefaultCurrentDirectoryInExePath`。
- gate 失败 payload:stdout/stderr 各自尾部 8K(别再合并截断)。

## 已修根因清单(防重复踩)

详见 git log 77b508d8..e7b8e774 的十个 `fix(ci)/fix(rh)/fix(tests)` 提交信息,每条都写了根因。
最大教训:解释器宽容 vs AOT fail-closed 的语义分歧是系统性的,每揭一层掩蔽就暴露一族;
以及大量"陈旧钉"测试(pin 老版本号/老文件名/老合同)在 windows/linux 门首次真正跑通时集中爆发。
