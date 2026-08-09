# plan/ — 执行投影索引

**产品真理**在 `PRD.md` / `prd/`；**结构 SSOT** 在 [`ARCHITECTURE.md`](ARCHITECTURE.md)。  
本目录只放**执行投影**（排序、风险、交接、证据）。过期叙事进 [`archive/`](archive/)。

## 现行（agent 默认只读这些）

| 文件 | 角色 |
|------|------|
| [`plan-v0.1.16.md`](plan-v0.1.16.md) | **在制版本**工作树（多 GUI 产品化 + Unix 多实例 + 0.1.15 尾账） |
| [`plan-v0.1.15.md`](plan-v0.1.15.md) | 上版证据与推迟表全文（must-ship 主波已合 main；未公开发版） |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | 代码分层 / 热文件 / 结构禁令 |
| [`plan-unix-gui-win-parity.md`](plan-unix-gui-win-parity.md) | Win↔Unix **可见行为**差距地图 |
| [`platform-ux-parity-evidence-matrix.md`](platform-ux-parity-evidence-matrix.md) | 平台 UX 证据矩阵（+ template） |
| [`precision-audit.md`](precision-audit.md) | 窄域正确性审计表（L2/L3 等） |
| [`agent-human-parity-audit.md`](agent-human-parity-audit.md) | **Agent↔Human 交互面**对齐审计（键鼠/声音/截图/结构树）+ 待办与决策项 |
| [`plan-control-center-ux.md`](plan-control-center-ux.md) | L-CC / v0.2.0 UX 任务书 |
| [`design-control-center-ux.md`](design-control-center-ux.md) | CC 布局/IA 设计 SSOT（实现级） |
| [`plan-cc-automation-cli.md`](plan-cc-automation-cli.md) | CC 自动化 CLI 设计稿（未实现） |
| [`plan-multiplatform-gui.md`](plan-multiplatform-gui.md) | Linux/macOS GUI 交付里程碑 |
| [`plan-mobile.md`](plan-mobile.md) | 移动端占位（未授权开工） |
| [`goal-crate-platform.md`](goal-crate-platform.md) | **/goal** 可执行：platform crate 跨平台封装收口（边界 SSOT + 机制 gap + catalog 闸） |
| [`goal-agenterm-osx.md`](goal-agenterm-osx.md) | **/goal** OSX 泳道：O-fix / O1b / con blackbox / 抽象小切片（落盘须脱敏，见根 `Agents.md` Document redaction） |
| [`plan-platform-encapsulation-gap.md`](plan-platform-encapsulation-gap.md) | 机制封装漏点表 + 跨平台任务执行句式（G1 breakaway 已收） |
| [`design-rh-aot.md`](design-rh-aot.md) | **rh 并行 AOT 轨**（check / transpile / backend 切换） |
| [`design-dynamic-core-experiment.md`](design-dynamic-core-experiment.md) | **研究轨**（非产品范围）：动态核 1 层 vs 2 层的判决性实验 → **已判决：2 层**；实现在 `research/dynamic-core/` |
| [`design-neutral-ir-experiment.md`](design-neutral-ir-experiment.md) | **研究轨**：中立 IR 能否把 ABI/布局推迟给降级（同 ISA 双 ABI 隔离）；承接上条 §7 |
| [`design-os-interface-as-data-experiment.md`](design-os-interface-as-data-experiment.md) | **研究轨**：Q7 —— OS 接口内容能否从每目标手写代码变成数据表（固定编组器）；承接 Q1 泄漏 L1–L5 → **已判决：有边界可达**；实现在 `research/dynamic-core/tables/` |
| [`reference-cross-target-execution.md`](reference-cross-target-execution.md) | **研究轨参考**（常驻，非任务单）：跨目标执行技术空间综述 —— 中立 IR 失败史与根因、二进制翻译、验证型字节码、OS 轴、装载机制；含「与动态核架构的对照」 |
| [`design-dynacore-logic-pack.md`](design-dynacore-logic-pack.md) | **产品设计（研究轨已结束，本条不是研究）**：dynamic-core 研究收成能力包机制，兑现 `PRD_02_10` 的 Layered deployment；v1 只调 `fleet.*`，不做 codegen/跨 ISA/任意原生 OS 调用 |

## 已归档（勿当任务单）

见 [`archive/README.md`](archive/README.md)。含：

- 已发版 / 已终止版本 plan：`plan-v0.1.8` … `plan-v0.1.14`、`goal-v0.1.14`
- 已落地专题：`plan-agenterm-server-mode`、`plan-skins-v1`、`plan-platform-facade-v4`、`osx-cpu-improve`
- 已完成 goal 快照：`goal-v0.1.15-server-instance-s-prime`
- 历史过程文：`platform-ui-ux-boundary-tree`（superseded by ARCHITECTURE）

## 归档规则（短）

1. 版本已发或专题 **shipped** → 移入 `archive/` + 文件头 ⚠️ 横幅。  
2. 未完成叶先 **upsert** 到在制 `plan-v0.1.*.md`，再归档。  
3. **从不删除**；PRD 链到 archive 路径保留历史证据。  
4. `plan/` 根目录保持「打开就能干活」，禁止堆完工叙事。
