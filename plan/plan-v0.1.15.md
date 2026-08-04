# AgenTerm v0.1.15 公开计划（占位稿 / 思维工作树）

状态：**占位草案**（2026-08-04 起草，基于 v0.1.14 发布日全天真实遥测）。
不改变任何已发布/在途版本的授权状态；不创建 tag/Candidate/Release。
主题预定：**反馈左移 + 发布链降本**——把「问题在离引入点最远、最贵的
车道才暴露」这一根因打掉。开工前需人工确认范围与 §三 的政策决策项。

数据来源：v0.1.14 发布日 ~10 轮 gate 级迭代的 timing 遥测
（candidate-quality-timing artifacts + job/step API 计时），关键事实：

```text
单轮全绿路径 ≈ 30min：CI ~5min → Candidate ~15-18min → Promotion ~5min
Candidate 唯一长杆 = windows 门（13-16min）：
  release 双构建 3.8-5.3min ＋ net-research 2.8min ＋ clippy/单测/mcp ~3min
  ＋ 14 个 GUI smoke 仅 ~90s ＋ 杂项 ~1min
失败构成（10 轮）：6 次确定性测试腐化（从未在 CI 车道执行过的断言）
  ＋ 4 次共享 runner 负载竞态 —— 单轮速度不是主要矛盾，反馈延迟才是
v0.1.14 已落地的止血：失败也保存构建缓存（always()）；remote-ui/fleet
  smoke 左移进 push CI；release 车道 smoke retry-once；wake pump 余量
```

---

## 一、目标树（占位，未定版）

```text
v0.1.15  Feedback shift-left & release-lane economics
│
├─ A. 反馈左移（低风险四件套，最高性价比）
│  ├─ [ ] A1 夜间定时 win-full-gate（release-stress）
│  │     动机：断言腐化攒到发布日集中爆雷 = v0.1.14 发布日 5/6 小时的
│  │     直接根因；夜间彩排让腐化 24h 内暴露
│  │     形态：schedule cron 触发现有 workflow_dispatch 入口；失败通知面
│  │     待定（issue / observer）；成本每晚 ~1 runner-hour
│  ├─ [ ] A2 Candidate 自动触发：main CI 绿后经 workflow_run 自动派
│  │     （开关形态待定：commit 标记 / repo variable / 手动兜底保留）
│  │     动机：省派发往返延迟 + 收窄「HEAD 被并发推前」竞态窗口
│  │     注意：不改变 preflight 语义与授权链，只自动化 dispatch 这一步
│  ├─ [ ] A3 script-smoke 左移进 push CI（debug 版，实测 ~7s）
│  │     动机：v0.1.14 发布日它贡献 2 次腐化（operation 计数 22→24、
│  │     sidebar 投影竞态），左移后 6 分钟内暴露
│  └─ [ ] A4 per-gate timing 表写进 GITHUB_STEP_SUMMARY
│        动机：现在要下载 artifact 才能看每门耗时；诊断路径应一眼可见
│
├─ B. Candidate 门瘦身（每轮直接省时）
│  ├─ [ ] B1 agenterm-net-research 移出 release 门（→ CI 或夜间车道）
│  │     实测每轮 2.8min；research 隔离验证不属于产品资格证明
│  │     涉及 qualification-gates.json（fail-closed 声明）+ 政策复核
│  ├─ [ ] B2 缓存 key 对 Cargo.toml 版本行归一化后再 hash
│  │     动机：版本冻结提交使 hashFiles 全变 → 每版本首轮全量重编
│  │     （~10min/版本）；归一化后冻结提交命中上一版缓存
│  │     成本：hashFiles 换脚本算 key，六 workflow 一致性维护
│  └─ [ ] B3 artifact-build 与 artifact-build-fast 产物复用审计
│        两者合计 3.8-5.3min；若 fast 车道可复用主构建产物可省 1-2min
│        （先审依赖关系再动，可能结论是「保持分离」）
│
├─ C. 竞态类问题的结构性收口（v0.1.14 遗留）
│  ├─ [ ] C1 flaky 复核：script_process::child_wait_timeout_reaps_descendants
│  │     30s ceiling 已止血（456a7f7）；根因（收割窗口 vs 观察竞态）待查
│  ├─ [ ] C2 bracketed-paste GUI 复制体滞后：smoke 已用 wait_observed 闭合
│  │     （9f3c480）；评估产品侧是否该在 ui-snapshot 暴露 GUI 视图的
│  │     bracketed 状态（Win/Unix schema 平权），让测试不再依赖间接信号
│  ├─ [ ] C3 stream pump 上限 64 的容量审计：wake-smoke 已留余量（24×2）；
│  │     评估运行时上限是否该随并发场景参数化或计入 back-pressure
│  └─ [ ] C4 quality-timing 嵌套 check 偶发（win-full-gate 30907369093，
│        NotFound）：复现窗口在满载 runner 嵌套 check；先观察夜间彩排
│        （A1）的复发率再决定投入
│
└─ D. 政策决策项（需人工拍板，agent 不自主执行）
   ├─ [ ] D1 Candidate preflight 从「SHA == main HEAD」放宽为
   │     「main 祖先 + 该 SHA 有绿 CI」
   │     动机：HEAD 竞态在 v0.1.14 发布日实咬两次（c46eb70 无法重封印、
   │     发布期并发 push 风险）；放宽后仍是 exact-SHA 封印，完整性不降
   │     反方：釘 HEAD 保证「发布的就是最新」；放宽后可能发布落后于
   │     main 的 SHA —— 需要明确这是否可接受
   ├─ [ ] D2 smoke 并行分片（14 个拆 2-4 runner）
   │     现值低（smoke 全绿仅 90s）；仅当 smoke 数量/时长显著增长再议
   └─ [ ] D3 发布窗口纪律 vs 工具化：发布期并发 agent 推 main 的协调
         （若 D1 通过则大幅弱化此需求）
```

## 二、排序建议（起稿人观点）

1. **A1 + A3 + A4**：一晚可落地，直接消灭 v0.1.14 发布日最大痛苦源。
2. **A2**：随后落地，发布全链自动化闭环（人只拍 Promotion 前的最终板，
   或连 Promotion 也自动 —— 后者是政策问题，归 D 组讨论）。
3. **B1**：独立叶，收益确定（每轮 -2.8min）。
4. **B2**：版本发布日专项收益；实现前先在分支验证 key 稳定性。
5. C 组按复发率排优先级；D 组等人工。

## 三、明确非目标

- 不动 Candidate/Promotion 的授权语义（D1 除外，且 D1 只在人工批准后做）。
- 不为提速削弱资格覆盖：任何门的移除/降级都要有「该验证去了哪里」的答案
  （如 B1 的 net-research 移去 CI/夜间，而不是删除）。
- 不做投机性并行化（D2 现值低）。

## 四、与其它文档的关系

| 文档 | 关系 |
|------|------|
| `plan/plan-v0.1.14.md` | 上一版执行记录；本文数据与止血项的出处 |
| `plan/plan-v0.1.13.md` §10.2.1 | 发布链坑清单（runbook 素材，D 组配套） |
| `plan/ARCHITECTURE.md` | 结构 SSOT |
| `prd/PRD_02_17_delivery_quality.md` | Candidate/Promotion 合同；D1 若通过需回写 |
| `plan/precision-audit.md` | C 组竞态根因复核的记录处 |
