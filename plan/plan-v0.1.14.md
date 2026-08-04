# AgenTerm v0.1.14 公开计划

状态：**开工**（2026-08-04 由占位稿升级；升级时修正了占位稿中已过时的
v0.1.13 Wave B 快照——该三项在 `plan/plan-v0.1.13.md` §10.2 已全部 `[x]`）。
本文件**不**改变 v0.1.12/v0.1.13 的授权状态，**不**创建 tag/Candidate/Release。
v0.1.13 发布链（Candidate → Promotion）独立推进，不受本 plan 影响。

主题：**身份正确性 + 信任尾账**。不开大功能波次；巨型状态机拆解、
snapshot 填充管线统一、net/WebView/大 CC 仍归 v0.2.0（plan-v0.1.13 §10.3）。
结构 SSOT 仍是 `plan/ARCHITECTURE.md`。

---

## 一、目标树

```text
v0.1.14  Identity correctness & trust tail
│
├─ A. server instance 身份贯通（用户实测缺陷，2026-08-04 报告）
│  ├─ [x] autostart 跨进程丢失 logical instance 修复
│  │     症状：`agenterm.exe --instance custom:work` 后 `server-list`
│  │     INSTANCE 列显示 `wjc2022_main`（scope pipe / workspace 均正确，
│  │     仅注册身份错）
│  │     根因：frontend_server 自启动只传 `--endpoint pipe:…`；server 端
│  │     resolver 中 CLI selector 整组压制环境变量（设计如此），且
│  │     endpoint/address 权威下 instance 硬编码回落 "main"（scope 哈希
│  │     单向不可反推）→ server 以 Main 身份注册
│  │     修复：`frontend_server_spawn_parameter()`（frontend_server.rs）——
│  │     endpoint 恰为按 scope 派生的默认 native endpoint 时改传
│  │     `--instance <canonical>`，子进程按同一 scope 重新派生同一
│  │     endpoint（无损）；显式 `--endpoint` / legacy `--address` 权威
│  │     保持原语义（身份为 main 是该权威的设计边界）
│  │     证据：frontend_server 4 单测绿（custom:work → --instance、
│  │     explicit endpoint / legacy address 权威保持）；lib 605 全绿；
│  │     clippy -D warnings 零告警（2026-08-04 本机亲测）
│  ├─ [ ] 真机回归：`--instance custom:work` → `server-list` INSTANCE 显示
│  │     `<user>_work`（等含本修复的二进制；display label 已去 "custom:"
│  │     前缀，见 6e6dcca + 0129a9b 测试对齐）
│  └─ [x] 复核其余 autostart/respawn 路径无同类身份丢失（2026-08-04）：
│        全部 CLI/GUI 自启动汇聚单点 start_frontend_server_process
│        （client/mod.rs::start_server_process 仅转发）；kill-server 走
│        已解析 endpoint 的 IPC、server-list/list-instances 读注册记录，
│        注册身份修复后自动正确；CC 迁移路径用 resolved.logical_instance
│        （control_center.rs:1338）。残留：旧二进制所起 server 的记录
│        仍标 main，server 重启后自愈，非代码缺陷
│
├─ B. precision-audit 决策项收口（继承占位稿 §三，机制已明、待拍板）
│  ├─ [ ] item 22：script_protocol/agenterm-rhai 三个 dedup HashSet 在
│  │     persistent worker 中只增不减；需人工拍板上限/淘汰策略后落地，
│  │     回填 plan/precision-audit.md
│  └─ [ ] item 16 剩余：Linux/macOS 无 HOME/XDG 时 instances 目录静默退化
│        共享 /tmp，未做符号链接/祖先加固；决定是否复用
│        protect_private_directory / metadata_is_real_directory
│
├─ C. v0.1.13 发布期遗留（非回归，独立产品叶）
│  ├─ [ ] CC 480px 高窗口 tab 条折叠：三行 tab 条仅首行在 client 界内，
│  │     Windows client 更矮整条出界（plan-v0.1.13 §10.2 已归因；产品层
│  │     把 strip 提前于详情行或自适应行数）
│  ├─ [ ] control-center-smoke 进 CI 矩阵评估（当前不在矩阵，同源缺口无门禁）
│  └─ [ ] 0.1.12 stale 注册记录体验：server-list 长期显示 stale 行，
│        评估 server-cleanup 自动化或提示
│
└─ D. CI/发布纪律（发布链复盘产物）
   ├─ [x] ci.yml workflow_dispatch 手动重跑通道（bcb7ec0，已落地；
   │     解决「exact-SHA 绿 CI 被取消/删除后 push 无法重触发」死角）
   ├─ [ ] 发布 runbook 固化：把 plan-v0.1.13 §10.2.1 坑清单提炼为
   │     prd/PRD_02_17 或 docs 稳定条目（短 SHA、HEAD==source_sha、
   │     exact-SHA 绿 CI、并发 agent 推 main 的窗口风险）
   ├─ [ ] 多文件/新文件改动前置 cargo fmt --check 清单化
   │     （占位稿 §二 记录的两次 rustfmt fail-closed 教训）
   └─ [ ] flaky 复核：script_process::child_wait_timeout_reaps_descendants
         在高负载 CI runner 偶发（run 30906435620，2026-08-04：owned
         descendants survived cleanup or could not be observed；同 SHA 重跑
         即绿）。方向：收割等待窗口 vs 观察竞态；归 precision-audit 风格叶
```

