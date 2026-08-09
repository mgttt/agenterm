# 归档版本 plan

本目录保存**已完成或已终止**版本的执行记录。它们是历史追溯 material，
**不是执行依据**——每份文件头部都有 ⚠️ 归档横幅指向当前权威处。

归档标准：该版本已发版或已终止，且其中的**要求已提炼到版本无关的权威文档**
或**未完成叶已 upsert 到在制版本 plan**，文件本身只剩叙事价值。
归档 = 移动 + 加横幅，**从不删除**。

| 文件 | 版本状态 | 备注 |
|------|---------|------|
| `plan-v0.1.8.md` | 已发布 | 归档时全仓零引用 |
| `plan-v0.1.9.md` | 已发布 | 里程碑证据仍被 `prd/PRD_02_18_roadmap.md` M6 引用 |
| `plan-v0.1.10.md` | 已发布 | 同上（M7） |
| `plan-v0.1.11.md` | 已发布 | 同上（M10） |
| `plan-v0.1.12.md` | **从未公开发布** | Candidate 被放弃/取代 |
| `plan-v0.1.13.md` | **从未公开发布** | Candidate 被放弃，目标移至 v0.1.14；§10.2.1 坑清单已提炼 |
| `plan-v0.1.14.md` | **已公开发布** | tag `8ff2b5a`；未完成叶 → `plan-v0.1.15.md` §一·五 **L′** |
| `goal-v0.1.14.md` | 交接快照 | 发布 goal 历史；勿再执行 |
| `goal-v0.1.15-server-instance-s-prime.md` | goal 完成 | S′ 形态已落地；进度见 plan-v0.1.15 §一·五 S′ |
| `plan-agenterm-server-mode.md` | 已实现 | `agenterm server` 同 PE 子命令；契约 PRD_02_02 |
| `plan-skins-v1.md` | 已实现 | 内置四预设（X1）；SkinHub 外置仍 v0.2.x |
| `plan-platform-facade-v4.md` | 已完成 | 2026-08-01；结构 SSOT → ARCHITECTURE |
| `osx-cpu-improve.md` | 已 shipped | P0–P3；再卡顿对照历史 + O 组 |
| `platform-ui-ux-boundary-tree.md` | superseded | 只叙事；权威 ARCHITECTURE |
| `design-agenterm-cli-merge.md` | 已 shipped | `agenterm cli` 同 PE 转发（AttachConsole + DuplicateHandle）落地 + 真机验证记录；权威 PRD_02_02 + plan-v0.1.16 §CLI |
| `design-agenterm-bin-separation.md` | 结论被推翻 | 同日"不能合并"分析；被落地实现证伪，保留论证过程 |

> 公开发布序列：v0.1.6 → v0.1.10 → v0.1.11 → **v0.1.14**。  
> v0.1.12 与 v0.1.13 有完整 plan 但无 tag、无 GitHub Release  
> （`git ls-remote` 已证实）。读旧 plan 时勿把它们当作已发布版本。

## 当前权威处

| 内容 | 文档 |
|------|------|
| **plan/ 现行索引** | [`plan/README.md`](../README.md) |
| 在制版本工作树 | `plan/plan-v0.1.16.md` |
| 上一已发布版本复盘（历史） | `plan/archive/plan-v0.1.14.md` |
| 发布链要求（版本无关） | `prd/PRD_02_17_delivery_quality.md` §Release-chain operating requirements |
| 结构 SSOT | `plan/ARCHITECTURE.md` |
| 版本归属 / 里程碑门 | `prd/PRD_02_18_roadmap.md` |
