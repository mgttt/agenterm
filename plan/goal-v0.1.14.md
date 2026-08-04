# /goal 交接：发布 AgenTerm v0.1.14

> 用途：任何机器上的 agent 接手「发布 v0.1.14」目标时的完整上下文。
> 前提：gh CLI 已认证 mgttt 账号且有 Actions write；repo = mgttt/agenterm。
> 交接时间：2026-08-05 约 00:55 +0800。

## 当前状态

- 发布 SHA 候选 = origin/main HEAD（交接时为 `7960e3a`，含 evidence 幂等修复）。
- 上一 agent 已启动无人值守链（CI → Candidate → Promotion），可能仍在跑。
  **接手第一步永远是查状态，避免重复劳动**：

  ```
  gh release view v0.1.14 --repo mgttt/agenterm     # 已发布则目标达成，只做验证
  gh run list --repo mgttt/agenterm --limit 5        # Candidate in_progress 则等它出结果
  ```

- 关键事实：自 v0.1.11 以来首次，上一轮 **14 个 GUI smoke 已全部绿过**
  （含 retry 兜底首次实战生效），最后死在资格收据 evidence 唯一性断言，
  已修（`7960e3a`：test_harness emit_evidence 幂等）。收据是 windows gate
  的最后一步，**其后已无未验证环节**。

## 发布流程（严格照做）

1. **main CI 绿**：该 SHA 的 ci.yml 必须有 success run。docs-only 提交
   （plan/prd/docs/md）不触发 push CI，可手动补：
   `gh workflow run ci.yml --repo mgttt/agenterm --ref main`
2. **派 Candidate**（source_sha 必须 40 位全量，且 == 派发瞬间的 main HEAD）：
   ```
   gh workflow run candidate.yml --repo mgttt/agenterm --ref main -f source_sha=<40位SHA>
   ```
3. **Candidate 全绿 → Promotion**（不重新构建，~5 分钟）：
   ```
   gh workflow run release.yml --repo mgttt/agenterm --ref main \
     -f candidate_run_id=<candidate run id> -f confirmation=publish-v0.1.14
   ```
4. **验证**：`gh release view v0.1.14`（isDraft=false，资产含六平台包）。
   完成后回写 plan/plan-v0.1.14.md §一.5 勾选 Promotion 项。

## 已知坑（全部踩过，勿重探）

- preflight 要求 `source_sha == main HEAD`：派发前 `git fetch` 确认 HEAD
  未被并发 agent 推前。共享 checkout 上有其它 agent 工作，提交必须精确
  pathspec，**禁 git add -A / add -u 全仓**（曾多次卷入并发暂存内容）。
- Candidate 唯一长杆 = windows job（~10–16 分钟）；其余五平台 3–5 分钟。
- release 车道 smoke 已带自动 retry 一次（check.rhai）；真回归两次都挂，
  瞬态竞态一次即过。
- windows gate 挂了先下载 artifact 定位再动手，不要盲目重跑：
  `release-quality-gate-failure-<run>`（gate 日志）与
  `candidate-quality-timing-<run>`（每门耗时）。
- 失败也保存构建缓存（always()）：重跑轮次编译很快，别为缓存犹豫。
- 本地开发机跑 **release 构建**的 GUI smoke 会假挂起，勿当证据；
  dev/debug 构建的本地 smoke 结果可信（`target/debug/agenterm-rhai.exe
  task run <id>-smoke --manifest agenterm.tasks.json`，记得 AGENTERM_NO_ACTIVATE=1）。
- Windows CI runner 与本机差异的四个来源（环境变量/负载时序/PID 复用/
  release profile）——本地绿不等于 CI 绿，反之 CI 的失败 artifact 是真相。

## 参考文档

| 文档 | 内容 |
|------|------|
| plan/plan-v0.1.13.md §10.2.1 | 发布链原始坑清单 |
| plan/plan-v0.1.14.md §一.5 | 本版发布推进记录（完成后回写处） |
| plan/plan-v0.1.15.md | 提速路线（夜间彩排/自动派发/门瘦身） |

## 完成定义

`gh release view v0.1.14` 显示已发布（非 draft），六平台资产齐全；
plan/plan-v0.1.14.md §一.5 已回写。
