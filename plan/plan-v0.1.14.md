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
   └─ [ ] 多文件/新文件改动前置 cargo fmt --check 清单化
         （占位稿 §二 记录的两次 rustfmt fail-closed 教训）
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
├─ [ ] main CI 全绿 → Candidate（40 位全量 SHA，dispatch 前确认
│      HEAD 未被并发推前）
└─ [ ] Candidate 全绿 → Promotion：
       gh workflow run release.yml --repo mgttt/agenterm --ref main
         -f candidate_run_id=<id> -f confirmation=publish-v0.1.14
```

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
| `plan/plan-v0.2.0.md` | 大重构去向 |
| `plan/precision-audit.md` | 持续审查权威记录；B 组决策后回写该文件 |
| `prd/PRD_02_17_delivery_quality.md` | Candidate/Promotion 合同 |
| `prd/PRD_02_18_roadmap.md` | 里程碑权威；0.1.13/0.1.14 为 M11→M12 间信任收口迭代 |
