# plan/ — 执行投影索引

**产品真理**在 `PRD.md` / `prd/`；**结构 SSOT** 在 [`ARCHITECTURE.md`](ARCHITECTURE.md)。  
本目录只放**执行投影**（排序、风险、交接、证据）。过期叙事进 [`archive/`](archive/)。

## 现行（agent 默认只读这些）

| 文件 | 角色 |
|------|------|
| [`plan-v0.1.15.md`](plan-v0.1.15.md) | **在制版本**工作树（§一·五 叶 + **§二·二-b 三端泳道派工**；含 L′ 尾账） |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | 代码分层 / 热文件 / 结构禁令 |
| [`plan-unix-gui-win-parity.md`](plan-unix-gui-win-parity.md) | Win↔Unix **可见行为**差距地图 |
| [`platform-ux-parity-evidence-matrix.md`](platform-ux-parity-evidence-matrix.md) | 平台 UX 证据矩阵（+ template） |
| [`precision-audit.md`](precision-audit.md) | 窄域正确性审计表（L2/L3 等） |
| [`plan-control-center-ux.md`](plan-control-center-ux.md) | L-CC / v0.2.0 UX 任务书 |
| [`design-control-center-ux.md`](design-control-center-ux.md) | CC 布局/IA 设计 SSOT（实现级） |
| [`plan-cc-automation-cli.md`](plan-cc-automation-cli.md) | CC 自动化 CLI 设计稿（未实现） |
| [`plan-multiplatform-gui.md`](plan-multiplatform-gui.md) | Linux/macOS GUI 交付里程碑 |
| [`plan-mobile.md`](plan-mobile.md) | 移动端占位（未授权开工） |
| [`goal-crate-platform.md`](goal-crate-platform.md) | **/goal** 可执行：platform crate 跨平台封装收口（边界 SSOT + 机制 gap + catalog 闸） |
| [`plan-platform-encapsulation-gap.md`](plan-platform-encapsulation-gap.md) | 机制封装漏点表 + 跨平台任务执行句式（G1 breakaway 已收） |

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
