# AgenTerm v0.1.17 公开计划

状态：**已定稿，待授权开工**（2026-08-10）  
不创建 tag / Candidate / Release，除非人工明确授权。  
版本列车停在 **0.1.16 代码线**；本文件是 **下一列车执行投影**，不替代 PRD。

**主题：发布链证据 + 安装尾 + 脚本引擎深化 + 低成本卫生。**

比 v0.1.16 更窄：本版只收被 v0.1.16 明确推迟的尾账与深化项，不引入新的
产品面大叶。v0.1.16 已完成的大规模并行工作（agenterm-con 产品化、
QuickJS 引擎、跨引擎共享层、SQL 后端）不再重复列出，仅记录其留下的未收
缺口。

> 上版工作树与证据：[`plan-v0.1.16.md`](plan-v0.1.16.md)。
> 结构 SSOT：[`ARCHITECTURE.md`](ARCHITECTURE.md)。

---

## 0. 基线事实（2026-08-10）

### 0.1 从 v0.1.16 迁入的已推迟项

v0.1.16 执行中发生了三件计划外的大规模并行工作（agenterm-con 产品化、
QuickJS 引擎 M0→M5d、跨引擎共享层 Common-M1→M7），合计占掉了本版绝大部分
实际工时，导致以下项被明确推迟到 v0.1.17：

| 推迟项 | 原属 v0.1.16 泳道 | 推迟原因 |
|--------|-------------------|----------|
| **R1e/R2e/R4e** | R′ 发布链证据收口 | 需要真实 Candidate 运行观测，v0.1.16 不发布 |
| **T-debt** | R′ 集成红认领 | linux_package / supply_chain 偶发红，需独立调试 |
| **G1** | G′′ 安装尾 | macOS `curl\|bash` happy path，依赖 G-P1 政策 |
| **H2** | G′′ 安装尾 | install.sh 消费 `releases.json`，依赖 H1 稳定 |
| **G7b/c/d** | G′′ 安装尾 | 升级遇 running server 默认策略，依赖 G-P2 |
| **L′ (全组)** | L′ 低成本尾账 | L7/L1/L5/L6/L4/L2/L3 全部，工期紧砍 |
| **U4** | Ux Windows 尾账 | 可选协议优化，明确标「工期紧可砍」 |
| **S4** | Ux Windows 尾账 | 同窗热切换权威边界，标「默认不进 must-ship」 |
| **Rh-M23** | Rh 脚本引擎 | AOT 扩面 + check parity + caller wave 1，独立轨 |
| **QJS-M6** | QJS 脚本引擎 | API 级静态校验（`shipped_surfaces` 对账），新发现缺口 |
| **C10d** | C 控制台宿主 | 回看搜索、OSC 8 超链接、脏行重绘，标「有余力再挑」 |
| **M/N/CC/NET** | 跨版轨 | 多 agent 观察 / platform facade / Control Center / 去中心化网络，均推 v0.2.x |

### 0.2 v0.1.16 留下的已知缺口（非本版引入，仅记录）

- agenterm-con 方向键在真实 shell 里不生效（ConPTY 翻译疑点）
- agenterm-con IME 端到端从未自动化验证
- qjs `check` 无 `shipped_surfaces` 级 API 静态校验（QJS-M6）
- qjs `pack` 字节码 hash 是指纹而非加载依据（与 lua 同因不同由）
- lua `check_many` 的 `--project-root`/`--timeout-ms` 已修复（Common-M7），但 lua 无 fail-closed entry 契约
- rh `shipped_surfaces.rs` 声明的 76 条 fleet.* 中有 32 条在 host `OPERATION_CATALOG` 不存在（stale 声明）
- 测试运行会泄漏 `agenterm.exe server` 孤儿进程锁住构建输出

---

## 1. 收敛工作树（可执行清单）

选择原则（继承 v0.1.14/15/16）：**宁可少而全绿，不要多而半途**。  
叶定义：用户问题 · 不变量 · 可观察证据 · 安全失败 · 黑盒 owner · 非目标。

### R′. 发布链证据收口（从 v0.1.16 迁入）

> 配置已在 v0.1.15 合 main；本版只做观测 + 最小修复。

```text
R′. Evidence closeout
├─ [ ] R1e Candidate bootstrap.worker.state==reused 连续两次 + cache 配额
├─ [ ] R2e cargo-home restore-keys 前缀命中日志
├─ [ ] R4e release dry_run 真跑一次（无 tag/draft）
└─ [ ] T-debt linux_package / supply_chain 集成红认领
```

- [ ] **R1e** — 观测两次连续 Candidate 的 bootstrap 是否 `worker.state=reused`；
  确认 cache 配额不再被 CI 挤掉
- [ ] **R2e** — 确认 `cargo-home-candidate-v2` 的 `restore-keys` 前缀命中生效
- [ ] **R4e** — 真实执行一次 `release.cmd --rehearse`，验证完整 dry-run 契约
  （无 tag、无 draft、无公开副作用）
