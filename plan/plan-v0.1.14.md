# AgenTerm v0.1.14 公开计划（草稿 / 占位）

状态：**草稿、占位**（2026-08-04 起草，基于 v0.1.13 现状 + CI 观测 + PRD 复核）。
本文件**不**改变 v0.1.12/v0.1.13 的授权状态，**不**创建 tag/Candidate/Release，
**不**代表 v0.1.14 的正式立项——PRD 路线图（`prd/PRD_02_18_roadmap.md`）目前
在 M11/v0.1.12 之后直接跳到 M12/v0.2.0，v0.1.13/v0.1.14 都是 v0.1.12 与
v0.2.0 之间的信任/质量收口迭代，尚未获得独立里程碑编号。真正开工前需要
人工确认范围与优先级；本文只是把「现在能看到的东西」先摆出来，方便下一轮
规划直接在这棵树上增删，而不是从空白开始。

---

## 一、v0.1.13 现状复核（写这份稿子时的快照）

依据：`plan/plan-v0.1.13.md` §10（2026-08-03 定版的执行规划）。

```text
v0.1.13 Wave A（功能补齐）—— 已全部完成
├─ [x] REPL 行编辑/history（console-line-editor feature，2026-08-04 落地）
├─ [x] Cockpit 诊断小步（每 tab 只读事实行，2026-08-04 落地）
└─ [x] precision-audit #13：Rhai catalog-vs-registration 自动化守卫
      （2026-08-03 由另一位 agent 完成，rhai metadata 仅 dev-dependency）

v0.1.13 Wave B（信任面收口）—— 仍是缺口，v0.1.14 的第一候选来源
├─ [ ] macOS pointer Unsupported 诊断清晰化（说清「缺哪项能力」，
│      不是新增正向指针支持）
├─ [ ] B 组 [~] facade 纯转发删除（contract/ipc.rs 里 2 处
│      #[allow(dead_code)] 纯转发，需要跨 target CI 才能安全删除，
│      本机 Windows 无法独立验证 Unix adapter）
└─ [ ] 六平台 parity-smoke 宿主矩阵门禁记录（Windows 已绿；
       Linux/macOS 归属 CI，本机无法亲测）
```

**结论**：v0.1.13 的「功能」叶子都收了，但「信任面收口」的 Wave B 三项
还没有一项真正落地——这些不需要等 v0.1.14 立项，理论上现在就能推进，
只是受限于「本机是 Windows、跨平台验证依赖 CI」这个客观条件。

---

## 二、CI 现状观测（2026-08-04，单次有界检查，非持续轮询）

通过 `gh run list --branch main` 做了一次有界检查（未展开去追更早的历史）：

```text
最近 main 上的 CI 记录
├─ [失败] feat(repl): per-key line editor with history on Windows and Unix
│         → linux-x86_64 + windows 两个 job 都失败
├─ [失败] feat(cockpit): project per-tab read-only facts into the diagnostic panel
│         → 同样 linux-x86_64 + windows 两个 job 失败
└─ [进行中] style: apply rustfmt to files the quality gate rejected
          → 已经是在修上面两次失败的后续提交
```

**根因（已用 `gh run view --log-failed` 确认，不是猜测）**：两次失败都是
**同一个原因**——`console_line_editor` 相关的新增/修改文件（`client/mod.rs`、
`script_catalog.rs` 等）落地前没跑 `cargo fmt`，导致 `check.cmd` 的
portable quality gate 在 CI 上因 rustfmt diff 非空而 fail closed。**不是**
逻辑 bug、**不是**功能回归，纯粹是提交前少了一步格式化。第三个提交
（进行中的 "style: apply rustfmt..."）看起来就是在补这一步。

**这件事本身不需要 v0.1.14 才能修**（已经在修了），但值得在 v0.1.14 里
留一条「纪律」类的叶子：多文件跨平台改动（尤其是新增整份文件，如这次的
6 个 `console_line_editor.rs` 变体）在推送前必须本地跑一次
`cargo fmt --check` 或 `.\lint.cmd`，而不是依赖 CI 才发现。这类失败拖慢的
是「本该很快」的合入节奏，属于 §三 精度审查里反复出现的「小修复」类别，
不是深层设计问题。

---

## 三、precision-audit（`plan/precision-audit.md`）遗留给 v0.1.14 的项

本轮持续 review（`/loop` 驱动，见 `plan/precision-audit.md`）已经落地了一批
真实 bug 修复（fork/exec 竞态、Windows STILL_ACTIVE 歧义、last-error 顺序、
settings.json/workspace.json 非原子写等），多数已收口。截至本文写作时，
仍标记为「需要决策才能继续」的项：

