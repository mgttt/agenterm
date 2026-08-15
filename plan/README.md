# plan/ — 执行投影索引

**产品真理**在 `PRD.md` / `prd/`；**结构 SSOT** 在 [`ARCHITECTURE.md`](ARCHITECTURE.md)。  
本目录只放**执行投影**（排序、风险、交接、证据）。过期叙事进 [`archive/`](archive/)。

## 现行（agent 默认先读这些）

| 文件 | 角色 |
|------|------|
| [`plan-v0.1.16.md`](plan-v0.1.16.md) | **当前代码线/发布链修复**；是否发布仍服从 exact-SHA 授权 |
| [`plan-v0.1.18.md`](plan-v0.1.18.md) | **在制唯一版本计划**，五条轨：A App Substrate、B v0.1.17 承接树、C `agenterm-con`、D `agenterm-cu`、E `libagenterm` 机制库 |
| [`plan-v0.1.19.md`](plan-v0.1.19.md) | **预开草案**：CC Phase 1 + cu `window-place` / `cu hotkeys`（PRD 32；macOS 已部分落地） |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | 代码分层、热文件与结构禁令 SSOT |
| [`refactor-chassis-l1-l2-l3.md`](refactor-chassis-l1-l2-l3.md) | **薄 L1 底盘 / 可换 L2 宿主 ABI / L3 应用包** 执行树（chassis，避免和终端 shell 撞名）；L1 面 [`chassis-l1-surface.json`](chassis-l1-surface.json)；goal [`goal-chassis-l1-l2-l3.md`](goal-chassis-l1-l2-l3.md) |
| [`plan-rh-3.md`](plan-rh-3.md) | Rh 当前执行与证据轨；已归档 namespace/trace 文档不得替代它 |
| [`plan-unix-gui-win-parity.md`](plan-unix-gui-win-parity.md) | Win↔Unix 可见行为差距地图 |
| [`platform-ux-parity-evidence-matrix.md`](platform-ux-parity-evidence-matrix.md) | 平台 UX 证据矩阵（含 templates） |
| [`precision-audit.md`](precision-audit.md) | 窄域正确性审计与仍开放项 |
| [`agent-human-parity-audit.md`](agent-human-parity-audit.md) | Agent↔Human 输入/观察 parity 的现行审计与剩余叶 |
| [`goal-crate-platform.md`](goal-crate-platform.md) / [`plan-platform-encapsulation-gap.md`](plan-platform-encapsulation-gap.md) | platform crate 边界、机制漏点与固定执行句式 |
| [`goal-agenterm-osx.md`](goal-agenterm-osx.md) | macOS 原生证据与安装尾账 goal |
| [`plan-multiplatform-gui.md`](plan-multiplatform-gui.md) | Linux/macOS GUI 交付里程碑 |
| [`plan-mobile.md`](plan-mobile.md) | 移动原生壳执行投影；**产品归 PRD 33**（PWA 先行，商店 App 占位） |
| [`plan-control-center-ux.md`](plan-control-center-ux.md) / [`design-control-center-ux.md`](design-control-center-ux.md) | Control Center 任务书与实现级设计 |
| [`plan-cc-automation-cli.md`](plan-cc-automation-cli.md) | CC 自动化 CLI 未实现设计 |
| [`design-rh-aot.md`](design-rh-aot.md) | Rh Build/CI AOT 轨；不是 product App Engine |
| [`design-dynacore-logic-pack.md`](design-dynacore-logic-pack.md) | `agenterm-dynacore` 当前产品方向 |
| [`design-dynacore-emulated-guest-core.md`](design-dynacore-emulated-guest-core.md) | emulated guest core 待实现设计 |
| [`reference-cross-target-execution.md`](reference-cross-target-execution.md) | 常驻跨目标执行参考；已完成实验规格在 archive |

### 过渡保留（不是默认派工入口）

- [`plan-v0.1.15.md`](plan-v0.1.15.md)：上版证据/推迟表；待 v0.1.16 最终发布审计确认全部叶去向后单独归档。
- [`ci-green-handoff.md`](ci-green-handoff.md) 与 [`claude-analyze-ci.md`](claude-analyze-ci.md)：当前 v0.1.16 CI 战役输入；只能作为带时点的观察材料，战役收口后同批归档。
- `research/dynamic-core/` 是已封闭研究的结果 SSOT；对应 Q0–Q15 实验规格现位于 `archive/`。

## 已归档（勿当任务单）

见 [`archive/README.md`](archive/README.md)。含：

- 已发版 / 已终止版本 plan：`plan-v0.1.8` … `plan-v0.1.14`、`goal-v0.1.14`
- 未开工即归档：`plan-v0.1.17`（2026-08-12；未完成叶已 upsert 至 `plan-v0.1.18` §11）
- 已合并即归档：`plan-libagenterm`（2026-08-12；全文并入 `plan-v0.1.18` §14 轨 E）
- 已落地专题：`plan-agenterm-server-mode`、`plan-skins-v1`、`plan-platform-facade-v4`、`osx-cpu-improve`
- 已完成 goal 快照：`goal-v0.1.15-server-instance-s-prime`
- 历史过程文：`platform-ui-ux-boundary-tree`（superseded by ARCHITECTURE）
- App Pack 历史讨论：`agenterm-rhai-app`、`plan-agenterm-app-pack`（现行版本投影为 `plan-v0.1.18`）
- dynamic-core Q0–Q15 已判决实验规格（综合结论在 `research/dynamic-core/SYNTHESIS.md`）
- Rh namespace/trace 历史三件套与已落地的 Script subcommand 设计
- 已被现行 parity/CC 文档吸收的旧 goal 与 feeder analysis

## 归档规则（短）

1. 版本已发或专题 **shipped** → 移入 `archive/` + 文件头 ⚠️ 横幅。  
2. 未完成叶先 **upsert** 到在制 `plan-v0.1.*.md`，再归档。  
3. **从不删除**；PRD 链到 archive 路径保留历史证据。  
4. `plan/` 根目录保持「打开就能干活」，禁止堆完工叙事。