- [ ] **T-debt** — 认领 `linux_package`（缺 SBOM 类产物）和 `supply_chain`
  （计数 pin）的偶发集成红；与 GUI 叶并行，文件域互斥

**非目标**：不重做 cache 策略设计；不扩 scope 到工作流重构。

### G′′. 安装/更新体验尾账

> 依赖政策拍板后才可执行；文案/文档可提前。

| 叶 | 条件 | 说明 |
|----|------|------|
| **G1** | G-P1 已拍板可回落 unsigned | macOS `curl\|bash` happy path |
| **H2** | H1 稳定一版后 | install.sh 消费 `releases.json` |
| **G7b/c/d** | 等 G-P2 | 升级遇 running server 的默认策略 |

- [ ] **G1** — macOS 无 signed asset 时，install 自动回落 unsigned-preview
  并打印信任模型警告（或 README 首屏固定写必带 env 的一行命令）
- [ ] **H2** — install.sh 从 `releases.json` 选版本而非硬编码
- [ ] **G7b/c/d** — 升级遇 running server 时的默认策略：
  是否提示重启 / 是否允许并存 / 是否自动 keep-server

**非目标**：不改 keep-server 默认行为；不引入 delta 更新。

### L′. 低成本尾账（从 v0.1.16 迁入）

> v0.1.16 原排列序：L7 → L1 → L5 → L6 → L4 → L2/L3。本版默认只认 L7 + L1。

- [ ] **L7** — 仓库卫生（过期文件清理、注释修正、最小 docs 同步）
- [ ] **L1** — PRD capability 状态同步（v0.1.16 实际完成面 vs PRD 声明）
- [ ] **L5** — （原序，待从 v0.1.15 §1.5 L′ 展开）
- [ ] **L6** — （同上）
- [ ] **L4** — （同上）
- [ ] **L2** — （同上）
- [ ] **L3** — （同上）

### Ux. Windows 尾账余量

- [ ] **U4** — TabSelected 不重推整屏 cells（可选协议优化；不阻塞发版叙事）
- [ ] **S4** — 明确「同窗热切换」边界：默认新窗 / As Window，不重做权威；
  仅文档边界，不进 must-ship

### Rh. 脚本引擎深化

- [ ] **Rh-M23** — AOT 扩面 + check parity + caller wave 1 + shim 硬化
  - 细节 SSOT：[`plan-rh-3.md`](plan-rh-3.md) §5
  - Lnx 侧 agent 主责；不阻塞 v0.1.17 其他泳道
  - 非目标：Cranelift JIT、Candidate 六 cell 改名（仍 HOLD）

### QJS. QuickJS 引擎缺口

- [ ] **QJS-M6** — API 级静态校验：qjs 的 `check_with_project_validation` 对齐 rh
  的第②件事（shipped API 引用静态校验，见 `design-qjs-module-imports.md` §7）
  - 需要：(a) 跨引擎可消费的 shipped surfaces 目录；(b) JS 源码 `__host.fleet_call('literal', ...)` 静态扫描器
  - 已知天花板：运行时变量调用不可静态扫出，rh 同
  - 非目标：动态 `import()`、迁移 `fleet.js` 到 export 风格

### C. 控制台宿主余量

- [ ] **C10d** — 未做的超越面（有余力再挑）：回看搜索、OSC 8 超链接、脏行重绘
  - 非目标：server attach（C4，仍明确不纳入）

### M / N / CC / NET（跨版轨）

| 轨 | 本版态度 |
|----|----------|
| **M** 多 agent 观察 | 文档/约定可补；大功能仍推 v0.2.x |
| **N1** platform facade | 可选小叶；不阻塞其他 |
| **L-CC** | 设计稿已有；实现默认 **v0.2.0** |
| **L-NET** | 研究继续，**不进**本版 must-ship |

---

## 2. 排序与泳道

### 2.1 建议执行序

| 序 | 叶 | 理由 |
|----|-----|------|
| 1 | **L7 + L1** | 仓库卫生与 PRD 同步；成本最小、可立即交付 |
| 2 | **T-debt** | 集成红阻断 CI 可信度 |
| 3 | **R1e → R2e → R4e** | 发布链证据；需 Candidate 窗口 |
| 4 | **QJS-M6** | API 校验缺口；独立轨 |
| 5 | **Rh-M23** | Lnx agent 独立轨 |
| 6 | **G1 → H2 → G7b/c/d** | 安装尾；依赖政策拍板 |
| 砍 | U4、S4 实现、C10d、M/N/CC/NET 大叶 | 见 §3 |

### 2.2 泳道

| 泳道 | 主机 | 叶 | 可写 | 禁区 |
|------|------|-----|------|------|
| **CI-R** | 任意独占 | R′ 观测/最小 workflow 修 | workflows / scripts/rh/check.rh | 不扩 scope 到 GUI |
| **Docs** | 任意 | L7/L1 | PRD / plan / README | 不改产品代码 |
| **Win-UX** | Windows | U4/S4（可选） | `remote_frontend*` | 不改 lease/As Window 核心 |
| **Rh** | 任意 | Rh-M23 | `crates/agenterm-rh/**` | 不删 `agenterm-rhai` PE |
| **QJS** | 任意 | QJS-M6 | `crates/agenterm-qjs/**` | 不引入新 unsafe/GC 路径 |
| **Install** | Linux/macOS | G1/H2/G7 | `scripts/install.sh` | 不改 keep-server 默认 |
| **C-fallback** | 任意 | C10d（可选） | `src/bin/agenterm-con.rs` | 不扩成全功能终端 |