## 一.5、发布推进（2026-08-04 晚，用户授权：停 v0.1.13 改发 v0.1.14）

```text
v0.1.14 Release 推进
├─ [x] v0.1.13 发布终止归档（plan-v0.1.13 §10.2.1 终局节）
├─ [x] remote-ui-smoke 整体加固（对症 CI 迭代烧钱根因）：
│      所有 --timeout-ms 等待统一 30s 上限（33 处）；纯轮询循环
│      200/240×25ms → 1200×25ms（30s）；wait_for_lease 240/300 → 1200；
│      new-dialog modal 配置轮询 80 → 400；等待均为条件满足即返回，
│      健康路径零成本。check.rhai smoke 外层墙钟预算 120s/60s →
│      600s/300s（防内部等待放宽后撞外层预算）。两脚本 agenterm-rhai
│      check 解析 OK
├─ [x] ci.yml platform-contract 4 job 补 cargo-home 缓存（restore+save，
│      沿用既有 key 模式）
├─ [x] 身份冻结 0.1.14：Cargo.toml ×2 / Cargo.lock / agenterm.tasks.json
│      （version + rc revision）
├─ [x] main CI 全绿 → Candidate（40 位全量 SHA，dispatch 前确认
│      HEAD 未被并发推前）：CI run 30941772992 绿 @ 8ff2b5a；
│      Candidate run 30942173420 六平台 + aggregate 全绿
└─ [x] Candidate 全绿 → Promotion：run 30944087372 成功
       gh workflow run release.yml --repo mgttt/agenterm --ref main
         -f candidate_run_id=30942173420 -f confirmation=publish-v0.1.14
```

### 发布结果（2026-08-05 03:37 +0800）

`v0.1.14` 已发布（非 draft），tag = `8ff2b5a`（== Candidate source_sha），
23 个资产：六平台包 + 各自 `.sha256`/`.provenance.json` + SBOM +
两份 macOS preview README + qualification receipt。

### 本轮修掉的发布链缺陷（八个，均为首次真正跑通该链路才暴露）

交接文档称「收据之后已无未验证环节」并不成立：`release.yml` 本次是
**该仓库有史以来第一次运行**，promotion 车道整段从未被执行过。

| # | 缺陷 | 提交 |
|---|------|------|
| 1 | workbench-smoke 宽度扫描读 render 竞态（`ErrorDotExpr` on `()`） | `b098110` + 并发 agent `ae3f748` 加固 |
| 2 | `agenterm-platform` 读 `AGENTERM_IME_DEBUG`，违反产品中立边界，**自 f42fdab 起 main CI 一直红** | `538ec73` |
| 3 | SPDX id 由含绝对路径的 `pkg.id` 派生 → SBOM 跨 runner 不可复现 | 并发 agent `bffb7b8`（与我 `2aef42d` 同解，我弃用重复提交） |
| 4 | verify 步骤写的 `candidate-run-identity.json` 被随后的 checkout 删除 | `4e6ef06`（我先前 `e15ce6e` 的 `clean: false` 判断有误，已纠正） |
| 5 | `promotion-identity.rhai` 断言从不存在的 `manifest.kind` | `80096e4` + 并发 agent `8ff2b5a` 从 sealer 侧补写 |
| 6 | `tests/promotion_identity.rs` fixture 编码了生产从未实现的 schema，导致 #5 长期不可见 | `ac068ff` |
| 7 | 创建 tag 后立即回读，撞 ref API 最终一致性（404） | `ab1b09e` |
| 8 | 创建 draft 后立即回读，撞 releases 列表最终一致性 | `56a2e17` |

