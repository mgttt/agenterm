# AgenTerm v0.1.15 公开计划（占位稿 / 思维工作树）

状态：**占位草案**（2026-08-04 起草，基于 v0.1.14 发布日全天真实遥测；
2026-08-04 晚外部 review 逐条对照最新代码核验并补充「现状（review）」行；
2026-08-04 深夜二次复核全部 review 行与 PRD 未来主线对齐，见 §五；
2026-08-05 补 macOS 真机安装/更新实测 → §一 G 组 + §八）。
不改变任何已发布/在途版本的授权状态；不创建 tag/Candidate/Release。
主题预定：**反馈左移 + 发布链降本**（附带交付后 install 卫生）——把「问题在
离引入点最远、最贵的车道才暴露」这一根因打掉。开工前需人工确认范围与
§一 D/G-P1 组、§五 5.7 的政策决策项。

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
│  │     现状（review，已核）：win-full-gate.yml 已有 release-stress profile
│  │     （check.cmd --release --include-stress，90min 上限），只缺
│  │     on: schedule；⚠️ 其 concurrency group = win-full-gate-{ref} +
│  │     cancel-in-progress: true，夜间定时同 ref 连跑会互相 cancel，
│  │     落地时需把 group 换成含 run_id 或接受单跑语义
│  ├─ [ ] A2 Candidate 自动触发：main CI 绿后经 workflow_run 自动派
│  │     （开关形态待定：commit 标记 / repo variable / 手动兜底保留）
│  │     动机：省派发往返延迟 + 收窄「HEAD 被并发推前」竞态窗口
│  │     注意：不改变 preflight 语义与授权链，只自动化 dispatch 这一步
│  │     现状（review，已核）：candidate.yml 现仅 on: workflow_dispatch；
│  │     加 workflow_run 后 source_sha 用 github.event.workflow_run
│  │     .head_sha（= 触发 CI 的 commit，preflight 的 GITHUB_SHA 检查
│  │     等价成立）；代价 = 触发器投递分钟级延迟，写进已知成本
│  ├─ [ ] A3 script-smoke 左移进 push CI（debug 版，实测 ~7s）
│  │     动机：v0.1.14 发布日它贡献 2 次腐化（operation 计数 22→24、
│  │     sidebar 投影竞态），左移后 6 分钟内暴露
│  │     现状（review，已核）：script-smoke 确认只在 release lane
│  │     （check.rhai smoke_ids）；94c3227 已把 remote-ui/fleet-smoke
│  │     并入 windows CI 的 release-lane-smokes 步骤，script-smoke 可
│  │     并入同一步骤而非新建步骤
│  └─ [ ] A4 per-gate timing 表写进 GITHUB_STEP_SUMMARY
│        动机：现在要下载 artifact 才能看每门耗时；诊断路径应一眼可见
│
├─ B. Candidate 门瘦身（每轮直接省时）
│  ├─ [ ] B1 agenterm-net-research 移出 release 门（→ CI 或夜间车道）
│  │     实测每轮 2.8min；research 隔离验证不属于产品资格证明
│  │     涉及 qualification-gates.json（fail-closed 声明）+ 政策复核
│  │     现状（review，已核）：check.rhai if release 内独立 gate（600s）、
│  │     qualification-gates.json 已声明、非 release 路径已标 skipped
│  │     ——移出=把「release 专属」改成「push CI 跑一次」，路径清晰
│  ├─ [ ] B2 缓存 key 对版本行归一化后再 hash
│  │     动机：版本冻结提交使 hashFiles 全变 → 每版本首轮全量重编
│  │     （~10min/版本）；归一化后冻结提交命中上一版缓存
│  │     成本：hashFiles 换脚本算 key，两 workflow（ci.yml / candidate.yml）一致性维护
│  │     现状（review，已核）：⚠️ 缓存 key = hashFiles('rust-toolchain.toml',
│  │     'Cargo.lock', 'Cargo.toml', 'build.rs', 'scripts/artifacts.json')
│  │     ——Cargo.lock 也在 key 里（版本冻结改 4 行），归一化必须同时
│  │     剔除 Cargo.lock 与 Cargo.toml 的版本行（root + agenterm-platform
│  │     两个 package）；建议共享脚本统一算 key，六 workflow 引用同一
│  │     脚本；build.rs / scripts/artifacts.json 保持敏感
│  └─ [ ] B3 artifact-build 与 artifact-build-fast 产物复用审计
│        两者合计 3.8-5.3min；若 fast 车道可复用主构建产物可省 1-2min
│        （先审依赖关系再动，可能结论是「保持分离」）
│        预判（review，已核）：release-fast = release + lto=false +
│        codegen-units=16 + incremental（Cargo.toml 实证），产物不可直接
│        互换；更现实的省法是 fast 车道复用主构建的同一 target 增量缓存，
│        先测命中率再决定是否动依赖关系
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
│        现状（review）：引用 run 30907369093 在前轮 review 中确认存在；本地 gh 不可用未复验，落地时以 Actions 页面复核
│
├─ D. 政策决策项（需人工拍板，agent 不自主执行）
│  ├─ [ ] D1 Candidate preflight 从「SHA == main HEAD」放宽为
│  │     「main 祖先 + 该 SHA 有绿 CI」
│  │     动机：HEAD 竞态在 v0.1.14 发布日实咬两次（c46eb70 无法重封印、
│  │     发布期并发 push 风险）；放宽后仍是 exact-SHA 封印，完整性不降
│  │     反方：钉 HEAD 保证「发布的就是最新」；放宽后可能发布落后于
│  │     main 的 SHA —— 需要明确这是否可接受
│  ├─ [ ] D2 smoke 并行分片（14 个拆 2-4 runner）
│  │     现值低（smoke 全绿仅 90s）；仅当 smoke 数量/时长显著增长再议
│  └─ [ ] D3 发布窗口纪律 vs 工具化：发布期并发 agent 推 main 的协调
│        （若 D1 通过则大幅弱化此需求）
│
├─ E. 发布链卫生（低成本噪音/存储治理）
│  ├─ [ ] E1 pages-build-deployment 噪音：每次 push 都产生一个
│  │     pages build run（GitHub Pages 自动构建），占 Actions 列表与
│  │     存储且与产品资格无关；确认是否需要 Pages（不需要则关设置
│  │     消除源头），需要则纳入清理策略
│  │     现状（review）：仓库启用 Pages（docs/ + CNAME 生效），用户此前
│  │     报告 Actions 列表存在大量 pages-build 噪音；域名为 agenterm.mega.tech，
│  │     与用户所述 agenterm.work 的归属/迁移关系见 §五 决策项 P1
│  └─ [ ] E2 定期清理旧 run：moltbaby 侧已有 gh-ci-cleanup.sh
│        （支持 --hours/--days/--keep-release-runs/--keep-pages-build/
│        --verify-rounds/--dry-run，删除后全量复核），agenterm 侧
│        建议 cron 保留 14 天；runbook 素材来自 plan-v0.1.13 §10.2.1
│
├─ F. Linux 云桌面实测尾账（2026-08-04 DISPLAY=:1，详见 §七）
│  ├─ [x] 单测误耦合：child_id_remains_stable_after_wait 把
│  │     top_level_window_supported 绑到 hosted_script_worker_available
│  │     （有 X11 才失败；无 DISPLAY 的 CI 绿掩盖）——已修进 main
│  ├─ [ ] F1 云环境快照补齐 libxkbcommon-x11-0 + libxcb-xkb1
│  │     （缺则 agenterm/agenterm-cc 在 xkbcommon-dl panic）
│  └─ [ ] F2 云桌面默认 Xft.dpi=96（VNC 0mm + DPI=-1 → scale≈0.99，
│        触发 control_center_linux_renderer_evidence）
│
├─ G. 安装/更新体验（2026-08-05 macOS aarch64 真机：0.1.12-local → v0.1.14，详见 §八）
   ├─ [ ] G1 macOS 默认 `curl | bash` 失败面：无 signed asset 时
   │     必须 AGENTERM_ALLOW_UNSIGNED_PREVIEW=1 才装得上
   │     动机：现网 v0.1.14 只有 `*-macos-*-unsigned-preview.zip`；
   │     未设 env 的 install 报「signed unavailable」即死，happy path 断
   │     建议（择一或组合，政策见 G-P1）：
   │       a) 无 signed 时自动回落 unsigned-preview 并打印信任模型警告
   │       b) 发布页/README 首屏固定写 macOS 必带 env 的一行命令
   │       c) 提供 `agenterm-cli update` 封装上述选择
   ├─ [ ] G2 升级后 BIN 断链清理：旧 `agenterm-script` 等残留 symlink
   │     动机：0.1.12-local 有 agenterm-script；0.1.14 包改为 agenterm-rhai
   │     （另含 agenterm-cc/agenterm-server 未入 BIN 链接集）。install 只
   │     replace REQUIRED_EXECUTABLES 五元组，不删 BIN 中指向
   │     `$INSTALL_ROOT/current/*` 但目标已不存在的孤儿链 → 实测
   │     `~/.local/bin/agenterm-script` 断链
   │     建议：装完扫描 `$BIN_DIR/agenterm*`，orphan 且 target 落在
   │     current/releases 下则移除并 say；可选把 agenterm-cc/server 纳入
   │     optional link 集合（或明确「仅五元组进 PATH」契约）
   ├─ [ ] G3 版本可观测性：GUI `agenterm --version` 拒收；无 VERSION 文件
   │     动机：用户/agent 要确认「窗口还是旧」时只能
   │     `agenterm-cli --version` 或 strings 二进制；GUI launcher 帮助
   │     不暴露版本；`~/.local/share/agenterm/current` 无旁路 VERSION
   │     建议：`agenterm --version` 打印即退（不启 GUI）；install 写
   │     `current/VERSION` 或 `INSTALL_ROOT/installed.json`
   │     （version/channel/source_tag/installed_at）
   ├─ [ ] G4 升级后运行态提示（install 收尾）：装完未告知「已开窗口仍旧码」
   │     动机：install 成功后 symlink 已指新版，但既有 GUI/server 仍旧映像
   │     建议：收尾 say 明确步骤；探测 live server/GUI 时打印 pid+旧 version
   ├─ [ ] G7 **升级后自适应 / 用户可理解提示**（产品需求，2026-08-05 用户点名）
   │     动机（真机复现路径）：
   │       1) 磁盘已升到 0.1.14，旧 0.1.12 server 仍在跑
   │       2) 关窗对话框默认 = `keep-server-running`（保留 server）
   │       3) 用户选默认/不关 server 再进 → 仍 attach 旧权威 → 标题/行为仍 0.1.12
   │       4) 用户无法从文案得知「升级生效 = 必须 stop-server-and-exit 再开」
   │     非目标：不强制静默杀会话；不削弱 keep-server 的会话保留语义
   │     验收（可证伪）：
   │       - 装盘 version ≠ live server version 时，用户**无需读文档**即可知道下一步
   │       - 走 keep-server 再 attach 旧 server 时，不会被误以为「安装失败」
   │     建议形态（可组合，实现时择优；政策见 G-P2）：
   │       a) **install 收尾自适应文案**（最低成本）：
   │          若探测到 live server/GUI 且 version < installed：
   │          打印「磁盘已是 X，运行中仍是 Y(pid=…)。要启用 X：关窗时选
   │          *退出 server*（stop-server-and-exit），或执行
   │          `agenterm-cli shutdown` 后重开。选 *保留 server* 将继续跑 Y。」
   │       b) **GUI/attach 运行时提示**（体验主路径）：
   │          client 二进制 version ≠ attached server version 时：
   │          启动条/模态一次性提示「会话 server 仍是 Y，本机已装 X；
   │          要切换到 X 请 stop-server 后重开」+ 显式按钮
   │          [继续用 Y] / [停止 server 并重开为 X]
   │       c) **关窗对话框升级感知**（减少误选默认）：
   │          当本机 installed/current version > 本进程/server version 时，
   │          将 default_action 改为 `stop-server-and-exit`，或在
   │          keep-server 选项旁标注「将继续使用旧版 Y，不会启用已装 X」
   │       d) **可选自动切换**（须 G-P2 批准）：
   │          install 结束或 attach 发现版本落后时提供
   │          `agenterm-cli update --apply-running`：优雅 shutdown → 起新 server
   │          → 恢复 workspace（restore_behavior 已有 restart-processes）；
   │          默认 off 或仅 CLI 显式 flag，避免 silent 丢交互态
   ├─ [ ] G5 无 first-class 更新入口 / 无 old→new 摘要
   │     动机：无 `agenterm-cli update` / `install.sh --check`；不打印
   │     当前已装版本、channel（unsigned-preview vs signed）、是否已最新
   │     建议：resolve 后对比 current；已最新则 no-op 退出 0；否则打印
   │     `0.1.12-local → 0.1.14 (macos-unsigned-preview)` 再下载
   ├─ [ ] G6 releases 目录不修剪：0.1.11-local / 0.1.12-local 永久堆积
   │     建议：保留 current + N 个历史（默认 2）或 `AGENTERM_KEEP_RELEASES`
   ├─ [ ] G-P1（政策）macOS 长期 channel：unsigned-preview 是否为默认
   │     公开通道，还是必须等 Developer ID 签名 asset 才算 stable
   │     （影响 G1 默认行为与 Promotion 文案）
   └─ [ ] G-P2（政策）升级时对 running server 的默认策略：
         仅提示 / 关窗改 default / 提供一键 apply（G7 a–d）——
         用户已要求「自适应或提示，否则用户不知道该怎么做」；
         agent 不自主改 keep-server 默认语义，须人工拍板后再改 default_action