### 2.3 并发波形

```text
时间 →
  Docs:     [L7][L1]
  CI-R:     [T-debt][R1e/R2e 观测][R4e]
  QJS:      [.......... QJS-M6 ..........]
  Rh:       [.......... M23a/b → M23c → M23d ..........]
  Win-UX:   [U4/S4 文档/可选]
  Install:  [G1][H2][G7 策略]
  C-fb:     [C10d 可选]
```

---

## 3. 明确非目标

- 公开 **tag / Candidate / Promotion**（除非另文授权）
- 多 GUI 产品面（W1–W4，已在 v0.1.16 scope）
- Unix 多实例可达（O-evidence，已在 v0.1.16 scope）
- agenterm-con 方向键根因修复 + IME 自动化（已知缺口，非本版）
- qjs 真实字节码加载 + 执行（已知取舍，非本版）
- lua fail-closed entry 契约统一（lua agent 主责）
- rh stale `shipped_surfaces` 32 条清理（Lnx agent 主责）
- 夜间彩排 A1、Candidate 自动派发 A2
- gate 大分片、smoke 并行分片
- L-NET 实现、L-CC 大内容、computer-use
- 回退 M22f 默认 rh backend
- 新脚本引擎（SQL 之后的下一个）开工

---

## 4. 决策项（agent 不自主拍板）

| ID | 题 | 阻塞 |
|----|-----|------|
| **G-P1** | macOS unsigned 是否自动回落 + 警告文案 | G1 |
| **G-P2** | 升级遇 running server 默认策略 | G7b/c/d |
| **P1/P5** | agenterm.work / Pages 归属 | H5、E1 |
| **D1** | Candidate preflight 是否可祖先 SHA | 仅工具链 |
| **Rh-M22-go** | Candidate 六 cell 改名（M22f 薄壳已 ship，公开 rename 仍 HOLD） | 公开 rename |
| **S-struct** | 是否开 architecture 围栏重构 | HOLD |

---

## 5. 与其它文档的关系

| 文档 | 关系 |
|------|------|
| [`PRD.md`](../PRD.md) / `prd/*` | 产品真理；本 plan 收敛后同步 capability 状态 |
| [`plan-v0.1.16.md`](plan-v0.1.16.md) | 上版工作树与已完成项全文 |
| [`plan-v0.1.15.md`](plan-v0.1.15.md) | 上上版证据与推迟表全文 |
| [`plan-unix-gui-win-parity.md`](plan-unix-gui-win-parity.md) | Unix 对齐地图 |
| [`plan-rh-3.md`](plan-rh-3.md) | rh 并行轨细节 |
| [`design-qjs-module-imports.md`](design-qjs-module-imports.md) | QJS-M6 设计依据 |
| [`design-scripting-boundary-comparison.md`](design-scripting-boundary-comparison.md) | 脚本引擎 L2 契约 |
| [`design-script-engine-trait.md`](design-script-engine-trait.md) | trait 统一设计 |
| [`ARCHITECTURE.md`](ARCHITECTURE.md) | 热文件 / 分层 |
| [`Agents.md`](../Agents.md) | 并发、观察、开发环 |

---

## 6. 验收总门（本版「做完」定义）

未授权公开发布时，**开发完成** = 下列同时成立：

1. **R4e** dry_run 真跑一次或书面记录「本版不跑 dry_run 的原因」
2. **T-debt** linux_package / supply_chain 红已认领（修复或诚实 skip 记录）
3. **L7 + L1** 仓库卫生 + PRD capability 状态已同步
4. **QJS-M6** 至少完成设计决策（做/不做/做到什么程度）并记录
5. `lint` / `check --quick` 绿

---

## 7. 决策记录

| 日期 | 决定 |
|------|------|
| 2026-08-10 | 开立 **v0.1.17** 工作树：从 v0.1.16 迁入所有已推迟项（R′/G′′/L′/U4/S4/Rh-M23/QJS-M6/C10d/M/N/CC/NET）；主题 = 发布链证据 + 安装尾 + 脚本引擎深化 + 低成本卫生 |
| 2026-08-10 | v0.1.16 保留 W1–W4 + U2 + O-evidence 为 must-ship；本版不重复 |

---

## 8. 开工检查单（每 agent 复制）

1. `git pull --ff-only origin main`
2. 读本节 §1 自己泳道 + §3 非目标
3. 声明 pathspec 热区；冲突让路
4. 小步 commit；PRD 状态变更同步 owning 模块
5. 不扩到 HOLD / §3 非目标

---

*执行投影，非产品宪法。能力状态以 PRD 为准。*