> 注：#1/#3/#5 与并发 agent 同时定位，取先落地的一方，避免无谓冲突。

### 发布经验总结（写给下一个接手发布的 agent）

#### 一、耗时结构：贵的是 Candidate，不是 Promotion

实测（2026-08-04 夜，共 6 次 Candidate + 7 次 Promotion）：

| 阶段 | 失败耗时 | 成功耗时 | 说明 |
|------|---------|---------|------|
| Candidate preflight 失败 | **15–20 秒** | — | 极便宜，随便撞 |
| Candidate 完整跑 | 15–32 分钟 | ~17 分钟 | windows job 是唯一长杆 |
| Promotion 失败 | **13–36 秒** | — | 几乎零成本 |
| Promotion 成功 | — | ~60 秒 | 不重新构建 |

**推论**：Promotion 失败几乎不花钱，Candidate 失败很贵。所以优化目标是
**「让每次 Candidate 都尽可能成功」**，而不是省 Promotion 次数。

#### 二、最关键的一条：哪些修复需要重跑 Candidate？

本轮踩得最痛的坑。判据是**该文件在哪个 ref 下被执行**：

| 修改对象 | 执行 ref | 是否需要新 Candidate |
|---------|---------|--------------------|
| `.github/workflows/release.yml` | `--ref main` | **不需要**，改完直接重发 Promotion |
| `scripts/rhai/promotion-identity.rhai`<br>`scripts/rhai/candidate-verify.rhai` | checkout `ref: source_sha`<br>（= Candidate 的 SHA） | **必须重跑 Candidate** |
| `scripts/rhai/lib/release_candidate.rhai`<br>及一切 gate/smoke 脚本 | Candidate 自身构建 | **必须重跑 Candidate** |

verify job 用 `ref: ${{ steps.candidate.outputs.source_sha }}` 检出，
是**刻意设计**：promotion 必须用「构建该 Candidate 的那份代码」来验证。
后果是——改了 promotion 脚本却复用旧 Candidate，会**一模一样地再失败一次**，
无论 main 上修得多正确。本轮为此白跑了一次 Promotion（30940776700）。

#### 三、离线预演：把 Promotion 的失败提前到本地

Promotion 每轮只暴露一个断言。与其一次次 20 分钟往返，不如把 sealed bundle
拉到本地整段预演——本轮靠这招一次性预清了 publish job 的全部断言：

```bash
gh run download <candidate_run_id> -R mgttt/agenterm \
  -n "release-candidate-<candidate_run_id>" -D /tmp/prom
# 1) 字节级校验（与 CI 同一脚本，输出应为 VALID CANDIDATE ...）
./target/debug/agenterm-rhai run scripts/rhai/candidate-verify.rhai \
  --project-root . -- . /tmp/prom/agenterm-*-candidate-manifest.json /tmp/prom/payload
# 2) 生成 promotion identity（退出 0 即通过 verify job 的核心断言）
./target/debug/agenterm-rhai run scripts/rhai/promotion-identity.rhai \
  --project-root . -- /tmp/prom/agenterm-*-candidate-manifest.json \
  <candidate_run_id> <source_sha> /tmp/id.json
# 3) 再用 jq/shasum 复算 publish job 的 body_sha256 / marker / mac_channel
```

本地结论与 CI 完全一致（`VALID CANDIDATE` 逐字相同），说明这套预演可信。

#### 四、诊断纪律（本轮验证有效的）

1. **先下 artifact 再动手**：`candidate-quality-timing-<run>` 的
   `first_failure.gate_id` 直接点名失败门；`first_failure: null` 则说明
   gate 全过、问题在 job 的其它步骤（本轮据此定位到 aggregate seal）。