├─ H. 分发面地基（Hub 前置，只做地基不做 Hub；对应 PRD 未来树 M13/M14）
│  ├─ [ ] H1 生成 `releases.json` 发布索引（CI 静态产物）
│  │     动机：install.sh 现在靠字符串拼 artifact 名 + `releases/latest`
│  │     重定向猜版本；未来 `agenterm-cli update`、agenterm.work 下载页、
│  │     Hub 客户端会各自再 scrape 一遍 GitHub → 四个真相源
│  │     现状（已核）：v0.1.14 资产共 23 项，每包已带 `.sha256` +
│  │     `.provenance.json`，另有 sbom.spdx.json / qualification-receipt.json /
│  │     candidate-manifest.json；字段齐全，索引可**纯派生**不新造事实
│  │     建议：release.yml 成功后由 provenance 派生 `releases.json`
│  │     （channels{stable,preview} + releases[].artifacts[]{os,arch,
│  │     variant,name,sha256,provenance,signed,notarized}）发到 Pages；
│  │     `variant` 字段直接解掉 macOS `-unsigned-preview` 后缀猜测
│  ├─ [ ] H2 install.sh 改为消费 `releases.json`（与 G1/G5 合并落地）
│  │     动机：G1 的 macOS happy path 断裂本质是「后缀靠 env 变量猜」；
│  │     有索引后它退化成读一个 `variant` 字段
│  │     建议：与 G5（old→new 摘要 / already-latest no-op）同批改，
│  │     避免两次动同一段 resolve 逻辑
│  ├─ [ ] H3 provenance 用户可见化（把 CI 证据交到用户手上）
│  │     动机：`.provenance.json` 每包都发但**用户端零消费**——install.sh
│  │     只校 sha256，从不下载 provenance
│  │     建议：下载并校验 provenance 的 sha256/version/source_tag 与实测
│  │     一致，收尾打印 commit / tag / build_log / signed / notarized；
│  │     与 G3 的 `installed.json`（version/channel/variant/source_commit/
│  │     sha256/installed_at/provenance 原文）同一批写入
│  ├─ [ ] H4 修 `provenance.sbom_sha256` 空串
│  │     动机：**已实测核实** v0.1.14 linux-x86_64 的 provenance
│  │     `sbom_sha256` 确为空字符串——声明了字段却未填，是真实证据缺口，
│  │     且 Hub 信任分级（M14）要复用这个字段
│  │     建议：打包步骤把 `dist/agenterm-<version>-sbom.spdx.json` 的
│  │     摘要写进各平台 provenance；低风险，纯补值
│  ├─ [ ] H5 agenterm.work 接通（**依赖决策项 P1**，本版只做别名不改内容）
│  │     现状（已核）：根 CNAME 与 docs/CNAME 均为 agenterm.mega.tech，
│  │     docs/index.html 的 canonical/og:url 同；agenterm.work 未接任何内容
│  │     建议：agenterm.work 设为 canonical，mega.tech 301 过去；
│  │     README 的 raw.githubusercontent 安装命令换成
│  │     `https://agenterm.work/install.sh`（技术债短链化，不改脚本实现）
│  │     联动：与 E1（pages-build 噪音）取向绑定——走 Pages 则 Pages 保留
│  └─ [ ] H6 PRD 未来树落文：M13（分发面）/ M14（Hub 底座）
│        **已落地**（本轮已写入 `prd/PRD_02_18_roadmap.md`），
│        与 §五 L-EXT / L-PKG 主线互链
│        非目标：本版**不写任何 Hub 代码**，不建 registry，不动 softmgr
│
└─ S. 结构 SSOT 机读化 + 微重构预备（契约=`plan/ARCHITECTURE.md` §8；**HOLD 待用户通知**）
   ├─ [ ] S0 状态：多 agent 并行中 → **本泳道不写主树**；仅文档预备
   │     复审触发：用户通知「可 review 新一轮再开工」
   ├─ [ ] S1 扩 `boundary_tests`（单向 A 档）：必存在 bins/关键目录、
   │     禁复活路径（如已删 services/frontend）、可选 adapter 行数软预算
   ├─ [ ] S2 代码→文档围栏（B 档）：扫描 `src`/`crates`/`src/bin` 生成
   │     structure 块；CI 与 ARCHITECTURE 围栏 diff（失败=结构漂）
   ├─ [ ] S3（可选，长期）`architecture.manifest` 真源（C 档）：
   │     清单驱动生成 md 块 + 同一清单喂测试；**不**新开第二份现行结构 md
   └─ [ ] S-prep 预备树（§九）：复审清单 + 微重构刀序 + 文件域互斥
         债务钩 L2/L3/L4 在 ARCHITECTURE §4；落地须同批回写 §1/§3