```text
├─ [ ] item 22：script_protocol.rs 的 ScriptFrameTracker（seen_frames /
│      known_invocations）+ agenterm-rhai.rs 的 completed 集合，三个
│      HashSet 在长寿命 persistent worker 里只增不减。已确认可达（persistent
│      worker 不会自动重启），但修复方案（加上限 + 淘汰策略）会改变
│      重放/取消判定的语义，需要产品侧拍板上限值和失败模式，
│      不适合审查 agent 自己决定
└─ [ ] item 16 剩余观察：instances 目录在 Linux/macOS 上，当 HOME 和所有
       XDG_* 环境变量都缺失时，会静默退化到共享的 /tmp，且没有像
       linux/ipc.rs::ensure_private_directory 那样做符号链接/祖先目录加固。
       触发条件很窄，但机制已经明确；`protect_private_directory`/
       `metadata_is_real_directory` 已经是仓库里现成的可复用构件
```

两项都不是"发现了但看不懂"，而是"发现了、机制清楚、但下一步是设计决策"，
适合直接进 v0.1.14 的候选清单，而不是继续挂在 precision-audit 的 Open 里。

---

## 四、v0.1.14 候选目标树（占位，未定版）

```text
v0.1.14  Trust tail + platform-narrowness tail（暂拟名，待人工定名）
│
├─ A. 继承 v0.1.13 Wave B（信任面收口的未完成部分）
│  ├─ [ ] macOS pointer Unsupported 诊断信息清晰化
│  ├─ [ ] B 组 facade 纯转发删除（等跨 target CI 可用后执行）
│  └─ [ ] 六平台 parity-smoke 矩阵门禁记录（Linux/macOS 部分依赖 CI 证据）
│
├─ B. precision-audit 决策项收口
│  ├─ [ ] 决定 script_protocol/agenterm-rhai 三个 dedup HashSet 的上限
│  │     策略（typed 淘汰 vs 时间窗口 vs 其它），落地后回填
│  │     plan/precision-audit.md item 22
│  └─ [ ] 决定 instances 目录共享-/tmp 退化路径是否需要符号链接加固
│        （若需要：复用 protect_private_directory / metadata_is_real_directory）
│
├─ C. CI/提交纪律（低风险、可随时做）
│  ├─ [ ] 多文件/新文件改动前置 `cargo fmt --check` 或 `.\lint.cmd`
│  │     的检查清单化（本次两次 CI 失败的直接教训）
│  └─ [ ] 六平台 CI 矩阵健康度做一次有界巡检（而非持续轮询），
│        确认最近失败只有本文 §二 记录的这一类，没有被忽略的深层问题
│
└─ D. 明确暂不纳入（继续挂在 v0.2.0，避免范围蔓延）
   ├─ [ ] 巨型状态机拆解（Unix ~223KB / Windows ~266KB 状态机文件）
   ├─ [ ] snapshot 填充管线统一（R2，跨端 host 上下文差异大，性价比低）
   ├─ [ ] Workflows / 大 Control Center 内容 / net / WebView 生产化
   └─ [ ] M8/M9（可选智能 / LLM 网关）——两者都要求先有具体用户场景证据
```

---

## 五、明确非目标（继承 v0.1.13 纪律，未来 review 请勿放宽）

- 不创建 `v0.1.14` tag；Candidate/Release 仍需独立 exact-SHA 授权链。
- 不因为「顺手」把 v0.1.13 §八/§九已经复核过「不成立」或「归 v0.2.0」的
  跨平台抽象项（R1/R2/R3/R4）重新拉回本版本——除非出现新证据。
- 不用「弱模型开放式微重构」替代有边界的叶子任务；precision-audit 决策项
  必须先有人工拍板的上限值/策略，再落代码。
- 不把 Wave B 的「跨 target 才能验证」项目伪装成本机可独立完成的项目。

---

## 六、与其它文档的关系

| 文档 | 关系 |
|------|------|
| `plan/plan-v0.1.13.md` | v0.1.13 权威执行记录；本文 §一 是对它的只读复核，不改写其内容 |
| `plan/precision-audit.md` | 持续代码审查的权威记录；本文 §三 只是把它里面「需要决策」的项摘出来放进候选树，决策后仍应回写到 precision-audit 本身 |
| `plan/ARCHITECTURE.md` | 现行结构 SSOT；本文不重画结构树 |
| `PRD.md` / `prd/PRD_02_18_roadmap.md` | 产品真相与里程碑权威；v0.1.13/v0.1.14 目前都是 M11 之后、M12 之前的信任收口迭代，尚未获得独立里程碑编号——若要正式立项，应先在 roadmap 增补对应条目 |
| `prd/PRD_02_17_delivery_quality.md` | Candidate/Promotion 合同；本文 §二 的 CI 观测不构成任何发布决策 |

---

**给下一轮规划者的话**：这份文件是占位稿，目的是不让「v0.1.13 Wave B 还没
收口」「precision-audit 有两个决策项在等」「CI 刚因为忘记 fmt 红过两次」
这几件已知的事实散落在对话记录里。真正开工前，请先跟人工确认：
（1）v0.1.14 是否要正式立项，还是继续算 v0.1.13 的尾巴；
（2）§三 的两个决策项谁来拍板、拍板到什么程度算数；
（3）§四 D 组的排除项是否仍然成立。