2. **区分「同一步骤」与「同一原因」**：#7/#8 都报在 publish 步骤，但一个是
   tag 回读、一个是 draft 回读；#4 两次都报 `cp: cannot stat`，但
   `clean: false` 无效——必须读日志里的真实机制行
   （`Deleting the contents of '...'`），不能凭默认行为推断。
3. **重试通过 ≠ 瞬态**：release 车道 smoke 自带一次 retry，#1 两次都挂，
   据此判定为真回归而非竞态偶发——这个判据本轮成立。
4. **改脚本前先找它的 fixture**：#6 就是反例。`promotion-identity.rhai`
   有 `tests/promotion_identity.rs`，我改了脚本没看测试，CI 才拦下。
5. **验证要针对缺陷成因**：#3 的复现条件是「不同绝对路径」，只跑两次同路径
   生成会假绿。本轮用 `git clone` 到另一路径再生成，得到逐字节相同才算数。

#### 五、并发 agent 协作（共享 checkout）

本轮 #1/#3/#5 与另一 agent 同时定位。有效做法：

- **提交必须精确 pathspec**，禁 `git add -A/-u`（交接文档已警告，确实必要）。
- push 被拒先 `git log HEAD..origin/main` 看对方改了什么，再决定 rebase 还是
  弃用自己的重复提交。#3 我与对方同解，直接 `reset --hard` 弃用己方提交，
  比强行合并干净。
- 对方的修复可能**比自己的更完整**（#1 对方补了 `find_tab` 抛异常的兜底，
  是我漏掉的），也可能**验证更弱**（#3 对方只跑了两次同路径）。取谁先落地，
  但自己更强的证据要留在提交信息里。
- Candidate dispatch 前 `git fetch` 确认 HEAD 未被推前：本轮 30941774787 就是
  在检查与派发之间被并发 push 挤掉，preflight 15 秒拦下——代价很小，别怕。

#### 六、下一版（v0.1.15）可直接落地的改进

1. **promotion 车道加 dry-run**：现在只能靠真发布来验证 verify+publish，
   建议加 `-f dry_run=true`，跑完 verify 全部断言但不建 tag/release。
   本轮 8 个缺陷里有 4 个可被 dry-run 在几十秒内全部暴露。
2. **fixture 从 sealer 生成**：#6 的根因是手写 fixture 与生产 schema 漂移。
   让 `tests/promotion_identity.rs` 直接调 `build_manifest` 产出 fixture，
   或加一个「fixture 字段集 == sealed manifest 字段集」的断言。
3. **GitHub API 回读统一加重试**：#7/#8 同类。ref / releases 列表都是最终
   一致的，凡「写后立即读」都应轮询而非直读。
4. **SBOM 可复现性纳入 CI**：#3 这类缺陷只在跨 runner 比对时才暴露，
   建议 CI 里从两个不同路径各生成一次并比对 sha256。

## 二、明确暂不纳入（继续挂 v0.2.0，避免范围蔓延）

- 巨型状态机拆解（Unix ~223KB / Windows ~266KB）
- snapshot 填充管线统一（R2）
- Workflows / 大 Control Center / net / WebView 生产化
- M8/M9（可选智能 / LLM 网关）——需先有具体用户场景证据

## 三、完成定义

- A 组全勾选：身份贯通有真机证据；同类路径复核有结论。
- B 组两项：人工拍板后落地并回写 precision-audit；未拍板不落码。
- 每叶独立提交 + clippy -D warnings + lib 全绿亲测；无未说明行为变化。
- 不创建 `v0.1.14` tag；Candidate/Release 仍需独立 exact-SHA 授权链。

## 四、与其它文档的关系

| 文档 | 关系 |
|------|------|
| `plan/ARCHITECTURE.md` | 现行结构 SSOT；本文不重画结构树 |
| `plan/plan-v0.1.13.md` | 上一版权威执行记录（§10.2.1 发布坑清单来源） |
| `prd/PRD_02_18_roadmap.md` M12 | 大重构去向（原 plan-v0.2.0.md 已并入） |
| `plan/precision-audit.md` | 持续审查权威记录；B 组决策后回写该文件 |
| `prd/PRD_02_17_delivery_quality.md` | Candidate/Promotion 合同 |
| `prd/PRD_02_18_roadmap.md` | 里程碑权威；0.1.13/0.1.14 为 M11→M12 间信任收口迭代 |