```

## 二、排序建议（起稿人观点）

1. **A1 + A3 + A4**：一晚可落地，直接消灭 v0.1.14 发布日最大痛苦源。
2. **A2**：随后落地，发布全链自动化闭环（人只拍 Promotion 前的最终板，
   或连 Promotion 也自动 —— 后者是政策问题，归 D 组讨论）。
3. **B1**：独立叶，收益确定（每轮 -2.8min）。
4. **B2**：版本发布日专项收益；实现前先在分支验证 key 稳定性。
5. C 组按复发率排优先级；D 组等人工。
6. **F1 + F2**：云快照一改即永久消除桌面 smoke 首轮噪音（见 §七）；
   单测误耦合已修，不必再排期。
7. **G2 + G4 + G7a（提示文案）**：安装卫生 + 升级后「该怎么做」可理解性；
   不碰发布链语义，可与 A 组并行。G7b/c（GUI/关窗对话框）为体验主路径，
   建议紧随 G7a；G7d / 改 default_action 依赖 **G-P2**；G1/G-P1 等
   macOS channel 政策拍板后再改默认回落行为。
8. **S 组 HOLD**：多 agent 在途时不抢主树；用户通知后先 **§九 复审** 再 S1→微重构。
   不必等 S3 全文双向；S1（可选 S2）安全带后即可小步刀。S3 不阻塞主主题。

> v0.1.15 是**纯发布链经济学**版本，不与 §五 未来主线（net / CC 内容 /
> 远程包管理 / computer-use）抢工期；未来主线只做「对齐记录 + 决策项」，
> 实际开工各自归口 v0.2.0 及后续版本 plan。

## 三、明确非目标

- 不动 Candidate/Promotion 的授权语义（D1 除外，且 D1 只在人工批准后做）。
- 不为提速削弱资格覆盖：任何门的移除/降级都要有「该验证去了哪里」的答案
  （如 B1 的 net-research 移去 CI/夜间，而不是删除）。
- 不做投机性并行化（D2 现值低）。
- **不把 §五 未来主线塞进 v0.1.15**：agenterm-net 稳定化、Control Center
  内容成熟、远程包管理、computer-use 各归其版本 plan 与 owning PRD。


## 三·五、UI/UX 现场观察（2026-08-05，自截图 + ui-snapshot-full.json + 源码复核）

> 证据：dist/evidence/{tab-tree-uiux-review,sidebar-zoom,sidebar-top-zoom,tab-tree-collapsed}.png
> ＋ ui-snapshot-full.json（1180x760 窗口，dark 主题）+ src/ui_geometry.rs + unix frontend render.rs。
> 全部为「观察/建议」，不改变 v0.1.15 授权范围；按影响面标注归口（v0.1.15 顺手 / v0.2.0+）。

### 3.5.1 标签树区（重点）

| # | 观察（证据） | 问题 | 建议 | 归口 |
|---|--------------|------|------|------|
| T1 | 行高 36px = name 17px + note 16px；10 tab 中 9 个 note 为空仍占满 | 空 note 行浪费 ~44% 垂直空间，视口可容行数少 | 无 note 时单行渲染（行高 ~20px）或按内容自适应 | v0.2.0+ |
| T2 | status 状态点几何存在（8x9，快照有），render_sidebar 无绘制调用 | 运行/退出/错误状态不可见，树行左侧留空 | 补渲染 status 色点（复用 success/warning/danger 调色板） | v0.1.15 顺手 |
| T3 | control_hover / control_pressed / active_border 全仓零使用 | 按钮、active 行无 hover/pressed/边框反馈 | 工具栏与树操作按钮接 hover/pressed；active 行加 active_border | v0.2.0+ |
| T4 | TREE_INDENT=10px，CJK 宽字符层级感弱 | 深层级树难辨归属 | 缩进 10→14~16px 或加连接线分段着色 | v0.2.0+ |
| T5 | marker 为文本 [+]/[-] 3 字符 | 与 11x11 expander 几何不符，视觉粗糙 | 换 8x8 三角/箭头字形，保持 hit 区不变 | v0.2.0+ |
| T6 | 树连接线用 divider 1px | 层级线不醒目 | 保留（或浅色变体），低优先 | 观察 |

### 3.5.2 工具栏 / 状态栏 / 整体

| # | 观察（证据） | 问题 | 建议 | 归口 |
|---|--------------|------|------|------|
| TB1 | 工具栏 7 按钮同底同色，无 hover/pressed（同 T3） | 无可点击性提示 | 与 T3 同修 | v0.2.0+ |
| TB2 | tabs 按钮 52px 标签 "<Tabs" | 信息性弱，与 New 无主次 | 折叠时可显示 tab 计数或当前 tab 名 | v0.2.0+ |
| SB1 | terminal/sidebar scrollbar visible=true 且 max_offset=0 | 无内容可滚动仍占 12px 轨道 | 无可滚动内容时隐藏滚动条 | v0.1.15 顺手 |
| SB2 | 状态栏 cwd 260px 显示全路径 | 窗口窄时挤压其它段 | 紧凑模式（home 缩写 + 省略号） | v0.2.0+ |
| W1 | 窗口标题带 profile 后缀（如 custom:uiux-review） | 用户可见噪音 | 发布构建隐藏 profile 后缀 | v0.1.15 顺手 |


## 四、与其它文档的关系

| 文档 | 关系 |
|------|------|
| `plan/plan-v0.1.14.md` | 上一版执行记录；本文数据与止血项的出处 |
| `plan/plan-v0.1.13.md` §10.2.1 | 发布链坑清单（runbook 素材，E2 配套） |
| `plan/ARCHITECTURE.md` | 结构 SSOT（含 §8 对齐机制/工具边界）；**S 组**执行叶指针；本文不重画结构树 |
| `prd/PRD_02_18_roadmap.md` M12 | Control Center 内容成熟（§五 L-CC 的版本归口；原 plan-v0.2.0.md 已并入） |
| `plan/plan-mobile.md` | 移动端计划（第三个 host：接入端 + 去中心化链接端）；与 L-NET/L-PKG 共享去中心化底座，文件域独立 |
| `prd/PRD_02_17_delivery_quality.md` | Candidate/Promotion 合同；D1 若通过需回写 |
| `prd/PRD_02_18_roadmap.md` | 里程碑权威（M11 收敛 / M12 = v0.2.0） |
| `prd/PRD_02_19_inspiration_and_future_vision.md` | 灵感库；§五 各主线 promotion 的入口 |
| `prd/PRD_02_21_control_center.md` | Control Center 边界与能力树 |
| `prd/PRD_02_22_decentralized_network.md` | agenterm-net 成熟度门（N0→N4） |
| `prd/PRD_02_20_native_platform.md` | Platform Facade 收口证据（§五 前置判断） |
| `plan/precision-audit.md` | C 组竞态根因复核的记录处 |
| `install.sh` | 安装/更新实现 SSOT；§八 / G 组改进入口 |

---

## 五、未来主线对齐（PRD 对比，2026-08-04 深夜补充）

> 目的：把「当前发布链经济学」与「产品未来主线」对齐，避免 v0.1.15
> 完工后产品断档。以下主线按用户已声明的方向整理（ipfs/libp2p、Control
> Center 内容、扩展能力台、rhai、远程包管理、computer-use），每线标注
> PRD 归口、成熟度现状、以及「开工前需拍板的决策项」。移动端
> （`plan/plan-mobile.md`，第三个 host）与 L-NET/L-PKG 共享去中心化底座。

### 5.1 前置判断：多平台 UI/UX 对齐 + 底层库封装（用户第一关注）

现状（review，已核）：

- Platform Facade 已是**唯一生产原生边界**（PRD_02_20 revision 4 全 [x]）：
  产品代码无 OS 分支，机制全部经 `crates/agenterm-platform` 能力化；
  边界闸 `src/platform/boundary_tests.rs` 拦截新原生导入/OS-selection。
- 共享 UX 语义单点化已收敛（ARCHITECTURE.md 分层）：interaction/selection/
  modal/focus/snapshot schema 两端共用；Win remote 与 Unix embedded 剩余
  差异是合法 host 适配边界（对账 vs 同树内联、host 控件绑定）。
- 证据矩阵 `plan/platform-ux-parity-evidence-matrix.md`：startup / wake /
  focus 三平台全 Supported；`remote-ui`（Windows-only 契约）与
  `unix-frontend`（跨 Unix host）按分支隔离；macOS physical pointer
  acceptance 仍 open（PRD_02_18 M11 行）。

**结论**：底层库封装已妥当；UI/UX 对齐已基本达成，剩「macOS 物理指针 +
  矩阵持续回归」尾账（归 v0.1.14/v0.1.15 发布链照常维护，不阻塞主线开工）。

### 5.2 主线 L-NET：ipfs/libp2p 去中心化网络（PRD_02_22）

| 项 | 状态 | 归口 |
|----|------|------|
| N0 选型/合同 | [x] | PRD_02_22 |
| N1 独立本地证明（identity/connect/CID/block） | [x] | research/agenterm-net |
| N2-M1 受控全节点纵切（node 生命周期/durable store/mesh/remote attach） | [~] 进行中 | v0.1.12 计划 + research |
| N3 产品消费者（Script API / InfoHub / CC 诊断） | [ ] | 归 v0.2.0+ |
| N4 server 服务集成（typed facade，不 link 引擎进权威） | [ ] | 更远期 |

关键约束（已核）：`agenterm-net` 是独立可选进程；二进制 2 MiB 门；
  默认 off、无 install/GUI autostart 监听；terminal/server 热路径零依赖。
  N2 剩余开放证据：三平台 fault/load、崩溃恢复、upgrade/downgrade、
  backup 加密/多设备语义。

**与 v0.1.15 的关系**：B1（net-research 移出 release 门）**不**削弱
  net 资格——research 验证仍每晚在 CI/夜间车道跑，只是不再占发布门。

#### 5.2.1 进度实查（2026-08-05，回答"做到哪个 exe 了"）

用户问"不太记得之前做到什么进度、做到哪个 exe"。**实查结论**：

**没有产品 exe，全部在 `research/agenterm-net/` 这个隔离 workspace 里。**

| 核查项 | 结果 |
|--------|------|
| 主 workspace 是否含 libp2p/ipfs | **否**——`Cargo.toml` members 仅 `[".", "crates/agenterm-platform"]`；根 `Cargo.toml` 与 `crates/*/Cargo.toml` 全无 libp2p/ipfs 依赖 |
| 是否有 `agenterm-net` 可执行体 | **否**——`src/bin/` 下七个 bin（`agenterm` / `-cli` / `-mux` / `-rhai` / `-server` / `-mcp` / `-cc`），无 net |
| 代码在哪 | `research/agenterm-net/`，**自带 `[workspace]`**（刻意脱离主构建图），`version = "0.0.1"`，`publish = false`，描述自称 *"Disposable ... research spike"* |
| 代码量 | 7 个模块约 **177 KB**：`main.rs` 49KB / `attach.rs` 31KB / `mesh.rs` 26KB / `store.rs` 11KB / `identity.rs` 21KB / `node.rs` 22KB / `tcp_fixture.rs` 17KB |
| 依赖面 | libp2p 0.56（gossipsub / kad / noise / ping / relay / request-response / tcp / yamux / cbor）+ `cid` 0.11 + `multihash-codetable` + sha2 |
| CLI 子命令 | `capabilities` / `peer-id` / `self-test` / `mesh-self-test` / `attach-self-test` / `tcp-self-test`（均 `--json`），另有十余个 `--json` 分支 |

**已被 CI 证明跑通的能力**（`scripts/rhai/agenterm-net-research.rhai`
在每次 release 门里真跑，本轮实测 142.2s，receipt schema
`agenterm-net/result/v1`，断言逐条列在脚本里）：

- 进程隔离：listener/connector 双进程，**PID 与 PeerId 均不同**、
  握手成功、bounded ping 往返、listener 生命周期可观测、
  子进程干净退出 + 孤儿清理武装 + 强制清理可 reap。
- 资源度量：peak child RSS > 0、最大子线程数 > 0、两次采样完整。
- 块存储：**round-trip 校验通过、损坏块被拒、store 可删除**。
- 静态质量：`clippy --locked --all-targets -D warnings` + `cargo test --locked` 全绿。

**对照 §5.2 的状态表**：N1（独立本地证明 identity/connect/CID/block）
确实 `[x]`——上面这些就是它的证据。N2-M1 标 `[~] 进行中` 也吻合：
`mesh.rs` / `attach.rs` / `node.rs` 已有实体且有各自 self-test 子命令，
但**尚未产出产品可消费的 typed facade**，也没有任何 `src/` 代码 import 它。

近期提交（`git log -- research/agenterm-net/`）显示最后动作集中在
"证明一次 bounded ping 往返 / 校准 self-test 阶段预算 / 收敛 listener
阶段 deadline / 显式恢复崩溃本地节点 / 保持 durable peer 身份生命周期"
——即**在补 N2 的鲁棒性证据**，方向与状态表一致。

> 结论一句话：**进度在 N1 完成、N2 进行中；产物是一个隔离的 research
> spike 二进制（非产品 exe），通过独立 self-test + JSON receipt 自证。**
> 下一步真正的门槛不是再加协议能力，而是 §5.2 表里的 **N3 产品消费者**
> ——决定它以什么形态（Script API / InfoHub / CC 诊断）被产品调用。

### 5.3 主线 L-CC：Control Center 内容成熟（PRD_02_21 → v0.2.0）

- v0.1.11 壳层已 shipped（进程边界/typed bridge/Cockpit read-only）；
  v0.2.0（PRD_02_18 M12，原 plan-v0.2.0.md 已并入）做内容成熟。
- 用户点名内容：**workflow/pipeline 工作台**（C1 promoted →
  MCP orchestration authority + CC 投影）、**AgenTerm 扩展能力台
  【插件/皮肤/信息】**（J4 promoted → softmgr substrate + PluginHub/
  AppHub 分视图）、**InfoHub**（J5 promoted）。
- 用户提示 **Control Center 可能改名** —— 见 §五 决策项 P2。
- rhai 能力（PRD_02_10）：unrestricted 本地运行时已 shipped；CC 消费
  task catalogs/automation primitives，但 CC **不引入** Script 权限层
  （AGENTS.md 铁律：能力≠授权）。

### 5.4 主线 L-EXT：扩展能力台【插件/皮肤/信息】+ rhai

- 插件/应用：J4 → softmgr（PRD_02_04）单一 catalog/source/install/
  update/rollback substrate；PluginHub 与 AppHub 是同一底座的两个
  产品级视图，不是两套包系统（PRD_02_18 M12 行）。
- 皮肤：既有 theme（Dark/Light + 自定义主题文件，PRD_02_06）为底座；
  「皮肤」扩展面需与 plugin 打包体系合并定义（见决策项 P3）。
- rhai：扩展脚本/任务目录已走 `agenterm-rhai` unrestricted runtime；
  包管理与脚本分发未来可接 L-NET 的内容寻址（H-T1 CID-signed modules）。

**产品设计补充（2026-08-05，已写入 PRD_02_18 M14）**：把 M12「PluginHub 与
AppHub 同底座」这句**扩展到全部四类 Hub**——plugin / skin / app / info
只是同一包描述里 `kind` 字段的取值，共用 catalog、验签路径与事务安装器。
这直接给出 **P3 的候选答案：皮肤不是新的扩展体系，是 `kind: skin` 的包**
（纯数据、权限清单为空、宿主耦合最低），因此它也是验证整条
catalog→install→rollback 链路**最安全的第一个靶子**，建议 SkinHub 先落地。
信任分级 `first-party / verified / community / unverified` 由 provenance +
SBOM + sha256 推导——本仓的发布链已经产出这三样（见 §7.3 与 H3/H4），
**这是相对多数插件市场的真实差异化点**，而非新造机制。
执行类（plugin/app）默认要求 ≥ `verified` 且需声明权限清单；
非执行类（skin/info）可放宽到 `community`。见新增决策项 P6。

### 5.5 主线 L-PKG：远程包管理（agenterm.work 域名）

- 用户声明：`https://agenterm.work/` 对应本仓；目前仓库 CNAME 与
  docs canonical 均为 `agenterm.mega.tech`（已核：根 CNAME + docs/CNAME
  + docs/index.html canonical/og:url）。**域名归属/迁移是待拍板项 P1**。
- 未来形态：远程 catalog / source / 更新服务，供 softmgr 事务消费；
  与 E1（pages-build 噪音治理）联动——若 agenterm.work 只是 Pages
  CNAME 迁移，则 Pages 需保留且 E1 改走清理策略；若另有独立服务，
  Pages 可关。

### 5.6 主线 L-CU：computer-use（自有实现，尚未入 PRD）

- 现状：仓库/PRD/plan 均无 computer-use 条目（已 rg 全仓核实）——
  属于**未捕获的新主线**，按 PRD_02_19 promotion 工作流需先入
  灵感库/owning module（可能归 Agent control plane 或专门化智能
  PRD_02_12 的衍生叶），再进版本 plan。
- 自有实现倾向：复用 Platform Facade 已有能力（screenshot /
  process-window / input / process-reference），不引入外部 computer-use
  框架；与 M8/M9（可选智能/LLM 网关）独立，证据门先行。
- 见决策项 P4：是否立项、归口哪个 PRD、首发平台与证据门。

#### 5.6.1 用户补充方向（2026-08-05）：`agenterm-remote.exe` 远程控制协议族

用户诉求原文要点：**控制远程资源**，规划 `agenterm-remote.exe` 逐步支持
`current` / `ssh` / `rdp` / `vnc` 等协议，做成 computer-use 的控制工具；
**`current` 最急**；参考 moltbaby 的 `my-computer-use` / `computer-use`。

**已核实的可复用资产**（`/Users/wjc/repos/moltbaby/skills/computer-use/`）：

| 资产 | 内容 | 对本仓的价值 |
|------|------|-------------|
| `SKILL.md` 的**洋葱分层**方法论 | native primitive → 通用 CLI → profile selector → workflow → 壳命令，**只允许外层依赖内层** | 直接可搬的分层契约，天然匹配本仓 Platform Facade 边界纪律 |
| `macos/`（原 my-computer-use，已合并） | Swift native AX + CGEvent + TS wrapper，含 helper daemon/client 拆分 | macOS 后端参考；daemon/client 拆分与本仓 process-reference 思路一致 |
| `windows/` | Python UIA/CDP/ctypes + C FFI；**已含 `_rdp.py` / `_freerdp.py`** | Windows 后端参考；**RDP 已有实作经验**，非从零 |
| `linux/` | AT-SPI2 桥接（框架就绪） | Linux 后端参考 |
| `shared/cu.md`、`computer-use.mindmap.md` | 操控 API 文档与认知地图 | 抽象命令集设计的起点 |

**关键设计判断（起稿人观点，待 P4 拍板）**：

1. **`current` 不是"一种远程协议"，而是协议族的 local 退化档**。
   把 `current`（控制本机）与 ssh/rdp/vnc 放进**同一套抽象命令集**
   （截图 / 枚举窗口与控件树 / 点击 / 输入 / 剪贴板 / 文件传输），
   `current` 只是 transport = in-process 的那一档。这样先做 `current`
   不是"临时方案"，而是**把接口钉死的最省事路径**——后续加 ssh/rdp/vnc
   只换 transport，不动上层 workflow。
2. **`current` 档应尽量复用本仓已有能力**，而不是移植 moltbaby 的 TS/Python：
   Platform Facade 已有 screenshot / process-window / input /
   process-reference，`workbench-smoke` / `platform-ux-parity-smoke`
   已在三平台证明这些原语可用（本轮发布亲测：`gui_child.window_pointer` /
   `window_message` / `window_control` 均在 CI 真机跑通）。
   moltbaby 的价值是**分层方法论与命令集设计**，不是具体实现语言。
3. **独立可执行体、默认 off**，与 `agenterm-net` 同一纪律：
   不 link 进 terminal/server 热路径，不默认监听，二进制体积设门。
   远程控制是高权限能力，**默认关闭 + 显式授权**是底线。
4. **协议优先级**：`current` → `ssh`（无 GUI，纯命令/文件，最易做证据门）
   → `rdp`（可复用 moltbaby `_freerdp.py` 经验）→ `vnc`。
   ssh 排第二不是因为需求急，而是因为它的证据门最好写，能先把
   "transport 可换"这个架构假设证伪或证实。

**给 v0.1.15 的准备工作（不实现，只钉接口与证据）**：

- [ ] CU0 立项判定（P4）：是否进 PRD、归口哪个 owning module。
- [ ] CU1 抽象命令集草案：把上表 6 类操作写成 typed 契约，标注
      `current`/`ssh`/`rdp`/`vnc` 各档的**可支持性矩阵**（哪些操作在
      哪些 transport 下无意义，例如 ssh 无窗口树）。
- [ ] CU2 复用清单：逐条核对 Platform Facade 现有原语能覆盖 `current`
      档的哪几条命令，缺口列出来（这一步只读代码，零风险）。
- [ ] CU3 证据门形态：参考 `agenterm-net-research` 的做法——
      **独立 workspace + 自证 self-test + JSON receipt**，
      先不进 release 门（见 §5.2 B1 教训）。

> 风险提示：远程控制 + computer-use 是**高危能力面**（可被用于横向移动）。
> 建议 CU0 拍板时一并确定授权模型（每会话显式授权？密钥绑定？审计日志？），
> 而不是留到实现阶段补。

### 5.7 决策项（需人工拍板，agent 不自主执行）

| ID | 决策 | 影响 |
|----|------|------|
| P1 | agenterm.work 与 agenterm.mega.tech 的归属/迁移（Pages CNAME 还是独立服务） | 决定 E1 走向 + L-PKG 基建 |
| P2 | Control Center 是否改名、改什么名 | 影响 PRD_02_21 标题/命名、可执行族与文档 |
| P3 | 「皮肤」扩展面与 theme/plugin 打包的边界 | 决定 L-EXT 的范围与版本归口 |
| P4 | computer-use 是否立项、归口 PRD、首发平台与证据门 | 决定 L-CU 是否进 v0.2.0 或更后 |
| D1–D3 | 见 §一 D 组（发布链政策） | 与产品主线独立，但 A2/B1 落地依赖 D1 取向 |
| G-P1 | macOS unsigned-preview 是否为默认公开通道（无 signed 时自动回落 vs 强制 opt-in） | 决定 G1 默认行为与 install/README 首屏命令 |
| G-P2 | 升级遇到 running server：仅文案提示 / 关窗改 default 为 stop-server / 一键 apply 热切换 | 决定 G7 落 a–d 哪几档；用户已要求自适应或提示 |
| P5 | 分发面归属：agenterm.work 作单一入口（含 releases.json 索引）还是仅 docs 别名 | 决定 H1/H5 形态与 E1 走向；P1 的具体化 |
| P6 | Hub 是否统一为单一 `kind` 底座（plugin/skin/app/info 共用 catalog+验签+事务）还是分立系统 | 决定 P3 皮肤边界的答案与 M14 的范围 |

---

## 六、决策记录

| 日期 | 决策 |
|------|------|
| 2026-08-04 | v0.1.15 主题定为反馈左移 + 发布链降本（占位稿，未授权开工） |
| 2026-08-04 | 代码复核：win-full-gate profile/并发组、candidate dispatch-only、script-smoke 仅 release lane、net-research release 门、hashFiles 缓存 key、release-fast profile、Pages/CNAME、gh-ci-cleanup.sh 参数均属实；run 30907369093 与 pages-build 噪音为 review 结论（本地 gh 不可用，落地时以 Actions 复核） |
| 2026-08-04 | §五 未来主线按用户声明对齐 PRD；P1–P4 为待拍板决策项，未开工 |
| 2026-08-04 | 并发提交 2c5f3d4 已并入 plan-v0.1.15.md 主体与 plan-mobile.md；本工作区仅剩自审修正（E1 措辞 / 决策记录口径 / §三 引用） |
| 2026-08-05 | 自截图 + ui-snapshot-full.json + 源码复核完成标签树区 UI/UX 观察（§三·五 T1-T6/TB1-TB2/SB1-SB2/W1）；全部为观察不改变 v0.1.15 授权范围；T2/SB1/W1 标 v0.1.15 顺手、其余归 v0.2.0+ |

| 2026-08-04 | Linux 云桌面（DISPLAY=:1 XFCE）实测意见写入 §七 / F 组；单测误耦合已修进 main；F1/F2 为环境快照尾账，不走 PR |
| 2026-08-05 | macOS aarch64 真机 0.1.12-local→v0.1.14 安装更新实测写入 §八 / G 组；G1–G6 + G-P1 为改进需求，未授权开工 |
| 2026-08-05 | 用户确认：升级后「关窗不退 server → 再进仍显示旧版」属真实踩坑；要求更新时**自适应或提示**，否则用户无法知道该选 stop-server；追加 **G7 + G-P2**，升 G7a 为 P0 文案、G7b/c 为体验主路径 |
| 2026-08-05 | 结构对齐/工具澄清 upsert：`ARCHITECTURE.md` §8 + 债务 L4；本 plan 增 **S 组**（S1 扩闸 / S2 围栏 / S3 manifest）；明确 LSP≠结构契约引擎 |
| 2026-08-05 | S 泳道 **HOLD**：等其他 agent 完成；用户再通知 → 新一轮 review → 再开工；预备树写入 **§九**（不改代码） |

---

## 九、结构微重构预备树（HOLD · 2026-08-05）

> 状态：**等待**。多 agent 开工期间本泳道只读/只更本文档，**不写** `src/**` / `crates/**` / `install.sh`。  
> 触发：用户说「可以 review 新一轮再开工」。  
> 契约：`plan/ARCHITECTURE.md` §8；债务 L2/L3/L4。  
> 原则：**不必等 S3 全文双向**；有 S1（+可选 S2）+ 同批回写 ARCHITECTURE 即可小步微重构。

```text
HOLD 多 agent 并行
│
├─ W0 静默纪律
│  ├─ 不抢主树单写者；不 git commit 结构债「半成品」
│  ├─ 不改 boundary_tests 行为（除非开工后 S1 授权）
│  └─ 发现他方已动热文件 → 记入 W1 冲突表，不并行硬改
│
├─ W1 开工前复审闸（用户通知后第一动作 · 只读）
│  ├─ git status / log --oneline -20 / 他方 pathspec 热区
│  ├─ 重读 ARCHITECTURE §1§4§8 与 boundary_tests 现状
│  ├─ 跑 quick（或至少 boundary 相关 test）取基线绿
│  ├─ 对照下表「候选刀」是否被他方占用 → 重排刀序
│  └─ 产出：一页「可开 / 让路 / 延后」三列（聊天或 §九 补记）
│
├─ W2 安全带（结构文档↔代码 · 最小集，开工第一刀可选）
│  ├─ S1 boundary_tests 扩：bins 必存在、禁复活路径、（可选）行数软预算
│  ├─ （可选）S2 structure 围栏生成 + diff
│  ├─ 明确不做：S3 manifest 本轮非阻塞
│  └─ 验收：闸红=结构漂；闸绿 ≠ 全文 prose 已对齐（人仍回写 §1）
│
├─ W3 微重构刀序（行为不变优先 · 单写者串行）
│  ├─ 刀1  client/mod.rs 切分
│  │      域：src/client/** 新子模；禁碰 adapters
│  │      验收：cli/script/mux 入口行为不变 + quick
│  ├─ 刀2  services/policy 半迁移收口（L3）
│  │      域：src/platform/{services,policy,mod}.rs
│  │      验收：无新增 dead_code 门面；或删未接线 facade
│  ├─ 刀3  unix/frontend 子模切分（仅拆文件，不改语义）
│  │      域：src/platform/adapters/unix/frontend/**
│  │      验收：unix smoke / 既有 gui 测路径
│  ├─ 刀4  windows remote_frontend 对称切分（刀3 后或文件域空闲时）
│  │      域：…/windows/remote_frontend.rs → 子模
│  │      验收：remote/windows 相关测
│  ├─ 刀5  ui-action 表驱动（R6，需 ActionId 完备性测）
│  │      域：src/frontend/* + 两端 adapter match 收敛
│  │      风险中：宜 S1/S2 后、双端文件无他人在途
│  └─ 延后  G7/G-P2 升级 UX、H 分发面、发布链 A/B —— 非本预备树
│
├─ W4 每刀闭环清单（开工后强制）
│  ├─ 改前：pathspec 声明 + 与 W1 冲突表核对
│  ├─ 改中：禁止顺手「改进」相邻语义
│  ├─ 改后：quick 绿 + ARCHITECTURE §1/§3/§4 同批一句
│  └─ 提交：pathspec 精确；message 带刀号（刀1/刀2…）
│
└─ W5 明确非目标（本预备树）
   ├─ 不等 S3 才开工
   ├─ 不把 LSP 当对齐完成证据
   ├─ 不重画第二棵现行结构 md
   └─ 不在 HOLD 期写主树「抢跑」
```

### 9.1 热文件互斥备忘（复审时更新）

| 域 | 代表路径 | 与谁易撞 |
|----|----------|----------|
| 发布链 | `.github/workflows/*`, `scripts/rhai/check*.rhai` | A/B/E 组 |
| 安装更新 | `install.sh`, CLI update 相关 | G/H 组 |
| 结构闸 | `src/platform/boundary_tests.rs`, `plan/ARCHITECTURE.md` | **S 组自有** |
| 双主机 GUI | `unix/frontend/**`, `windows/remote_frontend.rs` | UX/parity 他方 |
| client | `src/client/**` | script/mcp 他方 |

---

## 七、Linux 云桌面实测意见（2026-08-04）

宿主：Cursor Cloud `DISPLAY=:1` TigerVNC + XFCE（非 CI Xvfb）。
入口与 CI 同款：`AGENTERM_BOOTSTRAP_TASK=… ./scripts/bootstrap.sh`。

### 7.1 结果（环境补齐后）

| 套件 | 结果 |
|------|------|
| `control-center-linux-smoke --backend x11` | PASS |
| `unix-frontend-linux-smoke` | PASS |
| `./check.sh --quick` | PASS（615 lib） |

产品侧 Linux GUI journey **本身可绿**；首轮失败几乎全是环境/断言耦合，不是渲染回归。

### 7.2 失败树（按暴露顺序）

1. **缺 `libxkbcommon-x11-0`**（连带 `libxcb-xkb1`）  
   `agenterm` / `agenterm-cc` 在 `xkbcommon-dl` panic：
   `Library libxkbcommon-x11.so could not be loaded`。  
   README 已列包；云快照未装 → **F1**。

2. **`scale_factor ≈ 0.9896 < 1.0`**  
   VNC `xrandr` 报 `0mm×0mm`，XFCE `Xft/DPI=-1` → winit 给出亚 1.0 scale；
   smoke 断言 `scale_factor >= 1.0` 失败于 `control_center_linux_renderer_evidence`。  
   会话内 `Xft.dpi: 96` + `xfconf-query …/Xft/DPI -s 96` 后 scale=1.0、全绿 → **F2**。  
   意见：断言保持 `>= 1.0` 合理；应修环境默认 DPI，不要放宽产品契约。

3. **单测误耦合（已修）**  
   `child_id_remains_stable_after_wait` 要求  
   `top_level_window_supported == hosted_script_worker_available()`。  
   后者 Windows-only；前者在 Linux 有 X11 时为 true。  
   **无 DISPLAY 的 CI 绿掩盖，桌面 Quick 必挂**——典型「反馈左移」反例，
   与 v0.1.15 主题同构。修复：去掉该等式，只断言非 GUI 子进程无窗。

### 7.3 意见（给 v0.1.15 / 环境维护）

- **云环境 install**：把 README 的 X11 运行库写进快照（至少
  `libxkbcommon-x11-0 libxcb-xkb1`）；桌面会话默认 `Xft.dpi=96`。
- **不要用 headless CI 代替桌面观察**：`platform_facts` / scale / focus
  类断言在有 DISPLAY 时语义不同；Quick 若在桌面跑，应用真 DISPLAY。
- **Linux host-native smoke 可继续只在 push-main + Xvfb**；云桌面是
  额外真机车道，适合抓 F1/F2 这类快照缺口，不必再拆 PR。
- AGENTS.md Cursor Cloud 段已补 smoke 前置说明，与本 § 互为索引。

---

## 八、安装与更新实测（2026-08-05，macOS aarch64）

> 场景：本机已装 `0.1.12-local-macos-aarch64`（`current` 指向
> `~/.local/share/agenterm/releases/…`；BIN 链在 `~/.local/bin`），
> GitHub 已发布 `v0.1.14`（含 unsigned-preview zip）。由 agent 执行
> `AGENTERM_VERSION=v0.1.14 AGENTERM_NO_LAUNCH=1
> AGENTERM_ALLOW_UNSIGNED_PREVIEW=1 bash install.sh` 完成升级。
> 对照源：`install.sh`（resolve / symlink / macOS channel）、
> `agenterm-cli --version`、`readlink …/current`、BIN 目录。

### 8.1 结果

| 检查项 | 结果 |
|--------|------|
| 下载 + SHA-256 校验 | PASS（`aace8af7…`） |
| `current` → `0.1.14-macos-aarch64` | PASS |
| `agenterm-cli --version` → `0.1.14` | PASS |
| 五元组 BIN 链（agenterm / cli / mux / rhai / mcp） | PASS |
| 无 env 的 macOS happy path | **未走通**（见 G1；须 `ALLOW_UNSIGNED_PREVIEW=1`） |
| 旧 `agenterm-script` BIN 链 | **断链残留**（装完后仍在，手动 `rm`） |
| 已运行 GUI 是否自动吃新码 | **否**（须重开窗口） |

### 8.2 问题树（按暴露顺序）

1. **版本确认成本高**  
   `agenterm --version` → GUI launcher 报 unknown option；只能
   `agenterm-cli --version` 或 `strings` 二进制里的
   `TERM_PROGRAM_VERSION`/`0.1.12`。→ **G3**

2. **macOS 默认安装命令不可用**  
   发布资产名为 `…-macos-aarch64-unsigned-preview.zip`；
   `install.sh` 仅在 `AGENTERM_ALLOW_UNSIGNED_PREVIEW=1` 时改
   `PACKAGE_STEM`。未设 env 时下载 signed 名失败并 fail-closed。
   对「发了 0.1.14 请更新」的一线指令不友好。→ **G1 / G-P1**

3. **升级不清理过时 BIN 名**  
   local 0.1.12 曾链 `agenterm-script`；0.1.14 payload 无此文件
   （脚本面为 `agenterm-rhai`）。`replace_symlink` 只覆盖
   `REQUIRED_EXECUTABLES`，不扫孤儿。→ **G2**

4. **payload 与 PATH 契约未文档化**  
   0.1.14 zip 另有 `agenterm-cc`、`agenterm-server` 等，install 不链
   进 `BIN_DIR`。合理与否需写成契约，避免「装了但 PATH 没有 cc」。
   → **G2 可选叶**

5. **运行中实例无切换提示**  
   升级成功后用户窗口仍显示/行为旧版本直至退出重开；
   install 收尾无 say。→ **G4**

6. **关窗默认 keep-server → 再进仍旧版（用户主诉，产品缺口）**  
   关窗 `default_action = keep-server-running`；用户若按默认保留
   server，再开窗 attach 旧权威进程，标题/行为仍为旧 version（例：
   磁盘 0.1.14、运行 0.1.12）。用户无法从 UI 得知「启用新版 =
   必须 stop-server-and-exit 或 `agenterm-cli shutdown` 后重开」，
   易误判为安装失败。→ **G7**（自适应/提示；政策 **G-P2**）

7. **无 update 语义**  
   不比较已装版本；不打印 channel；已最新仍会重下重装（本轮因
   显式 `AGENTERM_VERSION` 未踩，但 `resolve_version=latest` 路径
   同样缺 no-op）。releases 下旧 local 目录永留。→ **G5 / G6**

### 8.3 建议落地切分（给 v0.1.15）

| 优先级 | 项 | 改动面 | 风险 |
|--------|----|--------|------|
| P0 | G2 孤儿 symlink 清理 | `install.sh` 收尾 | 低：仅删指向 current 且 target 缺失的 agenterm* 链 |
| P0 | G4 + **G7a** 升级后可理解步骤 | `install.sh` say + live version 探测 | 低：不杀进程，只文案 |
| P1 | **G7b** attach 版本不一致提示 / **G7c** 关窗对话框升级感知 | GUI + window_close | 中：文案/默认项需 UX 拍板（G-P2） |
| P1 | G3 VERSION 文件 + `agenterm --version` | install + GUI launcher 早退 | 中：launcher 参数解析需测 |
| P1 | G5 old→new / already-latest | `install.sh` | 低 |
| P2 | G7d 一键 apply 热切换 | cli + shutdown/restore | 中高：会话/交互态；**须 G-P2** |
| P2 | G1 自动回落 unsigned | `install.sh` + 文案 | **政策依赖 G-P1** |
| P2 | G6 keep-N releases | `install.sh` | 低；勿删仍被非 current 链引用的目录 |

### 8.4 与 v0.1.15 主题的关系

- 不改变 Candidate/Promotion 授权语义；属**交付后用户路径**卫生。
- 与 E 组（发布链噪音）独立；与 L-PKG（远程包管理）远期可汇合
  （`agenterm-cli update` 未来可接 softmgr），但 v0.1.15 只做
  install.sh / 本地可观测性，不预支包服务。
- 复现命令（脱敏）：

```bash
# 查当前
readlink ~/.local/share/agenterm/current
agenterm-cli --version

# 升到指定 tag（macOS 现网）
AGENTERM_VERSION=v0.1.14 \
AGENTERM_NO_LAUNCH=1 \
AGENTERM_ALLOW_UNSIGNED_PREVIEW=1 \
  bash install.sh

# 装后自检
agenterm-cli --version
ls -la ~/.local/bin/agenterm*
# 期望：无断链；version=0.1.14；已开 GUI 需手动重开
```

