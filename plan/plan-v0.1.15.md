# AgenTerm v0.1.15 公开计划

状态：**主波已合 main；公开发版未授权**（2026-08-05 定版；2026-08-06 收口；
2026-08-07 起新叶改走 [`plan-v0.1.16.md`](plan-v0.1.16.md)）。
素材与推迟表全文仍保留在 §1 / §2.6 / §3.5 / §5 / §7–§12。
不改变任何已发布/在途版本的授权状态；不创建 tag/Candidate/Release。

**主题：发布链降本（cache 优先）+ 交付后 install 卫生。**
比占位稿的「反馈左移 + 发布链降本」**更窄**：反馈左移只保留最便宜的两叶，
夜间彩排与自动派发推 v0.2.x——理由基于实测数字，见 §2。

政策决策项全文见 §5.7；阻塞关系见 §2.5。

**三端 agent 并发派工（历史）**：见 **§2.2.1**。  
**现行派工**：[`plan-v0.1.16.md`](plan-v0.1.16.md)。  
平台封装 / shared-first 纪律见 `AGENTS.md`、`plan/plan-platform-encapsulation-gap.md`。

**Win must-ship 状态（2026-08-06 收口）**：R1–R4、A3–A4、G2/G3/G6/G7a、  
H1/H3/H4、B1–B5、P0-1–P0-3、U1/U3、mux/mcp 独立 PE 移除、multi-lease /
As Window **已在 main**。  
U2 真机证据 / B6 / H2 / M* / N1 等 → **v0.1.16**。Unix 多实例深度见 0.1.16 O 组。  
**本版不发布 tag/Candidate/Release** 直至人工授权。

## 0. 数据来源与关键事实（全部实测，可复现）

v0.1.14 发布日 ~10 轮 gate 级遥测，加 2026-08-05 对成功 Candidate
`30942173420` 的逐门/逐 job 分析（详见 `plan/archive/plan-v0.1.14.md` §7）：

```text
单轮全绿路径 ≈ 30min：CI ~5min → Candidate ~15-18min → Promotion ~1min
关键路径 = windows-x86_64 单个 job 16.6min（次慢 job 5.5min，3 倍差）
  拆解：bootstrap（worker 重建）80.9s ＋ 39 门串行 869.1s ≈ 950s（与实测吻合）
  门耗时前三占 55%：artifact-build 211.3s / net-research 142.2s /
                    artifact-build-fast 127.5s
  14 个 smoke 合计仅 124.4s（14.3%）——「smoke 慢」是错觉
Candidate 失败 15–32min（贵）；Promotion 失败 13–36s、成功 ~59s（近乎免费）
失败构成（10 轮）：6 次确定性测试腐化（从未在 CI 车道执行过的断言）
  ＋ 4 次共享 runner 负载竞态
```

> ⚠️ 占位稿曾写「net-research 2.8min / smoke ~90s」，与上表不符。
> 以本节为准（142.2s / 124.4s），差异来自不同轮次与冷热缓存。

**2026-08-05 新增实测（占位稿完全没有，且是最便宜的杠杆）**：

```text
仓库 Actions cache = 9.9 GB / 10 GB 上限，19 个 entry
  （gh api 实测；2026-08-05 二次复验仍 9.9GB —— 是常态不是瞬时）
CI 的 debug target cache 独占 8.7GB，同一家族存 2–3 份陈旧世代
后果：撞顶后 LRU 驱逐 → Candidate 自己的 cache（target 0.22GB +
  home 0.06GB）在下次 Candidate 用到前就被 CI 挤掉
证据：四次 Candidate 的 bootstrap 全是 worker.state="rebuilt"，
  且成本单调上涨 47.1s → 49.7s → 59.4s → 80.9s
已排除 key 漂移：538ec73/bffb7b8/ac068ff/8ff2b5a 四个 commit 的
  Cargo.lock / Cargo.toml / scripts/artifacts.json 哈希完全相同
另核：cargo-home-candidate-v2 只有 key、**无 restore-keys**
  （对照 cargo-target-v2 有），hashFiles 一变即彻底 miss、无近似回退
复现：gh api repos/mgttt/agenterm/actions/caches
```

v0.1.14 已落地的止血（不再重复投入）：失败也保存构建缓存（`always()`）；
remote-ui/fleet smoke 左移进 push CI；release 车道 smoke retry-once；
wake pump 余量。

---

## 1. 目标树素材全集（**非执行清单**——执行看 §1.5）

> 本节保留占位稿的 A–H + P + S 全部原始条目与 review 行，作为**素材与依据**。
> 取舍结果见 §1.5；未纳入本版者的推迟理由见 §2.6。

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
│  │     （check.rh smoke_ids）；94c3227 已把 remote-ui/fleet-smoke
│  │     并入 windows CI 的 release-lane-smokes 步骤，script-smoke 可
│  │     并入同一步骤而非新建步骤
│  └─ [ ] A4 per-gate timing 表写进 GITHUB_STEP_SUMMARY
│        动机：现在要下载 artifact 才能看每门耗时；诊断路径应一眼可见
│
├─ B. Candidate 门瘦身（每轮直接省时）
│  ├─ [ ] B1 agenterm-net-research 移出 release 门（→ CI 或夜间车道）
│  │     实测每轮 2.8min；research 隔离验证不属于产品资格证明
│  │     涉及 qualification-gates.json（fail-closed 声明）+ 政策复核
│  │     现状（review，已核）：`scripts/rh/check.rh` if release 内独立 gate（600s；
│  │     历史称 `check.rhai`，已归档至 `scripts/archive/rhai/`）、
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
│  │     与用户所述 agenterm.work 的归属/迁移关系见 §5 决策项 P1
│  └─ [ ] E2 定期清理旧 run：moltbaby 侧已有 gh-ci-cleanup.sh
│        （支持 --hours/--days/--keep-release-runs/--keep-pages-build/
│        --verify-rounds/--dry-run，删除后全量复核），agenterm 侧
│        建议 cron 保留 14 天；runbook 素材见 `prd/PRD_02_17_delivery_quality.md`
│        §Release-chain operating requirements（v0.1.13/v0.1.14 两轮坑
│        已合并去重为版本无关要求）
│
├─ F. Linux 云桌面实测尾账（2026-08-04 DISPLAY=:1，详见 §7）
│  ├─ [x] 单测误耦合：child_id_remains_stable_after_wait 把
│  │     top_level_window_supported 绑到 hosted_script_worker_available
│  │     （有 X11 才失败；无 DISPLAY 的 CI 绿掩盖）——已修进 main
│  ├─ [ ] F1 云环境快照补齐 libxkbcommon-x11-0 + libxcb-xkb1
│  │     （缺则 agenterm/agenterm-cc 在 xkbcommon-dl panic）
│  └─ [ ] F2 云桌面默认 Xft.dpi=96（VNC 0mm + DPI=-1 → scale≈0.99，
│        触发 control_center_linux_renderer_evidence）
│
├─ G. 安装/更新体验（2026-08-05 macOS aarch64 真机：0.1.12-local → v0.1.14，详见 §8）
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
│  ├─ [x] H1 生成 `releases.json` 发布索引（CI 静态产物）
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
│  ├─ [x] H3 provenance 用户可见化（把 CI 证据交到用户手上）
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
│        与 §5 L-EXT / L-PKG 主线互链
│        非目标：本版**不写任何 Hub 代码**，不建 registry，不动 softmgr
│
├─ P. 粘贴失败硬骨头（终端区 + 输入区/composer；2026-08-05 用户实测，详见 §10）
│  ├─ [ ] P1 **UTF-8 / 异源大段文本**（他终端复制 → 粘贴常失败）
│  │     症状：从别的 terminal 复制大段（疑含 emoji / OSC 色码 / 混合控制符 /
│  │     非严格 UTF-8 字节）粘到 AgenTerm **终端区或 composer**，提示失败
│  │     （用户侧一度归因为「特殊 utf8 字符」）
│  │     代码锚（现状）：
│  │       - 读盘：`agenterm-platform` clipboard `String::from_utf8` 失败 →
│  │         `clipboard_backend_error`（macOS pbpaste / Linux 同类路径）
│  │       - 归一：`src/ui_clipboard.rs` `normalize_{terminal,composer}_paste`
│  │         丢弃 `is_control()`（保留 \t 与换行族）；纯控制/转义残片可致空串
│  │       - 空串：统一 `clipboard text contains no pasteable characters`
│  │         （`TerminalPasteFailure::Empty` / composer 同文案）
│  │       - 上限：`TERMINAL_PASTE_LIMIT_BYTES = 256 KiB` → too large
│  │       - 异步：unix 终端粘贴 worker + focus/tab 变 → StaleTarget 等
│  │     硬点：异源 clipboard 编码不统一；终端拷贝常夹带 SGR/OSC；
│  │     emoji 本身非 control，更可能是 **读盘 UTF-8 严校验** 或 **归一后空/过大**
│  │     或 **异步竞态** 被误述成「特殊字符」——需分类诊断再改策略
│  │     建议方向（实现时择优，勿一次改三层）：
│  │       a) 读盘：非法 UTF-8 走 lossy / 替换字符，并区分错误码
│  │          `clipboard_invalid_utf8` vs backend；记录替换计数
│  │       b) 归一：可选「终端粘贴保留更多可打印 Unicode + 剥离 CSI/OSC」
│  │          单测：emoji、CJK、SGR 色码、CRLF、空剪贴板
│  │       c) UX：失败文案带 **可区分 code**（empty / invalid_utf8 / too_large /
│  │          stale / focus），禁一律「Paste failed: …」含糊
│  │       d) 证据：复现夹具（合成非法 UTF-8 字节、带 SGR 的「假终端拷贝」、
│  │          含 emoji 的合法 UTF-8 大段）进 unit 或 smoke
│  ├─ [ ] P2 **无文本剪贴板**（截图/图像类 → no pasteable characters）
│  │     症状：剪贴板是截图/图像（或仅非 Unicode 文本格式）时粘贴，
│  │     用户见 `clipboard text contains no pasteable characters`
│  │     代码锚：normalize 后 empty；或 Win `has_unicode_text()==false` /
│  │     get_text 无文本；macOS `pbpaste` 空/非 UTF-8 再归一空
│  │     硬点：platform clipboard **仅 get_text**——图像在 API 层已不可见
│  │     （§10.3 断裂点 A）；子 harness 会粘图也收不到父终端未投递的字节
│  │     建议方向：
│  │       a) T0：探测无 text / 有 image → code `clipboard_image_only`，
│  │          文案点明「未透传，非 harness 不支持」
│  │       b) T1（可选）：image → temp 路径字符串注入 PTY/composer
│  │       c) T2 非本版：多 MIME 真透传（须 PRD）
│  ├─ [ ] P3 错误码与反馈统一（P1/P2 共用）
│  │     终端 vs composer 双路径文案对齐；`last_feedback_error` /
│  │     status_message 必须带稳定 machine code（已有部分
│  │     `terminal_paste_*`，empty 仍常落 `terminal_paste_failed`）
│  │     建议：Empty 细分为 `clipboard_empty` / `clipboard_no_pasteable_text`
│  │     / `clipboard_image_only` / `clipboard_invalid_utf8`
│  └─ [ ] P-P1（政策，可选）非法 UTF-8 默认 lossy 还是硬失败；
│        图像粘贴是否永远拒绝——默认建议：**lossy 可选 + 图像硬拒绝文案**
│
├─ S. 结构 SSOT 机读化 + 微重构预备（契约=`plan/ARCHITECTURE.md` §8；**HOLD 待用户通知**）
│  ├─ [ ] S0 状态：多 agent 并行中 → **本泳道不写主树**；仅文档预备
│  │     复审触发：用户通知「可 review 新一轮再开工」
│  ├─ [ ] S1 扩 `boundary_tests`（单向 A 档）：必存在 bins/关键目录、
│  │     禁复活路径（如已删 services/frontend）、可选 adapter 行数软预算
│  ├─ [ ] S2 代码→文档围栏（B 档）：扫描 `src`/`crates`/`src/bin` 生成
│  │     structure 块；CI 与 ARCHITECTURE 围栏 diff（失败=结构漂）
│  ├─ [ ] S3（可选，长期）`architecture.manifest` 真源（C 档）：
│  │     清单驱动生成 md 块 + 同一清单喂测试；**不**新开第二份现行结构 md
│  └─ [ ] S-prep 预备树（§9）：复审清单 + 微重构刀序 + 文件域互斥
│        债务钩 L2/L3/L4 在 ARCHITECTURE §4；落地须同批回写 §1/§3
│
└─ O. **macOS / OSX 本机跟进泳道**（2026-08-05 派发 · 见 **§11** 完整作业规格）
   ├─ [x] O0 接手基线（先做，只读）
   ├─ [x] O1a **ImeStatus macOS adapter** `28d6959`（N1 osx 半叶）
   │     ⚠ O1b Unix 状态栏 IME 段 **未开工**（跨平台布局，等用户定）— §11.7
   ├─ [x] O2 粘贴 macOS 诊断完成、**判定不改码**（Ok("") 三态；T2 等 P-P1）— §11.11
   ├─ [x] O3 install G7a 文案 `ee41cc6`；G2 无断链；G1 等 G-P1 — §11.12
   ├─ [x] O4 合成路径对照：无需改码 — §11.9
   ├─ [x] O5 CPU 实测无需开工 — §11.10
   ├─ [x] O6 **Shift+选区复制** 已解 `fb573f9`（O6a 禁静默 + O6b shift-extend）
   │     交付 §11.13；定因 §11.8 **全部成立**；修饰键管线早已存在只是未读
   │     ⚠ 顺带暴露 main 红灯：`prd_alignment_public_command_missing:delete-buffer`
   │     （非 O6 引入、非 flake；须有人认领 PRD 公开命令目录，§11.13）
   ├─ [ ] O1b Unix 状态栏 IME 段 — **已拍板开工**（编排者 2026-08-05，见 §6）
   │     对齐 Win `refresh_ime_label`；poll `ime::status()`；禁伪造 full_shape
   ├─ [ ] O-fix 红灯认领：`prd_alignment_public_command_missing:delete-buffer`
   │     **已拍板**：补 PRD 公开命令面提及 B′ buffer 族（含 delete-buffer），
   │     使 prd-alignment 绿；**不**删 CLI 命令
   └─ [x] O-禁：禁 Win IME 域；P-P1/G-P1/O1b **已由编排 agent 拍板**（§6）
```

## 1.5. v0.1.15 收敛工作树（**这是可执行清单**；上面 §1 是素材全集）

§1 的 A–H + P + S 共约 30 叶，是多轮追加堆出来的，含大量「观察」而非
「可执行」。本节是取舍后的定稿：**只列进入 v0.1.15 的叶**，每叶带动机、
可证伪验收、成本、依赖。未列入者一律见 §2.6（推迟表，含推迟理由）。

**多 agent / 三端怎么并行**：叶仍在本节；**派工与文件域互斥见 §2.2.1**。

选择原则（v0.1.14 教训）：**宁可少而全绿，不要多而半途**——发布日 5–6 小时
耗在从未跑过的车道上，根因不是做得少，是同时开了太多没验证的面。

### R. 发布链降本（本版第一优先；全部有实测收益）

- [x] **R1 cache 配额治理** ★最高性价比
  - **动机**：9.9/10GB 撞顶 → LRU 驱逐 → Candidate cache 每轮全 miss，
    bootstrap 47s→81s **单调恶化**（见头部实测块）
  - **做法**：CI 的 debug target cache 限制保留份数或缩小缓存路径；
    必要时给 candidate 车道独立前缀，确保关键路径不被挤掉
  - **落地（Win CI-R · 配置已合）**：Windows push CI target cache 改为
    **v3-slim**（只缓存 `deps/build/.fingerprint/incremental`，不整包
    `target/debug/` PE）；key 前缀与 Candidate `*-candidate-*` 仍隔离。
    **全量验收**（连续两次 Candidate `bootstrap.worker.state==reused` +
    cache 总量 &lt;8GB）需后续 Actions 观测，本叶不宣称已测绿。
  - **验收（可证伪）**：连续两次 Candidate 的 timing artifact 中
    `bootstrap.worker.state == "reused"`（当前恒为 `"rebuilt"`）；
    且 `gh api .../actions/caches` 总量 < 8GB
  - **成本**：小（改 workflow cache 配置 + 一次清理）
  - **收益**：≈3min/次 Candidate；**依赖**：无
- [x] **R2 `cargo-home-candidate-v2` 补 `restore-keys`**
  - **动机**：它只有 `key` 无 `restore-keys`（对照 `cargo-target-v2` 有），
    hashFiles 一变即彻底 miss、无近似回退
  - **落地**：`candidate.yml` 所有 `cargo-home-candidate-v2` **restore**
    步已加前缀 `restore-keys`（含 windows-aarch64）。
  - **验收**：版本冻结提交后首轮 Candidate 日志出现前缀命中，
    而非 `Cache not found`（需下一次 Candidate 日志确认）
  - **成本**：极小（一行）；**依赖**：R1 先腾配额，否则命中也会被驱逐
- [x] **R3 net-research 移出 release 门**（原 B1）
  - **动机**：142.2s／16.4%，耗时第二名，却与发布产物正确性关系最弱
  - **做法**：改为 push CI 跑一次；**不是删除**——保留验证，只换车道
  - **落地**：`scripts/rh/check.rh` release 块不再跑 `agenterm-net-research`；
    `scripts/qualification-gates.json` 去掉该 required gate；
    push CI linux 仍 `AGENTERM_BOOTSTRAP_TASK: agenterm-net-research`。
  - **验收**：release 门不再含该 gate 且 push CI 含之；
    `qualification-gates.json` 声明同步（fail-closed 不破）
  - **成本**：小；**依赖**：无
  - **PRD 核对**：已 grep，无任何 PRD 要求它必须在 release 门（见 §2.7）
- [x] **R4 promotion dry-run**（新增叶，v0.1.14 直接教训）
  - **动机**：`release.yml` 首跑即藏 4 个缺陷；dry-run 可在几十秒内
    暴露其中 4/8
  - **做法**：加 `-f dry_run=true`，跑完 verify 全部断言但不建 tag/release
  - **落地**：`release.yml` 增加 boolean `dry_run`；verify 接受
    `dry-run-publish-vX.Y.Z`；`publish` job `if: inputs.dry_run != true`。
    **真跑 dry_run 需人工 dispatch**（本波只合配置）。
  - **验收**：`dry_run=true` 跑完 verify 且仓库无新 tag、无新 draft
  - **成本**：中；**依赖**：无
  - ⚠️ **本叶自身就是「没跑过的车道」**，配置已合后须用真实 Candidate run_id 自证一次

### A′. 反馈左移（只保留最便宜的两叶；A1/A2 推迟见 §2.6）

- [x] **A3 script-smoke 左移进 push CI**（debug 版，实测 ~7s）
  - **动机**：v0.1.14 发布日它贡献 2 次腐化；左移后 6 分钟内暴露
  - **做法**：并入 `94c3227` 已建的 windows CI release-lane-smokes 步骤
  - **落地**：`ci.yml` windows `release-lane-smokes` 增加
    `task run script-smoke`（与 remote-ui / fleet 同步）
  - **验收**：push CI 含 script-smoke 且 CI 总时长增幅 < 30s
  - **成本**：极小；**依赖**：无
- [x] **A4 per-gate timing 写进 `GITHUB_STEP_SUMMARY`**
  - **动机**：现在要下载 artifact 才能看每门耗时；R1 的验收也依赖它可读
  - **落地**：`timing-summary.rh` 本就会 append 门表到
    `GITHUB_STEP_SUMMARY`；Candidate 步改为 bash 明确调用，并追加
    **bootstrap.worker.state** 行便于 R1 观测
  - **验收**：Candidate 运行页直接可见逐门耗时表，无需下载 artifact
  - **成本**：小；**依赖**：无（但先于 R1 做更好验证）

### G′. 安装/更新卫生（用户真机踩坑，详见 §8；与发布链正交，可并行）

- [x] **G3 版本可观测性**：`agenterm --version` 打印即退 + 写 `installed.json`
  - **动机**：用户/agent 无法确认「窗口是不是旧版」；G7a 的判断也依赖它
  - **落地**：`agenterm` GUI PE 支持 `--version`/`-V`（parent console attach）；
    `install.sh` 写 `releases/…/installed.json`（version/channel/variant/…
    installed_at）。**OSX 真机**复验 install 路径。
  - **验收**：GUI 二进制 `--version` 不启窗口即输出版本；
    `current/installed.json` 含 version/channel/variant/source_commit/
    sha256/installed_at
  - **成本**：小；**依赖**：无（是 G7a/H3 的前置）
- [x] **G2 升级后孤儿 symlink 清理**
  - **动机**：实测 `~/.local/bin/agenterm-script` 断链残留（改名后遗留）
  - **落地**：`install.sh` 装完后扫 `BIN_DIR` 断链（目标落在 install root 下）
  - **验收**：装完后 BIN 下无「指向 current/releases 但目标不存在」的链
  - **成本**：小；**依赖**：无
- [x] **G7a 升级后自适应文案**（install 收尾）
  - **动机**：用户点名——磁盘已 0.1.14、running server 仍 0.1.12，
    关窗默认 keep-server → 再进仍旧版，用户无从得知该怎么做
  - **落地**：检测到 running agenterm 时打印 stop-server / `--version` 指引
  - **验收**：探测到 live server 且版本低于已装时，收尾文案明确给出
    「要启用新版该做什么」；**用户无需读文档**
  - **成本**：小；**依赖**：G3
  - **注**：纯文案，**不受 G-P2 阻塞**（G7b/c/d 才受）
- [x] **G6 releases 目录保留策略**（current + N，默认 2）
  - **动机**：`0.1.11-local` / `0.1.12-local` 永久堆积
  - **落地**：`install.sh` `prune_old_releases`；`AGENTERM_RELEASES_KEEP`
    默认 2；保留刚装目录；BIN 仍引用的目录不删
  - **验收**：装新版后旧目录按策略修剪，且不删仍被非 current 链引用者
  - **成本**：小；**依赖**：无

### U′. 标签切换假刷新（**用户 2026-08-05 现场**；观察见 §3.5.2.1）

> 标签区选不同 tab 时「基本会触发重新刷新渲染」，打断工作。根因是切 tab
> 路径上叠了 **no-op composer 写、无条件 font/layout、整屏 delta**，不是
> 「换 active screen 画一帧」本身。本子树把假刷新与真内容替换拆开排期。

```text
U′. 标签切换假刷新
├─ [x] U1 no-op composer / 同 metrics 跳过 font·layout / 同文案跳过 SetWindowText
├─ [ ] U2 真机回归 + 可选黑盒（空 composer 连点 tab 无 ComposerDraft 风暴）
├─ [x] U3 显示后再 debounce PTY resize（网格落后 tab，TS2）
└─ [ ] U4 纯 TabSelected 不重推整屏 cells（协议/delta 优化，可选）
```

- [x] **U1 假刷新热路径止血**（代码已落 main）
  - **动机**：见 §3.5 TS1
  - **做法（已实现）**：
    1. `ControlHost::apply_set_composer`（default）与 Unix 覆盖：文本相同 →
       no-op，不 `ComposerDraft`；
    2. Win `remote_frontend`：`sync_composer` / `load_composer` 相同则跳过
       IPC / `SetWindowText`；
    3. Win `apply_effective_terminal_font`：family/size 未变 → 不重建 HFONT、
       不 `layout()`、不 resize。
  - **涉及文件**：`src/control_dispatch.rs`、
    `src/platform/adapters/windows/remote_frontend.rs`、
    `src/platform/adapters/unix/frontend/mod.rs`
  - **验收**：同字体预设、空/未改 composer 下连点 tab，journal 无成对
    无意义 `ComposerDraft`；工具栏 HWND 不因切 tab 整组 `SetWindowPos`
  - **成本**：小；**依赖**：无

- [ ] **U2 真机回归与可选证据**
  - **动机**：U1 仅 `cargo check`；用户体感与事件风暴需 GUI 真机确认
  - **做法**：本机/冒烟连点 tab；可选：对 headless+UI 路径断言
    「select 且 composer 未变 → 不出现 ComposerDraft」
  - **验收**：用户确认假刷新明显减轻；若加黑盒则一条路径全绿
  - **成本**：极小–小；**依赖**：U1 合入

- [x] **U3 显示后再 debounce PTY resize**（§3.5 TS2）
  - **动机**：切到「网格仍停留在旧窗口尺寸」的 tab 时，立即
    `resize_active_terminal` → ConPTY/应用整屏重排（vim 等），体感仍重
  - **落地（Win remote）**：`deferred_pty_resize_deadline` + 100ms debounce；
    `select-tab` / 活动 tab 变化 / new-tab|child 只 schedule；窗布局仍立即 resize
  - **验收**：快速扫过 N 个落后网格 tab，只对最后停留的 tab 发一次
    resize；最终网格仍对齐当前 layout（**真机 U2 仍建议手测**）
  - **成本**：中；**依赖**：U1；**工期紧可砍**
  - **非目标**：取消 resize（尺寸必须最终正确）

- [ ] **U4 纯 `TabSelected` 不重推整屏 cells**（可选协议优化）
  - **动机**：`TabSelected` 事件仍使 `ui-delta` 携带该 tab 全量
    `ui_tab_bootstrap`（含 cells）；客户端已有完整 screen 时属冗余 IPC/CPU
  - **做法**：delta 在「仅 active 变更 + 无 output 事件」时只推
    `active_tab_id`（或 screen generation 未变则省略 cells）；客户端
    paint 换 active 即可
  - **验收**：select 路径 payload 显著小于全屏 bootstrap；错代
    （generation 落后）仍 fail-closed 拉全量
  - **成本**：中–大（协议/兼容）；**依赖**：U1；**默认推 v0.2.x 若 U1+U3 已够**
  - **非目标**：脏矩形增量 cell 协议（另立）

> **砍叶**：U4 最先砍；其次 U3；**U1/U2 保留**（用户直接痛点）。
> 与 R 组正交，可后置并行，**不**用 U 去挤 R1/R2。

### P0′. 关窗再开保会话（**用户 2026-08-05 严重事故** · 在修）

> **事故**：关 GUI 再开并选 `main` 后，窗口内容像全重置、agent「全退出」。
> 实测常为：**起了第二个 main server**，用 workspace.json **假恢复**
> （`restore_tab` = 重新 `spawn cmd.exe`，只恢复 title/note，**不**接回原 PTY）。
>
> **已修（工作区）**：
> - GUI `connect_or_start`：默认 endpoint 连不上时，先
>   `find_live_endpoint_for_logical_instance` **附着**同逻辑 instance 的 live peer；
> - `start_frontend_server_process`：**拒绝**在已有 live 同名 instance 时再 spawn；
> - recovery 路径同样优先 pin 到 live peer。
>
> **仍未解决（诚实边界）**：若 Keep-server 的 server **已随 GUI job 被杀**，
> 只能假恢复；需平台侧 breakaway 加固（另叶）。Workspace 不保存 agent 进程句柄。

- [x] **P0-1 附着 live peer，禁止静默第二 main**（代码已落）
- [x] **P0-2 黑盒**：同 instance 第二 server 进程退出；`start_frontend` 拒绝双开；
  tab 名/note 在单 server 存活时保持（隔离 instance 实测）
- [x] **P0-3 server 脱离 GUI job（Windows breakaway）** 防关窗误杀 server
  - **落地**：`autostart_server` 走 `spawn_detached_command`（breakaway +
    ACCESS_DENIED→CallerJobFallback）；单测锁源码路径。限制性 Job 下仍可能
    随父进程结束——诚实诊断，非假 Keep-Running。

### S′. 多 Server / Instance 可达与选择（**用户 2026-08-05**；关窗后找得回来）

> **触发**：用户现场 GUI 窗口全部关闭（可能与构建替换二进制有关），不深究
> 单次根因，但提出：窗口顶部要有**横向 tab 用来选 server**。
>
> **产品判断（已记入对话，本叶遵从）**：
> - **需求合理**——多 logical instance / 多 live server 要可发现、可附着、
>   身份可见；关窗后 server 可能仍 live（keep-server）。
> - **2026-08-06 用户拍板**：要的是 **窗口顶部横向 server tab 条**（点选切
>   当前窗附着的 server）。此前「默认不做顶栏 tab」的判断被用户明确覆盖。
> - **实现（2026-08-06 真机迭代后）**：server strip **仅终端列**（左缘对齐终端，
>   左侧 Tabs 独占 + 左上时钟）；标签树顶在时钟下；点选 = 本窗 rebind（鼠标
>   flush attach；全部芯片可点；进程存活即可 enter；stale 开新窗）；
>   CLI `select-server-tab --name`；`Open instance…` / `open-instance` 仍可新窗（S3）。
> - **仍不做**：无确认的静默双权威、把 server tab 画进左侧 PTY 树。

```text
S′. 多 Server / Instance 可达与选择
├─ [x] S1 启动/重开：live instance 列表 + 一键附着
├─ [x] S2 主窗身份常显（标题/状态栏：instance · pid · 简短 endpoint）
├─ [x] S3 主窗「打开另一 instance…」（新窗附着，非同窗热切）
└─ [x] S4 同窗热切换：顶栏 server strip 点选 rebind（用户拍板形态；非 PTY 树）
```

**硬约束**

| 约束 | 含义 |
|------|------|
| 单一 Fleet 权威 / 窗 | Phase A：**一 GUI 窗绑定一 server**；S3 开新窗，不在本窗静默换 endpoint |
| 不发明第二权威 | 列表事实来自 `list-instances` / `server-list` 同类注册表，不另造目录 |
| 与 PTY tab 分离 | Server/instance 选择器**不得**画成与左侧树同级的「又一套 tab 条」冒充终端标签 |
| 关窗 ≠ 本叶范围 | 构建杀窗、崩溃根因另案；本叶只保证**找得回、切得对** |
| L-CC 分工 | 多 server 控制塔投影归 CC（见 `plan/design-control-center-ux.md`）；主终端只做轻量入口 |

- [x] **S1 启动/重开：live instance 列表 + 一键附着**
  - **动机**：窗全关后用户不知道谁还活着、该 `--instance` 哪个；
    CLI 有 `server-list`/`list-instances`，GUI 启动路径几乎不消费
  - **做法**：
    1. GUI 冷启动或「无已附着 server」时：展示 live/stale 列表
       （label、pid、endpoint 摘要、version、window 是否仍在）；
    2. 选一项 → 附着该 endpoint/instance 并开窗（或聚焦已有窗）；
    3. 无 live 时诚实空状态 +「启动新 server / 本机默认 instance」；
    4. 复用现有注册表与解析器，不新造发现协议。
  - **验收**：杀掉所有 GUI 但保留 live server 后，再开 `agenterm` 能从
    列表点选回到同一 instance，且 `server-list` 与列表一致；stale 项
    不可当 live 附着或明确标 stale
  - **成本**：中；**依赖**：无（可读现有 instance 注册）
  - **非目标**：跨机器发现、托盘常驻（可另叶）

- [x] **S2 主窗身份常显**
  - **动机**：多开 main/work 时「我连的是谁」靠猜；标题 profile 噪音
    （§3.5 W1）与身份信息不足并存
  - **做法**：状态栏或标题稳定展示 `instance_label`（或 logical name）+
    可选短 pid；**发布构建**避免把内部 profile 噪音盖过身份
    （可与 W1 合并处理）
  - **验收**：两窗分挂 main/work 时，截图/ui-snapshot 可区分 instance，
    无需看 CLI
  - **成本**：小；**依赖**：无；可与 S1 并行
  - **非目标**：在状态栏做完整 server 切换器

- [x] **S3 主窗入口：打开另一 instance（新窗）**
  - **动机**：日常在多 server 间作业；横向 tab 热切过重，**新窗附着**
    符合现模型且安全
  - **做法**：工具栏/菜单 `Open instance…` → 同 S1 列表 → **新进程/新窗**
    附着所选 instance；可选「若该 instance 已有 GUI 则聚焦」
    （与 process 复用策略对齐，不第二权威）
  - **验收**：从 main 窗打开 work，得到第二窗且两窗 server 身份不同；
    不替换原窗的 lease/PTY
  - **成本**：中；**依赖**：S1 列表组件可复用
  - **非目标**：同窗热切换（见 S4）

- [ ] **S4 同窗热切换权威（后置；默认不进本版 must-ship）**
  - **动机**：若产品坚持「单窗内换 server」，须完整状态机，不是顶栏装饰
  - **做法（仅当拍板要做时）**：确认对话框 → detach 当前 UI lease 语义
    明确 → 换 endpoint → 新 bootstrap；composer/未决交互 fail-closed；
    **禁止**无确认的横向 server tab 作为唯一入口
  - **验收**：切换有确认；失败可回到原 context 或诚实断开；无 PTY 串台
  - **成本**：大；**依赖**：S1/S2；**默认推 v0.2.x** 除非用户改拍板
  - **明确不做（本版）**：主终端顶栏横向 server tab 作为默认导航
    （与 Fleet tab 混淆；误触贵）

> **砍叶**：S4 默认不排期；工期紧时 **S3 → S2 细节** 可砍，**S1 保留**
> （关窗后找得回是触发原话）。不与 R1/R2 抢优先级。
> **与 X3/L-CC**：CC 顶栏 current context / 未来 discovery 可消费同一
> 列表数据源；主终端 S1/S3 是轻量入口，不替代 CC。

### B′. tmux/rmux 兼容：`send-keys` + buffer 族（**用户 2026-08-05 排期**）

> **触发**：多 agent / 脚本要「先能发信息」时，用户提出应**先兼容**
> tmux/rmux 的 `send-keys` 与 `buffer-paste` / `buffer-copy` 一类命令，
> 以便沿既有自动化习惯往 pane 投递。
>
> **产品判断（已定）**：
> - **B′ 要做**：补齐控制面与 rmux 用户脚本能力；`send-keys` 已有但 buffer
>   族基本缺失；夯实后可脚本化「往 shell 打字 / 粘贴」。
> - **B′ 不替代 M 组发消息**：`send-keys` / `paste-buffer` 注入的是
>   **PTY 输入流**，不是 agent 收件箱——会打断 Codex/TUI、无回执、无已读。
>   **短协作消息 / 状态**仍走 **M1 note + M3 handoff**；需要对方 shell
>   执行时再桥到 B′。
> - **与 PRD**：`PRD_02_15` 已列 `send-keys`；buffer 族为兼容扩展，落地后
>   回写 PRD 命令表与「unsupported 显式失败」清单。

```text
B′. tmux/rmux send-keys + buffer
├─ [x] B1 盘点与契约：现有 send-keys 行为 / buffer 族 / save-buffer 显式 unsupported
├─ [x] B2 夯实 send-keys（-l 已有；usage 补 PS `@N` 引号）
├─ [x] B3 命名 buffer 最小集：set/load/show/list/delete-buffer
├─ [x] B4 paste-buffer（空 buffer 失败；UTF-8 规范化 + bracketed-paste；cli-smoke）
├─ [x] B5 与 M 的桥接文档：PRD_02_15 + paste-buffer help
└─ [ ] B6（可选）copy 路径：从 pane/选区 → buffer（tmux copy-mode 子集）
```

**硬约束**

| 约束 | 含义 |
|------|------|
| 一 tab 一 pane | 不假装多 pane；`-t` 解析与现有 window/tab 目标一致 |
| 显式不支持 | 做不到的 tmux 旗标/子命令 **typed fail**，不静默 ignore |
| 不进 GUI 焦点 | CLI buffer/send **默认不** `window-activate` / 抢前台 |
| 有界 | buffer 字节上限 + paste 有界；超限失败不截断装成成功 |
| 非 agent 邮箱 | 文档与验收禁止把 B′ 描述成「agent 消息总线」 |
| 权限不进 Rhai | 与 AGENTS.md 一致：不把「能否 send」做成 Script 授权沙箱 |

- [x] **B1 盘点与契约表**
  - **动机**：避免「以为兼容了全 tmux buffer」；先列 shipped / 本版做 /
    显式 unsupported
  - **落地**：`PRD_02_15` + `commands.rs` / `control_dispatch` 登记
    send-keys 与 buffer 族；`save-buffer|saveb` **typed unsupported**
    （mux 面 + CLI dispatch）；B6 copy-mode 仍非目标
  - **验收**：`list-commands` 含 buffer 族；`save-buffer` 显式 fail；
    单测 `save_buffer_is_explicitly_unsupported_on_mux_surface`
  - **成本**：极小；**依赖**：无

- [x] **B2 夯实 `send-keys`**
  - **动机**：命令已存在，但跨 agent 实测暴露：PowerShell `@N` 须引号、
    目标名截断、双 `main` 须 `--endpoint`；行为旗标需可脚本依赖
  - **落地**：`-l` / `--native` 与 usage（`@N` 引号提示）已在 CLI 帮助与
    dispatch；错误目标 typed fail
  - **验收**：公共或本地 smoke：`send-keys -t @N -l 'hello\n'` 后
    capture/inspect 可见
  - **成本**：小；**依赖**：B1

- [x] **B3 命名 buffer 最小集**
  - **动机**：rmux/tmux 脚本常用命名 buffer；AgenTerm 现无对等 API
  - **落地**（server 有界存储 + CLI）：`set-buffer` / `load-buffer` /
    `show-buffer` / `list-buffers` / `delete-buffer`（及短别名）
  - **验收**：set → show 字节一致；list 含 name/size；超上限失败；
    `cli-smoke` `cli.named-buffer-paste` 证据
  - **成本**：中；**依赖**：B1
  - **非目标**：跨 server 共享 buffer、持久化到磁盘 workspace（可后置）

- [x] **B4 `paste-buffer`**
  - **动机**：大段投递比多次 `send-keys` 稳；脚本「buffer 然后 paste 到 pane」
  - **落地**：`paste-buffer [-b name] [-t target]` → 目标 tab PTY；空 buffer /
    无目标 typed fail；UTF-8 走 `normalize_terminal_paste` + 应用
    bracketed-paste 成帧（与 GUI 剪贴板粘贴同源）
  - **验收**：`cli-smoke` set→paste→capture 见 `BUFFER_PROBE_OK_*`；
    空 buffer paste 失败
  - **成本**：中；**依赖**：B3
  - **非目标**：保证 Codex/TUI 语义正确（那是应用层；文档写明风险）

- [x] **B5 与 M 的桥接说明（文档叶）**
  - **动机**：避免实现者把 paste 当 handoff
  - **落地**：`PRD_02_15` B′ vs messaging 表 + `paste-buffer` usage 链到
    该表；**非** agent 邮箱
  - **验收**：帮助/PRD 可发现
  - **成本**：极小；**依赖**：无

- [ ] **B6（可选）copy → buffer**
  - **动机**：tmux `copy-mode` / 选区进 buffer；完整 copy-mode 大
  - **做法（本版若做则最小）**：从 **已有** GUI 选区或
    `capture-pane` 结果写入命名 buffer（`set-buffer` 包装），
    **不做**全套 copy-mode 状态机
  - **验收**：一条路径：有选区或 capture → buffer → show-buffer 一致
  - **成本**：中；**依赖**：B3；**工期紧砍**

> **砍叶**：B6 → B5 可并入文档 PR；**B1–B4 为本组核心**。
> 与 R 正交；**不**用 B′ 挤 R1/R2。与 **M** 并行时文件面：
> B′ 偏 `control_dispatch` / CLI / server buffer；M 偏 note/handoff 约定与
> observe——避免同 PR 混「邮箱」与「键入」。

### H′. 分发面地基（只做**纯派生 + 补值**，不建服务；详见 §1 H 组）

- [x] **H4 补齐 Linux/Windows 的 `provenance.sbom_sha256`** ★先做
  - **动机（已修正，见 §2.7）**：实测六平台——**macOS 两个 arch 已正确填入**
    `65c32add…`，**Linux 两个为空串、Windows x86_64 缺字段、
    windows-aarch64 为空串**。`PRD_02_17:237-240` 只要求「macOS 双档
    provenance 携带同一 SBOM 摘要」，故**当前实现并未违反 PRD**；
    本叶是**把该保证扩展到全部六平台**，因为 M14 Hub 信任分级要对
    所有平台复用这个字段
  - **落地**：`package-client-release.rhai` 全平台写 `sbom_sha256`；
    `package-release-qualified.rh` Windows 资产补字段。下一次 Candidate
    六平台 provenance 应非空（待打包门验证）。
  - **验收**：新 Candidate 六平台 provenance 的 `sbom_sha256` 均 ==
    实际 SBOM 摘要（windows-x86_64 从「无此字段」变为有值）
  - **成本**：极小（纯补值）；**依赖**：无
  - **PRD 联动**：落地后应同步把 `PRD_02_17:237-240` 的 macOS 限定
    升级为六平台（见 §2.7 建议 2）
- [x] **H1 生成 `releases.json`**（CI 静态产物，纯派生）
  - **动机**：install.sh 靠字符串拼 artifact 名 + latest 重定向猜版本；
    未来 update/下载页/Hub 会各自再 scrape 一遍 → 四个真相源
  - **落地**：`scripts/rh/build-releases-index.rh` 从 sealed candidate
    派生 schema-v1 `agenterm-releases-index`；`release.yml` verify 写出
    `candidate/releases.json`（dry_run 可见），publish 作为 **GitHub
    Release asset** 上传（**不进** sealed payload 文件集）。Pages /
    agenterm.work 多版本索引托管仍属 P5/H5，**不阻塞**本叶验收。
  - **验收**：`releases_index_is_pure_derived_from_sealed_candidate` +
    workflow 含 `build-releases-index.rh` + `candidate/releases.json`；
    字段全部来自 provenance/candidate-manifest（**不新造事实**）
  - **成本**：中；**依赖**：H4（sbom 摘要写进索引字段）
- [x] **H3 provenance 用户可见化 + `installed.json`**
  - **动机**：`.provenance.json` 每包都发但用户端零消费（install.sh 只校 sha256）
  - **落地**：`install.sh` 下载 `.provenance.json`，校验
    sha256/version/source_tag/artifact 与实测一致，打印
    commit/tag/signed/notarized/sbom/build_log，写入
    `installed.json.provenance` 并落盘 `agenterm.provenance.json`
  - **验收**：远程 install 路径失败关闭（缺 provenance / 摘要不一致）；
    成功路径用户可见 supply-chain 摘要
  - **成本**：中；**依赖**：G3（共用 installed.json）；variant 仍可由
    本机 OS/ARCH + unsigned-preview 旗标解析（H2 再改读索引）


### N. 新功能（**本版唯一的"往前走"叶**；其余全是修补与降本）

> 自查发现的问题：R/A′/G′/H′ 共 13 叶**全部是修补、降本或地基**，
> 没有一片是新开工的功能——那是把 v0.1.14 的账还完，不是往前走。
> 本组补上一叶，且刻意只补一叶（v0.1.14 教训：宁可少而全绿）。

- [ ] **N1 补齐 macOS/Linux 的 `ImeStatus`**（兑现 platform facade 的封装承诺）
  - **动机（封装失衡的实证）**：`contract/ime.rs` 定义了完整的 `ImeStatus`
    （name / available / open / native_mode / full_shape）并配 4 个单测，
    但**只有 Windows 实现了**（`adapters/windows/ime.rs` 286 行）；
    **macOS 与 Linux 各 30 行 stub，`status()` 一律 `return None`**。
    后果：状态栏的中/英指示、输入法名称在 Unix 侧**永远显示 `IME: off`**，
    契约形同虚设。**这正是"封装"应当消除的平台失衡**。
  - **已实测可行（2026-08-05 本机 macOS 26.5 验证）**：
    ```c
    TISCopyCurrentKeyboardInputSource()
      → kTISPropertyInputSourceID   = "com.tencent.inputmethod.wetype.pinyin"
      → kTISPropertyLocalizedName   = "微信输入法"
      → kTISPropertyInputSourceType = "TISTypeKeyboardInputMode"
      → kTISPropertyInputSourceLanguages[0] = "zh-Hans"
    ```
    即 `name` / `available` / `native_mode` **三个字段可如实填充**
    （native_mode 由 input-mode 类别 + 语言标签推导）。
  - **诚实的能力边界（不猜、不假装）**：macOS **无公开 API** 可读
    `open`（IME 是否处于合成态）与 `full_shape`（全角半角）——
    二者是 Windows IMM 的概念。按 `contract/ime.rs` 自身的规定
    「hosts that cannot report a given field leave it empty rather than
    guessing」，这两个字段在 macOS 保持默认值，**不伪造**。
  - **做法**：
    - macOS：新增 Carbon/HIToolbox 绑定（`TISCopyCurrentKeyboardInputSource`
      + 三个属性读取），落在 `adapters/macos/ime.rs`；
      注意 `objc2-app-kit` 已是依赖，但 TIS 属 Carbon framework，需另加链接
    - Linux：读 XKB 布局组／或探测 fcitx5/ibus 的 DBus 接口（二选一，
      先做能力探测再定；探测不到则维持 `available: false`，不 panic）
  - **验收（可证伪）**：
    - macOS 真机切到中文输入法时 `ImeStatus.label()` 返回
      `IME: 微信输入法 · native`；切回 ABC 返回 `IME: … · latin`
      （**本机可直接验证**，不像 X2 那样悬着）
    - `open`/`full_shape` 在 macOS 保持 false 且**有注释说明为何不可得**
    - 新增单测覆盖「能报的字段照实报、不能报的字段不猜」
    - Linux 无 IME 环境下不 panic、`available: false`
  - **成本**：中（macOS 部分小；Linux 部分取决于走 XKB 还是 DBus）
  - **依赖**：无
  - **与 Windows agent 的分工（不冲突）**：他改的是 Windows **合成输入路径**
    （WM_IME_* → 内联 preedit，见 §3.5 3.5.3 I1）；本叶补的是
    **Unix 侧的状态读取**。两者在 facade 的不同侧，互不触碰对方文件。
  - **若工期紧**：可只做 macOS 档（Linux 留 stub 并注明），仍然兑现
    「三平台平权」的一半，且我方能真机验证
  - **派工**：macOS 半叶由 **§1 O / §11 O1** 本机 agent 执行；Linux 半叶另派

### M. 多 Agent 观察与交接（**Fleet 控制面地基**；2026-08-05 实测驱动）

> **不是**「用 PTY 当总线互相敲终端」；**是**把已经点亮的
> `agenterm-cli` 旁路观察做成可复用契约，并把真正的 handoff 放在
> **控制面权威**（tab note / workspace 文件 / 事件·receipt），为后续
> Control Center 投影与跨 server 协作铺路。
>
> **触发与实证（2026-08-05，本机 live Fleet）**：
> - Grok 会话经 `agenterm-cli --instance main` 成功读到 tab `@2`
>   标题 `ds4@c`（显示截断，语义为 `ds4@codex`）的视口：对方在
>   `deepseek-v4-auto` / `D:\dev\moltbaby` 下对 `agenterm` 做 `/review`。
> - **顺利**：`list-instances` → `list-windows` → `inspect -t '@2'` /
>   `capture-pane -p` 一次成型；不抢焦点。
> - **不顺**：PowerShell 未加引号时 `@2` 被当 splat；标题截断无稳定
>   agent 身份；内容是 TUI 投影非 structured transcript；`scroll-pane`
>   会动对方视口（观察与干扰同线）。
> - **架构判断**：跨 instance **只读观察**可包装成顺利路径；跨 server
>   **通讯**若继续靠 capture/send-keys **不会顺利**——须 handoff 契约。

**硬约束（本组全部叶）**

| 约束 | 含义 |
|------|------|
| 单一 Fleet 权威 | handoff 不另立第二 workspace/PTY 权威；事实仍来自 server |
| 观察默认无副作用 | 只读路径不得隐式 `select-window` / focus / scroll / send-keys |
| PTY ≠ 总线 | 禁止把对方 agent 的终端输入流当作可靠消息通道 |
| 有界与诚实 | 视口/字节有上限；读不到就 typed failure，不编造 transcript |
| 本版不绑 L-CC 实现 | CC 投影可登记指针；UI 成熟仍归 v0.2.0 / X3 |

```text
M. 多 Agent 观察与交接
├─ [ ] M1 稳定 agent 身份（命名 + note 约定）
├─ [ ] M2 只读 observe 面（CLI 契约 + 无副作用）
├─ [ ] M3 handoff 契约（控制面，非 PTY）
└─ [ ] M4 跨 instance 观察证据（黑盒夹具）
```

- [ ] **M1 稳定 agent 身份**
  - **动机**：live 标题显示为 `ds4@c` 而非完整 `ds4@codex`；仅靠窗口名
    无法做跨 agent 寻址。`new-agent -n`、rename、tab-note 已存在，但
    **缺产品约定 + 不被截断的身份字段**。
  - **做法（约定优先，代码最小）**：
    1. 文档约定：crew tab 名 = 稳定 slug（如 `ds4@codex`），禁止仅靠
       显示截断猜测；
    2. `set-tab-note` / `show-tab-note` 约定一行可解析前缀
       （例：`agent-id=ds4@codex;role=impl|review;status=…`）或
       短 JSON（有长度上限时截断 status 不截断 id）；
    3. 若 inspect/list 格式串已截断 `window_name`，**另暴露完整 name
       字段**（inspect JSON 已有 `name`——以 JSON/`-F` 全字段为准，
       文档写清「UI 芯片可截断，API 不得丢 id」）。
  - **验收**：同 instance 内两个 crew tab 可用完整 id 互相定位；
    `inspect` JSON 的 `name` 与 note 中 `agent-id` 一致可核对；
    文档给出 `new-agent -n` + `set-tab-note` 最小样例。
  - **成本**：小；**依赖**：无
  - **非目标**：全局目录服务、跨用户认证

- [ ] **M2 只读 observe 面**
  - **动机**：今天「能读」依赖操作者记得 PS 引号、记得别 scroll、
    自己拼 `list-instances` 过滤。要变成 **agent 默认可依赖的观察 API**。
  - **做法**：
    1. **文档 + 推荐调用序列**（本叶可先交付）：
       `server-list|list-instances` → `--instance|--endpoint` →
       `list-windows` → `inspect|capture-pane`（目标一律稳定 `@N` 或
       经 M1 解析出的 id）；
    2. **Shell 陷阱**：PowerShell / bash 示例统一 ` -t '@2'`；
    3. **无副作用声明**：observe 包装**禁止**调用
       `select-window` / `scroll-pane` / `send-keys` / `focus` /
       `ui-action window-activate`；需要更多历史时用
       `capture-pane` 有界字节或未来 journal，不滚动对方视口；
    4. **输出契约**：优先 `inspect` JSON 与 `capture-pane --json`
       （若已有）+ `--max-bytes`；UTF-8；失败 typed。
    5. 可选极薄 CLI 子命令（工期允许）：
       `agenterm-cli observe tab --instance X --tab @N|--name slug`
       只读聚合（identity + note + viewport snippet + dead 标志）。
  - **验收**：另一 agent（或脚本）在不激活对方窗口的前提下读到
    viewport 摘要；故意省略引号的失败有文档说明；包装路径下
    对方 `input_writes` / 视口 scroll 偏移不被 observe 改变
    （黑盒：observe 前后 inspect 对比）。
  - **成本**：小–中（仅文档+约定为小；新增 `observe` 子命令为中）
  - **依赖**：M1（身份）；可先 M2 文档、后补 M1 字段
  - **非目标**：完整 Codex/Claude transcript 导出；替换对方 harness 日志

- [ ] **M3 handoff 契约（控制面，非 PTY）**
  - **动机**：观察地基已亮；**通讯**不能靠互相 `send-keys`。
    需要「A 写意图 → B 可靠读到 → 可选回执」且不打断对方 TUI。
  - **做法（v0.1.15 最小切片）**：
    1. **Handoff 载体二选一（可并存）**：
       - **Tab note**：短状态/指针（受 `UI_TAB_NOTE_MAX_BYTES` 约束）；
       - **Workspace 旁路文件**：如
         `{workspace_dir}/handoff/{agent-id}.json` 或 repo 内约定路径
         （agent 协作时用 git-visible 路径更利于审计）；
    2. **最小 JSON schema**（字段可增不可偷换语义）：
       `schema`, `from`, `to`, `kind`（`status|request|result`）,
       `summary`, `refs[]`（path/PR/commit）, `updated_at`,
       可选 `receipt_id`；
    3. **写路径**：仅 `set-tab-note` / 文件写 + 既有 Fleet 突变 receipt
       （若走 note）；**禁止**把 handoff body 注入对方 PTY；
    4. **读路径**：`show-tab-note` / 读文件 + M2 observe 可附带 note；
    5. **文档**：一页「多 agent 礼仪」——谁可写谁的 note、冲突时
       last-writer-wins 或显式 generation 字段。
  - **验收**：Agent A 写入 handoff 后，Agent B **只读 API** 取到同一
    `summary`/`refs`，全程无 `send-keys` 到对方 tab；note 超长失败
    诚实（不截断到语义损坏而不报错——至少 id/kind 完整或整体失败）。
  - **成本**：中；**依赖**：M1；与 M2 可并行文档，实现上 M3 可后于 M2
  - **非目标**：分布式共识、跨机器加密频道、Agent 权限/审批引擎
    （仍属未来 harness，不进 Script Runtime）
  - **与 L-CC**：handoff 事实可被未来 Cockpit/Diagnostics **投影**；
    本叶不要求 `agenterm-cc` 新 UI（见 X3 / `plan/design-control-center-ux.md`）

- [ ] **M4 跨 instance 观察证据**
  - **动机**：跨 server「观察」产品上说得通（`--instance` /
    `--endpoint` 已存在），但缺**黑盒证明**「同一用户域两个 live
    instance 上，observer 读 peer 不串台、不踩对方」。
  - **做法**：Rhai/脚本黑盒：起（或选用）两个隔离 instance/endpoint，
    各一 tab；从 CLI 分别 list + inspect；断言 name/note/viewport
    归属正确；可选：一端写 M3 handoff 文件，另一端只读看到。
  - **验收**：公共或本地 smoke 一条路径全绿；失败码区分
    `instance_not_found` / `tab_not_found` / `observe_bounded`；
    **不**要求跨机器网络、不要求第二台物理 host。
  - **成本**：中；**依赖**：M2（及若测 handoff 则 M3）
  - **若工期紧**：可降为「文档写明跨 instance 调用序列 + 手工收据」
    而不进 CI，但须在本叶注明证据等级

> **规模与砍叶**：M 组是用户 2026-08-05 追加的「agents 互动地基」，
> **不是**发布链主题的核心，但比 L-NET 更贴「每天多 agent 同屏」的
> 实测痛点。工期紧时砍叶顺序（在原有 H1/H3 → R4 → N1 Linux 之后）：
> **M4 → M3 → M2 子命令形态**；保留 M1 约定 + M2 文档作为最小切片。
> **绝不**为 M 组去砍 R1/R2。

### X. 已在途/已落地（**并发 agent 泳道**——非本次规划产出，登记以免范围失真）

定稿时（2026-08-05）本工作区尚未看到；`fe51c7c` 合并后补记。
**这些不是我排的叶，但它们确实占用 v0.1.15 的工期与风险预算**，
因此必须登记——否则「13 叶」的规模自查会低估实际范围。

- [x] **X1 内置皮肤 v1（四预设）** — 已落地 `e30689c`/`3cd346b`
  - 内容：`AppearancePreset`（classic/fancy × day/night）+ settings 迁移
    （legacy `color_theme` → `appearance_preset`）+ `assets/skins/**`
    manifest/palette/icon + Win/Unix 选择器 + 窗口标题/图标
  - 规模：约 1600 行（`src/theme.rs` +685、`src/settings.rs` +116、
    `assets/skins/` 新增 11 文件）
  - 证据：`theme-smoke.rhai` 21 处 preset 断言；契约在
    `prd/PRD_02_06_human_workspace.md` §Built-in skins (v1)
  - 执行计划：[`plan/archive/plan-skins-v1.md`](archive/plan-skins-v1.md)
  - **与 §5 5.4 L-EXT 的关系**：这是**内置**皮肤，外部 SkinHub 包仍归
    M14／v0.2.x——即 P6（Hub 单一 kind 底座）**未被本次落地预判**
- [x] **X2 Windows IME 内联合成 + 协议兼容 UX** — 已落地 `83843ea`
  - 内容：见 §3.5 3.5.3（I1 候选条锚点／I2 ui-hello 版本分类 + 原生
    MessageBox，新增 platform `alert` 能力）
  - 证据：607 lib tests 绿 + `incompatible_ui_contract_names_the_stale_side`
  - 待办：两项均**待真机回归**（中文输入、MessageBox 路径）
- [ ] **X3 Control Center UX 设计** — 设计定稿 rev3，**实现归 v0.2.0**
  - 任务书：[`plan/plan-control-center-ux.md`](plan-control-center-ux.md)
  - 实现级 SSOT：[`plan/design-control-center-ux.md`](design-control-center-ux.md)
  - 不占本版工程工期；本条仅登记指针。与 **M 组**关系：M 的 handoff/
    observe 事实可被未来 Cockpit 投影，但 M **不依赖** CC UI 先落地

### L′. 从 v0.1.14 迁入的未完成叶（**2026-08-06 upsert**；发版后归档）

> 来源：`plan/archive/plan-v0.1.14.md` §1 目标树仍为 `[ ]` 的项。  
> v0.1.14 **已发布**（tag `8ff2b5a`）；未完成项**不是**发布阻塞，而是信任/卫生尾账。  
> 与 §1 C 组、§1.5 其它泳道重叠的，只保留指针，避免双排期。

```text
L′. v0.1.14 carry-forward
├─ L1 身份真机回归（原 A 半叶）
├─ L2 precision-audit item 22（HashSet 上限）
├─ L3 precision-audit item 16（无 HOME 时 /tmp 共享）
├─ L4 CC 矮窗 tab 条折叠（原 C）
├─ L5 control-center-smoke 进 CI 评估（原 C）
├─ L6 stale 注册记录体验（原 C；S′ strip 已部分缓解）
├─ L7 多文件改动前置 cargo fmt 清单化（原 D）
└─ L8 flaky child_wait → **并入 §1 C1**（不另开叶）
```

- [ ] **L1 身份真机回归**
  - **来源**：plan-v0.1.14 A「`--instance custom:work` → server-list 显示 `<user>_work`」
  - **现状**：代码侧 autostart 身份修复已 `[x]`；S′ strip / multi-instance 已大幅覆盖
  - **验收**：干净二进制下 `agenterm --instance custom:work` + `server-list` INSTANCE 列为
    用户 scope 的 work 标签（非误标 main）
  - **成本**：极小（真机/隔离黑盒）；**依赖**：无
- [ ] **L2 precision-audit item 22**
  - **来源**：script_protocol / agenterm-rhai 三个 dedup `HashSet` 在 persistent worker
    中只增不减
  - **验收**：人工拍板上限/淘汰策略后落地；回填 `plan/precision-audit.md`
  - **成本**：中（需拍板）；**依赖**：策略拍板
- [ ] **L3 precision-audit item 16 剩余**
  - **来源**：Linux/macOS 无 HOME/XDG 时 instances 目录静默退到共享 `/tmp`，未做
    符号链接/祖先加固
  - **验收**：决定是否复用 `protect_private_directory` / `metadata_is_real_directory`；
    有/无 HOME 路径均有黑盒或文档边界
  - **成本**：中；**依赖**：策略拍板
- [ ] **L4 CC 矮窗（~480px）tab 条折叠**
  - **来源**：plan-v0.1.13/14 — 三行 tab 条仅首行在 client 内
  - **验收**：矮窗下导航可用（折叠/滚动/提前 strip）；Win smoke 有界
  - **成本**：中；**依赖**：无；与 **X3 / L-CC** 对齐时优先做
- [ ] **L5 control-center-smoke 进 CI 矩阵评估**
  - **来源**：当前不在矩阵，同源缺口无门禁
  - **验收**：书面评估（进/不进 + 墙钟预算）；若进则 push 或 release 车道声明一致
  - **成本**：小–中；**依赖**：无；可挂 **A′** 反馈左移
- [ ] **L6 stale 注册记录体验**
  - **来源**：server-list 长期 stale 行；评估 cleanup 自动化或提示
  - **现状**：S′ strip 对 stale 芯片可点「开新窗」；list 体验仍可能脏
  - **验收**：`server-list`/`server-cleanup` 或明示引导；不误杀 live
  - **成本**：小–中；**依赖**：无
- [ ] **L7 多文件改动前置 `cargo fmt --check` 清单化**
  - **来源**：v0.1.14 占位稿两次 rustfmt fail-closed 教训
  - **验收**：agent/dev 清单或 lint 入口明确「改多文件先 fmt」；可与 `lint.cmd` 对齐
  - **成本**：极小；**依赖**：无
- **L8** → **§1 C1**（`child_wait_timeout_reaps_descendants` flaky 根因复核）。  
  不再单开叶；C1 勾选即关闭 0.1.14 D 尾账。

**L′ 砍叶顺序（工期紧）**：L7 → L1 → L5 → L6 → L4 → L2/L3（后两者要拍板）。

> **规模影响**：L′ 最多 7 个可执行叶（+1 指针），其中多数为小/中修补；
> **不**挤掉 R1/R2。C1 与 L8 合并后竞态组不增叶。

> **规模影响**：X1+X2 已消耗的工期不小（约 2900 行入 main）。若把它们计入，
> v0.1.15 实际范围已**超过**我在 §2 主张的「窄」。这不改变 R 组的排序理由
> （cache 仍是最便宜的杠杆），但**应当据此更保守地对待 H1/H3**——
> 见 §2.2 序 7 的「工期紧则优先砍」已预留该出口。
> 2026-08-05 再补 **M 组 4 叶**（多 agent 观察/交接）与 **N1**：宽度继续
> 上升，砍叶出口见上表与 M 组附注。
> 2026-08-06 再补 **L′**（v0.1.14 未完成 upsert）+ **S′ 真机迭代已大部落地**。

**规模自查（2026-08-06，含 L′ / S′ 落地）**：

| 泳道 | 叶数 | 性质 | 状态 |
|------|-----|------|------|
| R / A′ / G′ / H′ | 13 | 修补 · 降本 · 地基 | 待授权开工 |
| **U′** | **4**（U1 代码已落） | **UX 假刷新止血** | U1 待合；U2–U4 待授权 |
| **S′** | **4**（S1–S4 形态已落地） | **多 instance / 顶栏 strip** | 真机迭代完成；边角可回 L6 |
| **B′** | **6**（B6 可选） | **tmux send-keys + buffer 兼容** | 工作区曾落地；待持续证据 |
| **L′** | **7+1** | **v0.1.14 尾账** | 2026-08-06 迁入；L8→C1 |
| **N** | **1** | **新功能（平台 facade）** | 待授权开工 |
| **M** | **4** | **新功能（多 agent 控制面地基）** | 待授权开工 |
| X（并发 agent） | 2 已完成 + 1 设计归 v0.2.0 | 功能 / 设计 | 约 2900 行已入 main |

对照 v0.1.14 的教训（发布日 5–6 小时耗在从未跑过的车道上），规划叶宽度
已明显大于「13 叶窄版」。**R 组仍第一优先**。工期吃紧时砍叶顺序：
**H1/H3 → R4 → S4/U4/B6 → U3/S3 → M4 → M3 → N1 Linux → M2 子命令**；
保留 **R1/R2、U1/U2、S1、B1–B4、M1 约定 + M2 文档**；**绝不砍 R1/R2**。

### 1.5.1 为什么 v0.1.15 不推进 L-NET（ipfs/libp2p）

用户 2026-08-05 原话：本想督促 ipfs/libp2p 功能，但认同「先把底子弄好」——
多平台 UI/UX 对齐、稳定性增强、功能补丁优先。这个判断与实测证据一致：

- **L-NET 的下一关不是写代码，是定形态**。§5.2.1 实查表明 research spike
  已自证完备（进程隔离／CID／block store 全绿，每轮 release 门真跑 142s），
  但 `src/` **零 import**——卡点是 N3「产品消费者以什么形态存在」
  （Script API？InfoHub？CC 诊断？），那是**拍板题不是工程题**。
  在形态未定前投工程，做出来的接口大概率要返工。
- **底子确实欠账**：N1 揭示 `ImeStatus` 契约只有 Windows 实现、Unix 两档全是
  stub；§8 实测的安装/升级体验有 G2/G3/G6/G7a 四处硬伤；cache 撞顶正在
  单调恶化。这些都是**用户每天碰得到**的，而 L-NET 目前无人使用。
- **结论**：v0.1.15 做底子，L-NET 保持 research 车道（R3 只是把它从 release
  门移到 push CI，**验证不减**）。待 N3 形态拍板后，L-NET 作为 v0.2.0 主线开工。

## 2. 排序与理由（**基于实测数字，非直觉**）

### 2.1 为什么主题从「反馈左移」改为「发布链降本（cache 优先）」

占位稿把 A1（夜间彩排）排第一，理由是「腐化攒到发布日爆雷」。这个判断
**方向对但排序错**，因为当时还没有 §7 的逐门/cache 实测。三点修正：

1. **A1 成本远高于收益密度**。夜间 release-stress 每晚 ~1 runner-hour，
   且 win-full-gate 的 concurrency group 是 `win-full-gate-{ref}` +
   `cancel-in-progress: true`——同 ref 连跑会互相 cancel，落地前还得先改
   并发语义（§1 A1 已核）。**投入是本版最大的一项，收益是概率性的**。
2. **R1（cache）投入最小、收益确定且可证伪**。9.9/10GB 撞顶是**已复验的
   常态**，bootstrap 47s→81s 是**单调恶化**的实测曲线。改 cache 配置属
   配置级改动，收益 ≈3min/次 Candidate，按 v0.1.14 的 6 次 Candidate 计
   约省 18min ——**且它同时止住恶化趋势**，不做的话下一版更贵。
3. **A3/A4 才是「反馈左移」里真正便宜的部分**。A3 实测 ~7s、A4 是纯输出改动，
   两者合计成本远低于 A1，却覆盖了「腐化早暴露」的主要价值。

> 结论：反馈左移的**思想**保留（A3/A4 + R4 dry-run 都是它的实现），
> 但**最贵的实现方式（A1 夜间彩排）推迟**。主题相应改为「发布链降本」。

### 2.2 执行顺序（建议）

| 序 | 叶 | 理由 |
|----|-----|------|
| 1 | **R1 → R2** | 最便宜、收益确定、且在恶化；R2 依赖 R1 腾出的配额 |
| 2 | **A4** | 让 R1 的收益可直接在运行页读出（验收工具先于验收对象） |
| 3 | **R3、A3** | 各自独立、成本小，可并行 |
| 4 | **H4** | 纯补值、零依赖，且是 H1 的前置 |
| 5 | **G3 → G7a、G2、G6** | 安装卫生泳道，与发布链正交，可与 1–4 并行 |
| 6 | **R4** | 中等成本，且**自身就是没跑过的车道**，放在链路稳定后做 |
| 7 | **H1 → H3** | 本版最大的两叶；若工期紧，这两叶优先砍 |
| 8 | **U1 合入 → U2 真机回归** | 标签假刷新止血；用户现场痛点，与 R 正交、宜早合 |
| 9 | **S1 → S2** | 关窗后找得回 + 身份常显；对症「窗没了不知道连谁」 |
| 10 | **B1 → B2 → B3 → B4** | tmux send-keys 夯实 + buffer 最小集 + paste；脚本投递 pane |
| 11 | **B5 + M1/M2 文档** | 划清 note/handoff vs send/paste；可与 B2 同 PR |
| 12 | **S3** | 新窗打开另一 instance；复用 S1 列表 |
| 13 | **M3 → M4 / U3 / N1** | 后置并行池；S4/U4/B6 默认可砍或 v0.2.x |
| **OSX 机** | **O 泳道 + G′ 真机**（详见 §2.2.1 / §11） | 不与 Lnx 同时改 `unix/frontend` 巨石；O6 已关则接 O1b / G |

### 2.2.1 三端并发泳道派工（2026-08-06；Win / OSX / Lnx）

> **先编排、再并发。** 不要三台 agent 同时「对齐 Win 工作台手感」。  
> 主轴仍是 **R/G/A**（发布与安装）；GUI 三端只做 **有文件域、有 catalog 纪律** 的叶。  
> 叶定义仍以 **§1.5** 为准；本节只回答 **谁做、碰哪、禁哪、怎么验收**。

#### 2.2.1.1 总原则

| # | 规则 |
|---|------|
| 1 | **一人一热域**：同一时刻同一热文件只一个 agent 写（见下表）。改前 `git pull --ff-only`；pathspec 精确提交；禁止 `git add -A`。 |
| 2 | **shared-first**：新产品手势先进 `src/frontend/*` / `control_dispatch` / `ui_action_catalog`；单端必须 catalog `WINDOWS_ONLY`/`UNIX_ONLY` + `parity-gap:`。禁止「Win 先做完再让 OSX/Lnx 抄 match」。 |
| 3 | **机制进 crate**：OS 能力只改 `crates/agenterm-platform`；产品层不硬编码 `ERROR_ACCESS_DENIED` / 散装 native（boundary + breakaway 闸）。 |
| 4 | **unix/frontend 单写者**：OSX 与 Lnx **不得**同时改 `src/platform/adapters/unix/frontend/**`。默认 **Unix 产品主责 = OSX 机 agent**；Lnx 做复验 / 环境 / Linux-only adapter。 |
| 5 | **R/A 泳道独占**：workflow / cache / qualification 改动 **一人** 串行做完 R1→R2→A4…，另端不抢 `.github/workflows/*`。 |
| 6 | **S 结构 HOLD**：`plan/ARCHITECTURE.md` 大重构 / boundary 扩围栏等用户通知 + §9 复审后再开。 |
| 7 | **推送**：小步 `main`；冲突热文件让写者 rebase；观察 Actions 遵守 `AGENTS.md`（单 observer、退避，勿多 agent 狂刷 API）。 |

#### 2.2.1.2 泳道表（可直接派工）

| 泳道 | 主机 / agent | 叶（优先序） | 文件域（可写） | 禁区 / 备注 |
|------|--------------|--------------|---------------|-------------|
| **CI-R** | 任意一台，**独占一人** | §2.2 序 1–3、6：**R1→R2→A4→R3/A3→R4** | `.github/workflows/*`、`scripts/rh/check*.rh`、qualification / cache 相关声明 | 不碰 GUI 巨石；R4 自身是新车道须 dry-run 自证 |
| **G-install** | **优先 OSX**（§8真机）；Lnx 可选复验 | **G3→G7a→G2→G6**（与 R 正交并行） | `install.sh`、version / `installed.json` 写出路径、相关 docs | **不**改 keep-server 默认（G7b/c/d 等 G-P2）；G7a 纯文案可先做 |
| **Win-UX** | **Windows agent** | **U2** 真机回归；**P0-3** breakaway 若仍欠；用户现场 Win-only 痛点；B′/M 文档若排期到 | `windows/remote_frontend*` 最小 diff；已 SHARED 的只改 present | 新 ui-action 先 catalog；strip/picker 深度仍 Win-first，Unix 不默默假实现 |
| **Unix-UX** | **OSX agent 主责**（单写 `unix/frontend`） | **O1b** 状态栏 IME（已拍板）；§11余叶；对 **SHARED** 的诚实接线 / 真机 | `unix/frontend/**`、`adapters/macos/**`、共享 `frontend/*` 仅当语义真共享 | **禁止**复刻 server-strip 全量当本版必做；读 `ui_action_catalog` WINDOWS_ONLY 的 `parity-gap` |
| **Lnx-env** | **Linux agent** | **F1/F2** 环境快照（可不进 PR）；Linux adapter / smoke 复验；**不**开第二套产品策略 | 云桌面依赖、DPI、`adapters/linux/**`、CI 复现笔记 | **不写** `unix/frontend/mod.rs` 巨石除非 Unix-UX 交接写权；Wine 不能替真机 ConPTY |
| **S-struct** | — | **HOLD** | — | 用户通知后再开 §9 |

#### 2.2.1.3 推荐并发波形（2–3 条即可）

```text
时间 →
  CI-R:     [R1][R2][A4][R3|A3]……[R4]
  G-install:[==== G3 → G7a → G2 → G6 ====]     （OSX 真机）
  Win-UX:   [U2 真机][P0-3?][现场叶]
  Unix-UX:  [O1b][O 余叶 / SHARED 接线真机]
  Lnx-env:  [F1/F2][Linux smoke 复验]          （不与 Unix-UX 抢 frontend）
```

- **H4 / H1 / H3** 仍按 §2.2 序 4、7：CI-R 或 Win 独占，**勿**与 R1 抢同一 workflow 文件同时写。  
- **S′ S1–S4** 已落地：本派工表不重开；Unix strip/picker 属 **parity 产品叶**，非 CI 阻塞，默认不进本版强制三端齐。  
- **U1** 代码已落：Win-UX 只做 **U2 真机**；U3/U4 工期紧可砍（§1.5）。

#### 2.2.1.4 接手 agent 开工检查单（每台复制）

1. `git pull --ff-only origin main`；读 **§1.5 自己泳道的叶** + 本表禁区。  
2. 读 `AGENTS.md`（Platform crate vs product UI / shared-first）与 `plan/plan-platform-encapsulation-gap.md`。  
3. 声明本回合 **pathspec 热区**（聊天或 PR 描述一行）；与他泳道冲突则让路。  
4. 验证：本叶写明的验收；GUI 叶须真机或黑盒，不只 `cargo check`。  
5. 小步 commit + push；不扩 scope 到 HOLD / 推迟表（§2.6）。

#### 2.2.1.5 与 §11（OSX 作业规格）的关系

- §11 = **macOS 本机上下文与 O 组细节**。  
- 本小节 = **三端怎么并行、谁不抢谁**。  
- OSX agent：§11 工序 + 本表 **G-install + Unix-UX**。  
- Lnx agent：本表 **Lnx-env**（+ 若 Unix-UX 明确交权才动 frontend）。  
- Win agent：本表 **Win-UX + 可选 CI-R（若未另派）**。

### 2.2.2 OSX / Lnx 接手清单（Win CI-R 主波之后）

> **角色**：跑测试套件并修失败；**不要**重做 R1–R3/A3/A4 workflow 改动。  
> **先** `git pull --ff-only origin main`，读 §2.2.1 禁区。

| 主机 | 必做 | 可选 / 勿做 |
|------|------|-------------|
| **OSX** | `./check.sh --quick`（或本机惯用 quick）；有 GUI 时再跑相关 smoke；**G3→G7a→G2→G6** install 真机（§8）；**O1b** 若仍开 | 勿改 `.github/workflows` cache 键除非修你引入的红；勿整页重写 server-strip |
| **Lnx** | `./check.sh --quick` + 本机/CI 已有 linux smokes 复验；**F1/F2** 环境 | 勿与 OSX 同时写 `unix/frontend/mod.rs`；Wine 不替 ConPTY 真机 |
| **两端** | 失败先归因是否 pre-existing；修自己引入的红；精确 pathspec 提交 | 不宣称 R1 Candidate `worker.state=reused` 除非你观测了 Actions |

**Win 已交棒的 CI-R 证据指针**：`ci.yml`（v3-slim target + script-smoke）、  
`candidate.yml`（cargo-home restore-keys + timing/bootstrap summary）、  
`scripts/rh/check.rh` + `qualification-gates.json`（net-research 出 release 门）。

### 2.3 明确不做速度优化的部分

- **gate 分片**（39 门串行 869s，理论可压到 7–9min）：收益最大，但要重排
  windows job 结构，属结构性改动，**推 v0.2.x**——本版不碰关键路径结构。
- **artifact-build / artifact-build-fast 合并**（合计 339s / 39%）：
  已核 release-fast = release + lto=false + codegen-units=16 + incremental，
  产物不可互换；真省法是共享增量缓存，而那正是 R1 的副产品——
  **先做 R1 再测命中率**，不单独立叶。
- **smoke 并行分片**（原 D2）：14 门合计仅 124.4s，现值低，维持不做。

## 2.4 与 v0.1.14 教训的对应

| v0.1.14 教训 | 本版对应叶 |
|--------------|-----------|
| release.yml 首跑藏 4 个缺陷（「没跑过」≠「没问题」） | **R4** dry-run |
| 腐化在最贵车道才暴露 | **A3**（左移）+ **A4**（可见） |
| bootstrap 恒 rebuilt、cache 全 miss | **R1 + R2** |
| provenance 有字段没填、用户端零消费 | **H4 + H3** |
| 升级后不知道要 stop-server | **G3 + G7a** |

## 2.5 决策项阻塞关系（**需人工拍板，agent 不自主执行**）

政策项全文见 §5 5.7；此处只列**它阻塞了本版哪些叶**：

| 决策项 | 阻塞的叶 | 不拍板的后果 |
|--------|---------|-------------|
| **P1 / P5**（agenterm.work 归属） | H5（本版未纳入）、间接影响 H1 的托管位置 | H1 仍可做（产物发到现有 Pages），但落地 URL 待定 |
| **G-P1**（macOS unsigned 是否默认通道） | G1（本版未纳入） | 不阻塞已纳入的 G2/G3/G6/G7a |
| **G-P2**（升级遇 running server 的默认策略） | G7b/c/d；**G7a 不受阻**（纯文案） | 只做 G7a 即可交付主要价值 |
| **D1**（preflight 放宽 HEAD 约束） | 本版无叶依赖 | 不阻塞；但若拍板通过会弱化 D3 |
| **P6**（Hub 是否单一 kind 底座） | 本版无叶依赖（H 组只做地基） | 不阻塞 v0.1.15 |
| **P-P1** | **已拍板（编排 2026-08-05）** 见 §6：T2 立项 v0.2.x；T1 不做；v0.1.15 不阻塞 | 粘贴产品化归 v0.2.x |
| **G-P1** | **已拍板**：无 signed 时 **自动回落 unsigned-preview + 强制信任警告**（G1 可开工） | 解锁 G1 |
| **O1b** | **已拍板开工** | Unix 状态栏 IME |
| **O-fix** | **已拍板**：PRD 补 buffer 公开命令（修 prd_alignment 红） | main 红灯 |

> **O 泳道技术决策由编排 agent 拍板，不转嫁董事长。** 域名/预算/Hub 形态等仍见 §5 5.7 人工项。

## 2.6 推迟表（**明确不进 v0.1.15，含理由**）

| 叶 | 推去 | 理由 |
|----|------|------|
| A1 夜间彩排 | v0.2.x | 本版最贵一项（~1 runner-hour/晚）且需先改 concurrency 语义；收益概率性 |
| A2 Candidate 自动派发 | v0.2.x | 触发器分钟级延迟 + 授权链敏感；D1 未拍板前不动 |
| B2 cache key 版本行归一化 | v0.2.x | 需六 workflow 共享算 key 脚本，一致性维护成本高；R1 已拿走大部分收益 |
| B3 双构建复用审计 | 合入 R1 | 已核产物不可互换；真省法是 R1 的增量缓存副产品 |
| C1–C4 竞态收口 | v0.2.x | 均已止血，剩根因排查；C4 明确说了要先观察复发率 |
| D1–D3 政策 | 等拍板 | 见 §2.5 |
| E1 Pages 噪音 | 等 P1 | 与域名归属绑定，先拍板再动 |
| E2 旧 run 清理 | v0.2.x | 纯卫生，无阻塞；moltbaby 已有脚本可随时搬 |
| F1/F2 云桌面快照 | 环境维护 | **不走 PR**——是环境快照尾账，不是代码叶（见 §7） |
| G1 macOS 默认回落 | 等 G-P1 | 政策未定 |
| G4/G5 | v0.2.x | G7a 已覆盖主要价值；G5 是 G7a 的锦上添花 |
| G7b/c/d | 等 G-P2 | 碰 keep-server 默认语义，须人工拍板 |
| H2 install.sh 消费 releases.json | v0.2.x | 依赖 H1 落地并稳定一版后再改消费端 |
| H5 agenterm.work 接通 | 等 P1/P5 | 政策未定 |
| P 组（粘贴）全量跨平台 | v0.2.x 全量；**O2=本机 T0 诊断可先做** | 全量夹具重；macOS 真机半叶归 O 泳道 |
| S 组（结构 SSOT） | **HOLD** | 多 agent 在途，用户通知后先复审再开工（见 §9） |
| O 组 Linux 半叶 / Win IME | 他泳道 | O 只 macOS；N1 Linux、Win I1–I3 不归本机 agent |
| §3.5 UI/UX 观察 | 分散 | T2/SB1/W1 标「顺手做」，其余归 v0.2.0+；本版不单独排期 |
| §5 五条主线 | 各自版本 | L-NET/L-CC/L-EXT/L-PKG/L-CU 只做对齐记录与决策项 |

## 2.7 PRD 一致性核对（2026-08-05，逐叶 grep 实测）

对本版每一叶反查 `PRD.md` 与 `prd/*.md`，找**契约冲突**而非措辞差异。
结论：**一处需修正的是 plan 侧（已改），一处建议反向升级 PRD**。

| 叶 | PRD 侧相关条款 | 判定 |
|----|---------------|------|
| R1/R2 cache | `PRD_02_17:241-243`「Cache miss/corruption 只影响速度，不影响资格」 | ✅ **一致**。R1 纯提速，不碰资格语义 |
| R3 net-research 移出 | 全仓 grep：**无任何 PRD 要求它在 release 门**；唯一提及是 `PRD_02_19:562` 的二进制预算 | ✅ **无冲突**。且符合 §3「门的迁移要说明验证去哪了」 |
| R4 dry-run | `PRD_02_17:193-199` 已写「非发布彩排从未记录…dry-run 能力提为 v0.1.15 项」 | ✅ **PRD 已预留**，本叶正是它的落地 |
| A3/A4 | 无相关契约 | ✅ 无冲突 |
| G3 `--version` | `README:144` 记载 `agenterm-cc.exe` 已有 `--version` 信息命令；无 PRD 禁止 GUI 同样支持 | ✅ **有先例**，不冲突 |
| G2/G6/G7a | 无相关契约（属 install 脚本行为） | ✅ 无冲突 |
| H1 releases.json | `PRD_02_18` M13 已写入「machine-readable `releases.json` derived from existing provenance」 | ✅ **PRD 已归口**，本叶是其第一步 |
| H3 provenance 可见化 | `PRD_02_18` M13「supply-chain evidence becomes user-visible rather than CI-only」 | ✅ 一致 |
| **H4 sbom_sha256** | **`PRD_02_17:237-240`：Candidate aggregation 独立校验「两个 macOS archive provenance 携带同一 SBOM SHA-256」** | ⚠️ **曾误判，已修正** |

### 2.7.1 唯一的实质分歧：H4

**起初的写法有误**。占位稿与本 plan 早期版本称「`sbom_sha256` 空串是
违反声明的证据缺口」。逐平台实测后**这个说法不成立**：

```text
macos  aarch64  sbom_sha256='65c32add1e44e5d96b846…'   ← 已填
macos  x86_64   sbom_sha256='65c32add1e44e5d96b846…'   ← 已填
linux  aarch64  sbom_sha256=''                          ← 空串
linux  x86_64   sbom_sha256=''                          ← 空串
windows aarch64 sbom_sha256=''                          ← 空串
windows x86_64  （无该字段）                             ← 缺字段
```

`PRD_02_17:237-240` **只要求 macOS 双档**携带同一 SBOM 摘要——而 macOS
两档确实都填了。**所以当前实现符合 PRD，没有违约。**

**解决哪一边**：两边都动，但方向不同——

1. **plan 侧（已改）**：H4 的动机从「修违约缺口」改为
   「**把 macOS 已有的保证扩展到六平台**」，因为 M14 Hub 信任分级要对
   所有平台复用该字段。这是**能力扩展**，不是 bug 修复。
2. **PRD 侧（建议，落地后再改）**：H4 完成后，把 `PRD_02_17:237-240` 的
   macOS 限定升级为六平台描述。**顺序很重要**——先有实现再改契约，
   不要先把 PRD 改成尚未成立的样子（否则就是制造一条新的「没跑过」声明，
   正是 §Release-chain operating requirements 警告的反模式）。

> 方法论备注：这次分歧是**我方读得过宽**而不是 PRD 过窄。教训与 v0.1.14
> 的 `manifest.kind` 缺陷同源——**断言一个字段「应该有值」之前，先确认
> 契约到底要求了哪些平台**。

## 3. 明确非目标

- 不动 Candidate/Promotion 的授权语义（D1 除外，且 D1 只在人工批准后做）。
- 不为提速削弱资格覆盖：任何门的移除/降级都要有「该验证去了哪里」的答案
  （如 B1 的 net-research 移去 CI/夜间，而不是删除）。
- 不做投机性并行化（D2 现值低）。
- **不把 §5 未来主线塞进 v0.1.15**：agenterm-net 稳定化、Control Center
  内容成熟、远程包管理、computer-use 各归其版本 plan 与 owning PRD。


## 3.5. UI/UX 现场观察（2026-08-05，自截图 + ui-snapshot-full.json + 源码复核）

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

### 3.5.2.1 标签切换整窗刷新（2026-08-05 用户报告）

**执行清单**：**§1.5 U′**（U1–U4）。本节保留现场观察与根因，不重复排期。

| # | 观察（证据） | 问题 | 映射 | 状态 |
|---|--------------|------|------|------|
| TS1 | 标签区点选不同 tab「基本会触发重新刷新渲染」，打断工作 | ① 无变更 `set-composer`→`ComposerDraft`+旧 tab 全量 delta；② 无条件 font/`layout()`；③ 无条件 `SetWindowText` | **U1**（止血）+ **U2**（回归） | U1 代码已落工作区 |
| TS2 | 切到网格落后于当前窗口的 tab → 立即 resize → 应用整屏重排 | 正确性需要最终 resize；连点时过早 resize 仍重 | **U3** debounce | 未开工 |
| TS3 | 切 tab 须画新 active screen（fill + cells） | 内容替换的一帧，不是假刷新 | 非叶 | 保持 |
| TS4 | 纯 `TabSelected` 仍推全量 cells | 客户端已有 screen 时冗余 | **U4** 可选协议 | 默认可推 v0.2.x |

根因摘要（Win remote GUI，U1 前）：

```text
click tab row
  → sync_composer (was always set-composer)  → ComposerDraft + full tab_update
  → ui-interact select → TabSelected + full tab_update
  → poll_deltas → apply_delta replaces tab objects
  → load_composer / resize_active_terminal
  → tick: if active changed → apply_effective_terminal_font (was always layout)
  → paint: fill terminal + paint_screen(active)
```

### 3.5.3 Windows IME 与协议兼容 UX（2026-08-05 落地，随 v0.1.15 开发）

用户实测缺陷（2026-08-04/05）：终端区中文输入法候选条跟随光标但有
恒定偏移（约 3-4 个汉字，即合成串宽度）；且担心「新版 GUI 连旧版 server」
时只有 cryptic 报错。本轮两处落地：

| # | 改动 | 证据/验证 |
|---|------|----------|
| I1 | Windows 平台适配器在 WM_IME_START/COMPOSITION/ENDCOMPOSITION 时缓存合成串（GCS_COMPSTR + GCS_CURSORPOS）；GUI 在光标处内联渲染合成面板（镜像 Unix frontend preedit），并抑制 IME 自带浮动合成窗（WM_IME_SETCONTEXT 清除 IS_SHOWUICOMPOSITIONWINDOW），候选条锚点保持在光标 | cargo check / clippy -D warnings / 607 lib tests 绿；待真机中文输入回归（AGENTERM_IME_DEBUG=1 + PLATFORM_IME_DEBUG=1 落盘 %TEMP%） |
| I2 | `ui-hello` 拒绝时按 ClientTooOld/ClientTooNew 分类并带双方版本号生成可操作错误；GUI 启动失败与 launcher handoff 被拒时弹原生 MessageBox（新增 agenterm-platform `alert` 能力，走 selected/adapters 边界，product-neutral） | `incompatible_ui_contract_names_the_stale_side` 单测 + 607 lib 全绿；MessageBox 路径待真机验证 |
| I3 | 修复 IME 合成期间拼音按键回显透传进终端：合成中（`ime_composing`）丢弃非提交文本的 `WM_CHAR`（`TranslateMessage` 对合成键的回显），以 `WM_IME_CHAR` 计数放行提交文本（`WM_IME_CHAR`→`WM_CHAR` 1:1），合成结束/失焦重置状态；新增 product-neutral 诊断 `PLATFORM_IME_MSG_TRACE=1` 逐条落盘 `%TEMP%\platform-ime-msg.log` | cargo check / clippy -D warnings / 612 lib tests 绿；用户真机 vim 中文输入通过（`vim set encoding=utf8` 下；提交 `77358bb`+`c71ffd5` 入 main） |

非目标：不改变 ui-bridge 协议版本（仍为 1）；不自动杀旧 server（保留用户
终端会话），错误文案明确指引用户重启/退出旧版。

## 4. 与其它文档的关系

| 文档 | 关系 |
|------|------|
| `plan/archive/plan-v0.1.14.md` | 上一版（已发布）执行记录；本文数据与止血项的出处；未完成叶已 upsert 为 **§1.5 L′** |
| `prd/PRD_02_17_delivery_quality.md` §Release-chain operating requirements | 发布链坑清单权威处（v0.1.13/v0.1.14 两轮合并去重，版本无关；runbook 素材，E2 配套） |
| `plan/ARCHITECTURE.md` | 结构 SSOT（含 §8 对齐机制/工具边界）；**S 组**执行叶指针；本文不重画结构树 |
| `prd/PRD_02_18_roadmap.md` M12 | Control Center 内容成熟（§5 L-CC 的版本归口；原 plan-v0.2.0.md 已并入） |
| `plan/plan-mobile.md` | 移动端计划（第三个 host：接入端 + 去中心化链接端）；与 L-NET/L-PKG 共享去中心化底座，文件域独立 |
| `prd/PRD_02_17_delivery_quality.md` | Candidate/Promotion 合同；D1 若通过需回写 |
| `prd/PRD_02_18_roadmap.md` | 里程碑权威（M11 收敛 / M12 = v0.2.0） |
| `prd/PRD_02_19_inspiration_and_future_vision.md` | 灵感库；§5 各主线 promotion 的入口 |
| `prd/PRD_02_21_control_center.md` | Control Center 边界与能力树 |
| `prd/PRD_02_22_decentralized_network.md` | agenterm-net 成熟度门（N0→N4） |
| `prd/PRD_02_20_native_platform.md` | Platform Facade 收口证据（§5 前置判断） |
| `plan/precision-audit.md` | C 组竞态根因复核的记录处 |
| `install.sh` | 安装/更新实现 SSOT；§8 / G 组改进入口 |
| `plan/plan-v0.1.15.md` §1 **O** + **§11** | macOS 本机 agent 作业规格（ImeStatus / 粘贴 T0 / install UX） |

---

## 5. 未来主线对齐（PRD 对比，2026-08-04 深夜补充）

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

**已被 CI 证明跑通的能力**（`scripts/rh/agenterm-net-research.rh`
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

### 5.2.2 旁路：多 Agent 观察与交接（**v0.1.15 M 组**，非 L-NET）

与 L-NET 无关的控制面地基：同屏多 agent（Codex / Grok / …）经
`agenterm-cli` **只读观察**彼此 tab，handoff 走 note/文件/事件而非 PTY。
执行清单见 **§1.5 M**；实证触发见该节头部。L-CC 未来可投影这些事实，
但不作为 M 的开工前置。

### 5.2.3 旁路：多 Server / Instance 选择（**v0.1.15 S′**）

关窗后找得回、多 instance 作业入口。执行清单见 **§1.5 S′**。
**不做**主终端顶栏横向 server tab 作默认导航；CC 仍是控制塔投影面。

### 5.2.4 旁路：tmux/rmux `send-keys` + buffer（**v0.1.15 B′**）

控制面兼容与脚本往 pane 投递。执行清单见 **§1.5 B′**。
**不**当作 agent 消息总线；协作短消息仍 **§1.5 M**。

### 5.3 主线 L-CC：Control Center 内容成熟（PRD_02_21 → v0.2.0）

- **UX 任务书**：[`plan/plan-control-center-ux.md`](plan-control-center-ux.md)
- **实现级设计 SSOT（rev3）**：[`plan/design-control-center-ux.md`](design-control-center-ux.md)
  （IA / 几何 hit 契约 / PR Plan；2026-08-05 定稿，**实现仍归 v0.2.0**）。
- v0.1.11 壳层已 shipped（进程边界/typed bridge/Cockpit read-only）；
  v0.2.0（PRD_02_18 M12，原 plan-v0.2.0.md 已并入）做内容成熟。
- 用户点名内容：**workflow/pipeline 工作台**（C1 promoted →
  MCP orchestration authority + CC 投影）、**AgenTerm 扩展能力台
  【插件/皮肤/信息】**（J4 promoted → softmgr substrate + PluginHub/
  AppHub 分视图）、**InfoHub**（J5 promoted）。
- 用户提示 **Control Center 可能改名** —— 见 §5 决策项 P2。
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
| D1–D3 | 见 §1 D 组（发布链政策） | 与产品主线独立，但 A2/B1 落地依赖 D1 取向 |
| G-P1 | ~~待拍~~ → **2026-08-05 编排裁定**：无 Developer ID signed asset 时 install **自动选用 unsigned-preview**，并 **强制打印多行信任/预览警告**（不得静默当 stable）；有 signed 则优先 signed。G1 可据此实现 | 解锁 G1 |
| G-P2 | ~~全档待拍~~ → **2026-08-05 编排裁定**：默认 **不**把关窗 default 改成 stop-server（保会话）；**已装 version > 运行 server version 时必须可感知提示**（G7a 已部分交付；G7b/c 可做文案/标注，**不**自动 kill）。一键 apply（G7d）默认 off，仅显式 flag | G7b/c 可开工；G7d 可选 |
| P-P1 | ~~打包待拍~~ → **2026-08-05 编排裁定**：**拆开**——(1) **T2 类型感知** 立项，目标 v0.2.x（O2 已证 pbpaste 无法区分空/图/非法）；(2) **T1 图→路径 本版与下版均不做**（零证据）；(3) v0.1.15 粘贴保持 text-only，图像/无文本继续硬失败，文案可在 T2 前仅做「可能无文本」级提示 | O2 不改码成立；T2 进 v0.2.x |
| P5 | 分发面归属：agenterm.work 作单一入口（含 releases.json 索引）还是仅 docs 别名 | 决定 H1/H5 形态与 E1 走向；P1 的具体化（**仍人工**） |
| P6 | Hub 是否统一为单一 `kind` 底座（plugin/skin/app/info 共用 catalog+验签+事务）还是分立系统 | 决定 P3 皮肤边界的答案与 M14 的范围（**仍人工**） |
| O1b | ~~等用户~~ → **2026-08-05 编排裁定：开工** Unix/macOS 状态栏增加 IME 段，消费 `ime::status()`，对齐 Win 语义；`full_shape` 不可得则不显示伪状态 | O1 闭环用户可见 |
| O-fix | ~~只记录~~ → **2026-08-05 编排裁定：认领** B′ buffer 族（含 `delete-buffer`）写入 owning PRD 公开命令叙述，修 `prd_alignment_public_command_missing`；不删命令 | main 红灯 |

---

## 6. 决策记录

| 日期 | 决策 |
|------|------|
| 2026-08-04 | v0.1.15 主题定为反馈左移 + 发布链降本（占位稿，未授权开工） |
| 2026-08-04 | 代码复核：win-full-gate profile/并发组、candidate dispatch-only、script-smoke 仅 release lane、net-research release 门、hashFiles 缓存 key、release-fast profile、Pages/CNAME、gh-ci-cleanup.sh 参数均属实；run 30907369093 与 pages-build 噪音为 review 结论（本地 gh 不可用，落地时以 Actions 复核） |
| 2026-08-04 | §5 未来主线按用户声明对齐 PRD；P1–P4 为待拍板决策项，未开工 |
| 2026-08-04 | 并发提交 2c5f3d4 已并入 plan-v0.1.15.md 主体与 plan-mobile.md；本工作区仅剩自审修正（E1 措辞 / 决策记录口径 / §3 引用） |
| 2026-08-05 | 自截图 + ui-snapshot-full.json + 源码复核完成标签树区 UI/UX 观察（§3.5 T1-T6/TB1-TB2/SB1-SB2/W1）；全部为观察不改变 v0.1.15 授权范围；T2/SB1/W1 标 v0.1.15 顺手、其余归 v0.2.0+ |

| 2026-08-04 | Linux 云桌面（DISPLAY=:1 XFCE）实测意见写入 §7 / F 组；单测误耦合已修进 main；F1/F2 为环境快照尾账，不走 PR |
| 2026-08-05 | macOS aarch64 真机 0.1.12-local→v0.1.14 安装更新实测写入 §8 / G 组；G1–G6 + G-P1 为改进需求，未授权开工 |
| 2026-08-05 | 用户确认：升级后「关窗不退 server → 再进仍显示旧版」属真实踩坑；要求更新时**自适应或提示**，否则用户无法知道该选 stop-server；追加 **G7 + G-P2**，升 G7a 为 P0 文案、G7b/c 为体验主路径 |
| 2026-08-05 | 结构对齐/工具澄清 upsert：`ARCHITECTURE.md` §8 + 债务 L4；本 plan 增 **S 组**（S1 扩闸 / S2 围栏 / S3 manifest）；明确 LSP≠结构契约引擎 |
| 2026-08-05 | S 泳道 **HOLD**：等其他 agent 完成；用户再通知 → 新一轮 review → 再开工；预备树写入 **§9**（不改代码） |
| 2026-08-05 | 用户报告终端/输入区粘贴常失败：异源 UTF-8 大段（疑 emoji/控制符）+ 截图类 `no pasteable characters`；写入 **§1 P 组 + §10**（硬骨头，未授权开工） |
| 2026-08-05 | 用户补：多 harness 已支持图/复杂文本却透传不过 → §10.3 断裂点 A/B/C（text-only API + 归一 + 无投递协议）；T0/T1/T2 选项 |
| 2026-08-05 | **定稿**：主题由「反馈左移 + 发布链降本」收窄为「发布链降本（cache 优先）+ install 卫生」；依据 §7 实测——cache 撞 10GB 顶致 bootstrap 47s→81s 单调恶化，治理成本最小收益确定（≈3min/次 Candidate），而 A1 夜间彩排是本版最贵项且收益概率性 → A1/A2 推 v0.2.x，保留 A3/A4 |
| 2026-08-05 | 定稿产出 §1.5 收敛工作树（13 叶，含动机/可证伪验收/成本/依赖）、§2 排序理由、§2.5 决策阻塞关系、§2.6 推迟表（含理由）、§2.7 PRD 一致性核对 |
| 2026-08-05 | **PRD 核对纠错**：H4 原称「sbom_sha256 空串违反声明」不成立——逐平台实测 macOS 两档已填、Linux/Windows 未填，而 `PRD_02_17:237-240` 只要求 macOS 双档，故当前实现合规。H4 改为「把该保证扩展到六平台」；PRD 侧待 H4 落地后再升级为六平台描述（先实现后改契约） |
| 2026-08-05 | **定稿后补记**：`fe51c7c` 合并带入并发 agent 的内置皮肤 v1（四预设，约 1600 行，`prd/PRD_02_06` §Built-in skins 已立契约）与 Windows IME/协议兼容 UX（见 §3.5 3.5.3）。二者已入 main 但**不在本次规划的 13 叶内** → 新增 §1.5 X 组登记，并据此调整规模自查：实际范围已不算窄，工期紧时的砍叶顺序定为 H1/H3 → R4，绝不砍 R1/R2。Control Center UX 明确归 v0.2.0，不占本版工期 |
| 2026-08-05 | 用户指出两点：(1) Windows agent 在修其 IME，osx 侧「要有自己的思路，这才是封装的意义」；(2) 工作树全是补丁，问「哪些是新开工的功能」。自查属实——13 叶无一新功能。实证核查发现 `ImeStatus` 契约仅 Windows 实现（286 行），macOS/Linux 各 30 行 stub 恒返回 None，状态栏在 Unix 侧永远 `IME: off` → 新增 **N 组 / N1** 补齐，并本机实测 TIS API 可行（`TISCopyCurrentKeyboardInputSource` 读到「微信输入法」/ zh-Hans）；同时诚实标注 macOS 无法获取 `open`/`full_shape`，按契约规定留空不猜 |
| 2026-08-05 | 用户认同「先把底子弄好」优先于督促 ipfs/libp2p → 新增 §1.5「为什么 v0.1.15 不推进 L-NET」：L-NET 卡点是 N3 产品消费者**形态未定（拍板题）**而非工程量，形态未定前投工程会返工；底子欠账（IME 契约失衡、install 四处硬伤、cache 恶化）是用户每天碰得到的。L-NET 保持 research 车道，R3 只换车道不减验证 |
| 2026-08-05 | 用户确认：`agenterm-cli` 可读 live tab `ds4@codex`（`@2`/`ds4@c`），并问跨 server 通讯前景 → 结论写入对话结论与 **§1.5 M 组**：观察地基已亮；通讯不得以 PTY 为总线。追加 **M1 身份 / M2 只读 observe / M3 handoff 契约 / M4 跨 instance 证据**；硬约束含无副作用观察、单一 Fleet 权威；CC UI 不阻塞 M；砍叶 M4→M3→M2 子命令，保留 M1+M2 文档；绝不砍 R1/R2。X3 指针同步到 `plan/design-control-center-ux.md` |
| 2026-08-05 | 用户报告标签区切换 tab 几乎总触发整窗刷新 → §3.5 **TS1–TS4** 观察 + **§1.5 U′** 可执行子树：U1 假刷新止血（代码已落）、U2 真机回归、U3 debounce PTY resize、U4 纯 TabSelected 不重推 cells（可选/可推 v0.2.x）；砍叶 U4→U3，保留 U1/U2 |
| 2026-08-05 | 用户 GUI 窗全关后提出「顶栏横向 tab 选 server」→ 判断**需求合理、默认形态不采用主窗横向 server tab**。新增 **§1.5 S′**：S1 启动/重开 live instance 列表附着、S2 身份常显、S3 新窗打开另一 instance、S4 同窗热切后置且须确认；硬约束一窗一权威、与 PTY tab 分离、列表复用 server-list；与 L-CC 分工写明；砍叶保 S1 |
| 2026-08-05 | 用户要求排期兼容 tmux/rmux **send-keys + buffer-paste/copy** 以便「先能发信息」→ 判断 **B′ 控制面兼容要做，但不替代 M handoff**。新增 **§1.5 B′**：B1 契约盘点、B2 夯实 send-keys、B3 命名 buffer 最小集、B4 paste-buffer、B5 与 M 选用表、B6 可选 copy→buffer；硬约束一 pane/tab、有界、不抢焦点、显式 unsupported；排序 B1→B4 为核心 |
| 2026-08-05 | **B′ 落地（工作区）**：`named_buffer` store + CLI `set/load/show/list/delete/paste-buffer`（别名 setb/loadb/…）；`send-keys` usage 补 PS `@N`；隔离 instance 黑盒：`set-buffer`→`paste-buffer`→capture 见 `BUFFER_PROBE_OK`。live main 须换新 `agenterm-server` 后才有命令。agent 协作仍优先 note；paste 进 Codex TUI 会打断 |
| 2026-08-05 | **P0 关窗再开重置**：根因=新 server + workspace 假恢复 / 双 main。修 `find_live_endpoint_for_logical_instance` + GUI connect 附着 + `start_frontend_server` 拒双开；黑盒同 instance 仅 1 live、tab note 保留。P0-3 job breakaway 未做 |
| 2026-08-05 | **关窗确认丢失**：`SC_CLOSE`→`CloseRequested`；`ensure_window_close_dialog_presented` 先 restore 再 layout/show 三按钮；AlreadyOpen 二次 close 重 assert。隔离 instance 黑盒：`ui-action close-window`→`confirm-window-close`；最小化后 close→restore+modal；Keep→重开同 `server_pid`+tab 名保留 |
| 2026-08-05 | **S′ S1–S3 落地**：`instance_identity` + `instance_picker`；`ui-snapshot.window.instance/server_pid/endpoint`；状态栏 Connected·instance·pid；系统菜单 Open instance…；CLI `open-instance-picker` / `instance-picker-select --name` / `confirm` / `open-instance --name`；Attach 延后 rebind 避免 command 过期；OpenAnother breakaway 新窗。Unix picker 诚实 gap |
| 2026-08-06 | **用户澄清**：只要窗口**顶栏横向 server tab**。落地 `server_strip` 几何 + 绘制/点击 + `select-server-tab --name` + snapshot `layout.server_strip`；点选 rebind 当前窗（非左侧 PTY 树） |
| 2026-08-06 | **版本列车 0.1.15 + 顶栏/CC 真机迭代**（工作区 → main，用户测通后授权推远程）：(1) `Cargo.toml` / platform / tasks → **0.1.15**；(2) 默认 `build.bat` stage **release-fast** 到 `dist/`（`target/debug` 仍为纯 debug PE；显式 `build.bat dev`）；(3) **关窗确认**：重入 `window_proc` 禁 DefWindowProc(SC_CLOSE/WM_CLOSE)、整客户区变暗；(4) **server strip 可点**：鼠标路径 `flush_deferred_server_tab_attach`（曾只 defer 不 attach）；全部芯片可点（取消 mid-break 丢尾）、进程仍活即可 enter（空 tab 树不拦）、stale 点选开新窗；(5) **布局**：strip 仅终端列；左上时钟 `HH:MM:SS`；标签树几何 **+sidebar_tree.top** 不再压时钟；(6) **CC**：优先 `agenterm-cc-web.exe`（direct-WRY 三 Tab：超级智能体 / InfoHub / 超级控制；GUI 子系统无黑控制台；父控制台 AttachConsole 保 `--help`/`--probe`）；缺文件回落原生 `agenterm-cc`。证据：双 live instance 黑盒 strip 跳转；open-control-center 起 web 壳；geometry 单测 strip+clock+tree。相关提交含 `933b92e`…`e56cb67`；设计稿 `plan/plan-cc-automation-cli.md`（自动化 CLI 未开工） |
| 2026-08-05 | 用户真机回归：vim 中文输入「中文+乱码」顺序输出 → 根因 = IME 合成期间 `TranslateMessage` 把拼音按键回显成 `WM_CHAR` 并透传进终端（用户猜测命中「不该透传的事件透传了」）。落地 **I3**（§3.5 3.5.3）：合成中丢弃非提交 `WM_CHAR`、`WM_IME_CHAR` 计数放行提交文本、失焦/结束重置；`77358bb`+`c71ffd5` 入 main。vim `set encoding=utf8` 下真机通过（用户提示此前可能也可行，编码未深究）。另状态条 CURSOR/MOUSE 读数 + 输入区 Ctrl-O/Ctrl-A（`5711880`）已入 main；本次 exe 构建为 dirty（含 B′ 未提交改动） |
| 2026-08-05 | 用户要求在 plan 写入 **OSX 要做的事子树**，供另一 agent 在本 macOS 机跟进 → 新增 **§1 O 组 + §11**（O0 基线 / O1 ImeStatus / O2 粘贴 T0 / O3 install UX / O4 合成对照 / O5 可选；禁 Win 域） |
| 2026-08-05 | 用户真机：AgenTerm **Shift+鼠标选区后复制不了**，阻塞工作 → **O6** 入 O 组/§11，排序 **O0→O6→O1…**；疑点含 complete 后 `let _ = copy` 静默失败、Cmd+C has_selection、shift 手势未建选区、pbcopy 写失败 |
| 2026-08-05 | **O6 关闭** `fb573f9`（O6a+O6b）；§11.8 定因全成立；agent(cc) 更正「pre-existing flake」归因 → 稳定红 `prd_alignment_public_command_missing:delete-buffer` |
| 2026-08-05 | **编排拍板（不转嫁董事长）**：P-P1=T2 立 v0.2.x / T1 不做 / v0.1.15 text-only；G-P1=无 signed 自动 unsigned+警告；G-P2=不默认 kill server，版本落后须提示；**O1b 开工**；**O-fix 认领** PRD 补 buffer 公开命令。agent 问决策 → 编排回写 §5 5.7 + §1 O |
| 2026-08-06 | **v0.1.14 未完成 upsert → §1.5 L′**（L1–L7 可执行 + L8→C1）；`plan-v0.1.14.md` / `goal-v0.1.14.md` **归档**至 `plan/archive/`（已公开发布 tag `8ff2b5a`）；archive README 与引用指针改指向 archive 路径 + 在制 `plan-v0.1.15.md` |
| 2026-08-06 | **plan/ 卫生**：再归档已落地/superseded 专题（`plan-agenterm-server-mode`、`plan-skins-v1`、`plan-platform-facade-v4`、`osx-cpu-improve`、S′ goal、`platform-ui-ux-boundary-tree`）；新增 `plan/README.md` 现行索引；PRD/ARCHITECTURE 链接改指向 archive |
| 2026-08-06 | **跨平台 shared-first 试跑**（用户：Win 先做再 OSX/Lnx 对齐太慢）：`AGENTS.md` 增「Cross-platform UI: shared-first」；`src/frontend/ui_action_catalog.rs` 立 SHARED + WINDOWS_ONLY + UNIX_ONLY 清单与 set-diff / 源码字面量单测；ARCHITECTURE L2 标 interim gate + agent 禁令 #7。**未做**完整 ui-action 表驱动——只挡「单端默默加动词」。可执行 goal：`plan/goal-crate-platform.md` |
| 2026-08-06 | **CLI ui-input 交付**（远程 main）：见本文 §9；`operations` 登记 ui-input 与四枚孤儿 ui-action；Windows 侧 ui-input 仍为开放决策 |
| 2026-08-06 | **goal-crate-platform 执行**：P0 边界 SSOT（platform README / ARCHITECTURE §1.0 / AGENTS Platform crate vs product UI）；P1 gap 表 `plan-platform-encapsulation-gap.md` + **G1 收口** `spawn_breakaway_visible_*`（去产品侧 `ERROR_ACCESS_DENIED=5`）；P2 catalog 纪律测；P3 执行句式 |
| 2026-08-06 | **goal-crate-platform 加深**：G6 纠正 catalog——`control_dispatch` 已实现的 24 个动词从 WINDOWS_ONLY 升 SHARED（防「假 parity-gap」）；G7 `open-new-terminal` Win 接线并升 SHARED；G2 script/worker spawn 审计证伪；G1 回归测禁产品硬编码 `raw_os_error==5`；WINDOWS_ONLY 余 17 条按 strip/settings/font 分组 `parity-gap` |
| 2026-08-06 | **goal-crate-platform 封装完结（contract）**：G8 `font-decrease`/`font-increase`/`toggle-locale` Unix 补 ui-action 并升 SHARED（方法本已存在）；gap 文写「完结定义」+ G3/G4 标 out-of-goal residual；WINDOWS_ONLY 余 14=strip/picker+settings-scope（产品叶非 OS 泄漏）；成功清单在 `goal-crate-platform.md` 勾选 |
| 2026-08-06 | **三端并发派工写入本文 §2.2.1**（用户要求不另开 orchestrate 文件）：泳道 CI-R / G-install / Win-UX / Unix-UX / Lnx-env / S-HOLD；unix/frontend 单写者=OSX；shared-first + 热文件互斥；§1.5 与 §11 指针回链 |
| 2026-08-06 | **章节编号改为阿拉伯数字** `x.y.z`（废止「一、二·二-b」等中文章节号）；交叉引用同步为 `§2.2.1` 等形式；原 CLI 专节与结构 §9 撞号 → CLI 改为 **§12** |
| 2026-08-06 | **Win CI-R 主波落地**：R1 CI target **v3-slim**；R2 Candidate cargo-home **restore-keys**；R3 net-research 出 release 门（gates.json + `check.rhai`，现 `scripts/rh/check.rh`），push CI linux 仍跑；A3 script-smoke 进 windows release-lane-smokes；A4 Candidate summary 强化 bootstrap 行。R4 未做。OSX/Lnx 接手见 **§2.2.2** |
| 2026-08-06 | **移除 agenterm-mux / agenterm-mcp 独立 PE**：用户拍板不保留兼容入口；权威入口仅为 `agenterm-cli mux` / `agenterm-cli mcp`。Cargo bins、artifacts.json、install、smokes、PRD 已同步。 |
| 2026-08-06 | **续推**：R4 dry_run 配置合入 `release.yml`；H4 全平台 `sbom_sha256`；G3 `agenterm --version` + `installed.json`；G2 断链清理；G7a 文案加强。G6 releases 修剪仍开 |
| 2026-08-06 | **续推 2**：G6 `prune_old_releases`（`AGENTERM_RELEASES_KEEP`）；U3 Win tab PTY resize debounce 100ms；P0-3 文档/单测锁 breakaway autostart。U2 真机/H1 releases.json/B′/M 仍开 |
| 2026-08-06 | **H1+H3+B′ 勾选对齐**：`build-releases-index.rh` + release.yml 派生/上传 `releases.json`；install.sh 下载并校验 `.provenance.json` 写入 `installed.json`；B1–B5 与已 shipped buffer/send-keys 对齐。H2（install 消费索引）仍 v0.2.x；B6/U2/M/N 仍开 |
| 2026-08-06 | **B′ 尾巴**：`save-buffer` 显式 unsupported；`paste-buffer` 空失败 + UTF-8 规范化/bracketed-paste；`cli-smoke` `cli.named-buffer-paste`（set→paste→capture）。B6 仍开 |
| 2026-08-06 | **测试门收口（不发布）**：rustfmt/clippy/rh pack Windows dll 名、fleet-smoke `mux_argv`、prd-alignment（mux/rh-pack）、quick unit 超时与 CJK 测试宿主字体条件；`lint`+`check --quick` 绿；隔离 GUI buffer 黑盒绿 |

---

## 11. macOS / OSX 本机 agent 作业规格（2026-08-05 派发）

> **给接手 agent 的完整上下文。** 用户本机：macOS aarch64（曾验证 macOS 26.5 + 微信输入法 TIS）。  
> 仓库：`agenterm`，分支 `main`（派发时 HEAD 约 `b15b145`，开工前务必 `git pull`）。  
> 目标树入口：**§1 O 组** + 收敛叶 **§1.5**；三端并行派工 **§2.2.1**（G-install + Unix-UX）。  
> 契约 SSOT：`plan/ARCHITECTURE.md`；shared-first：`AGENTS.md` + `ui_action_catalog`。  
> **不做**：Windows IME 文件、发布链 R/H 核心（归 CI-R 泳道）、S 组结构大重构（HOLD）、T2 粘贴。

### 11.1 分工（避免撞车）

| 泳道 | 谁 | 文件域 |
|------|-----|--------|
| **O（本规格）** | **本 macOS 机 agent** | `adapters/macos/**`、unix frontend 粘贴/IME 消费侧、`install.sh` 文案级、本机证据 |
| Win IME I1–I3 | 已入 main / Win agent | `adapters/windows/**` — **O 禁写** |
| 发布链 R/A/H | 他 agent | workflows / `scripts/rh/check.rh` — O 勿抢 |
| N1 Linux 半叶 | 另派 | `adapters/linux/ime.rs` — O 默认可只留 stub 注释 |

### 11.2 作业树（执行序）

```text
O  macOS 本机泳道
│
├─ O0 基线（30min 内）
│  ├─ git pull --ff-only；记录 HEAD
│  ├─ agenterm-cli --version；readlink ~/.local/share/agenterm/current
│  ├─ 状态栏 IME 读数截图/笔记（预期常为 off / 空，因 stub）
│  ├─ **必做**：终端区 左键拖选 + Shift+鼠标选 + Cmd+C → 能否 `pbpaste` 见文
│  ├─ 粘贴四例各一次：纯中文、emoji 行、截图、他终端大段 → 记成败文案
│  └─ 产出：§11 进度表「基线」行（含 O6 复现结果）
│
├─ O6 Shift/拖选无法复制（**优先修复 · 阻塞工作**）
│  ├─ 复现并分类 a–d（§1 O6 疑点）
│  ├─ 修：静默 `let _ = copy` → 可见失败；gesture complete；Cmd+C has_selection
│  ├─ 真机验收四条见 §1 O6
│  └─ 与 O2 共享 clipboard 写路径时串行，先 O6 后 O2
│
├─ O1 ImeStatus macOS（主交付 · 对齐 N1）
│  ├─ 改 adapters/macos/ime.rs：TISCopyCurrentKeyboardInputSource + 属性
│  ├─ link Carbon/HIToolbox（按 crate 现有 FFI 风格）
│  ├─ 单测：能报则报、不能报不猜
│  ├─ 真机：微信输入法 / ABC 切换，状态栏或 label() 符合 N1 验收
│  └─ 提交 pathspec 精确；回写 N1 checkbox（macOS 半叶）+ O1
│
├─ O2 粘贴 T0（真机诊断优先，小修可选）
│  ├─ 按 §10 夹具复现；定位断裂点 A/B/C（§10.3）
│  ├─ 最小可交：错误码/文案细分（image_only / empty / invalid_utf8）
│  │     或书面报告「须跨平台改 ui_clipboard，本机只证」
│  └─ 禁止：未批 T1 写 temp 图路径；禁止改 Win clipboard
│
├─ O3 install/升级 UX（本机可证）
│  ├─ G7a：有旧 server 时升级/重开提示是否可理解
│  ├─ G2：BIN 断链扫描
│  └─ G1：无 G-P1 只写探测笔记，不改默认回落策略
│
├─ O4 合成路径（观察+小修）
│  ├─ vim + 中文 preedit 是否完整；与 O1 状态是否一致
│  └─ 问题记 plan；大改走新叶，不抄 Win control_window
│
└─ O5 可选
   ├─ physical pointer acceptance
   └─ CPU：仅用户仍报卡时对照 archive/osx-cpu-improve.md
```

### 11.3 验收命令（亲测，示例）

```bash
git pull --ff-only
cargo test -p agenterm-platform --all-features   # 含 ime 单测时
# 或仓库惯用 quick：
# ./check.sh --quick

agenterm-cli --version
# GUI O6：拖选/Shift+选 → 松手或 Cmd+C → pbpaste 应含正文
# GUI：切换输入法看状态栏；粘贴四例；升级场景若需要再跑 install 文案
pbpaste | head -c 200
```

### 11.4 进度表（接手 agent 更新）

| 叶 | 状态 | HEAD/笔记 |
|----|------|-----------|
| O0 | [x] | `b15b145`；`current → 0.1.14-macos-aarch64`；`agenterm-cli` 在 `~/.local/bin` |
| **O6** | [x] **已修** `fb573f9` | O6a 止血 + O6b shift-extend；定因见 §11.8，交付见 §11.13 |
| O1 | [x] **adapter 半叶** `28d6959` | 见 §11.6；**消费侧半叶未做（新发现，见 §11.7）** |
| O2 | [x] **诊断完成，判定不改码** | 发现 `Ok("")` 三态重叠；须 T2 才能修，等 P-P1；见 §11.11 |
| O3 | [x] `ee41cc6`（G7a 文案）| G2 无断链（§8 该条已过时）；G1 仅探测，等 G-P1；见 §11.12 |
| O4 | [x] **对照完成，无需改动** | 合成路径实现完好；见 §11.9 |
| O5 | [x] **实测后判定无需开工** | 本机 idle 6.0% CPU；见 §11.10 |

### 11.6 O1 交付记录（2026-08-05 · `28d6959`）

**改动**：`crates/agenterm-platform/src/adapters/macos/ime.rs` 30 行 stub → 真实现，
走 HIToolbox Text Input Sources（`TISCopyCurrentKeyboardInputSource` +
`kTISPropertyInputSourceType` / `kTISPropertyLocalizedName`）。
**未新增依赖**——Carbon/CoreFoundation 符号手写声明，签名对齐仓内既有的
`adapters/macos/process_window.rs`（`*mut i8` / `bool` / `*const c_void`），
避免 `clashing_extern_declarations`。

**本机实测**（macOS 26.5 + 微信输入法）：

```
STATUS name="微信输入法" available=true open=true native=true full=false
LABEL=IME: 微信输入法 · native
```

`cargo test -p agenterm-platform --features ime` → 50 passed；
`cargo clippy --all-targets` 零告警。

**一处判断修正（值得记）**：初版按「macOS 观测不到就留空」把 `open` 恒置 false，
实测 label 输出 `IME: 微信输入法 · latin`——**用户正打中文，状态栏却说 latin**。
复核契约语义：`open` 是「拦截合成 vs 键击直通」。macOS 没有 IMM 的开/关开关，
**选中 keyboard input mode 本身就是拦截态**；报 false 不是「谦虚留空」，
而是**断言了一件假事**。故 `open`/`native_mode` 均随 input-mode 选中态。
`full_shape` 才是真不可观测（在输入法自身进程内，无公开 API），保持默认。

> 教训：契约说「不能报的字段留空」≠「拿不到就填 false」——
> 先看该字段的**默认值本身是否在断言某种状态**。

### 11.7 新发现：O1 只完成了一半（消费侧缺口）

实证核查（`grep -rn "ime::status"` 全仓）：**`ImeStatus` 只有一个消费者**——
`src/platform/adapters/windows/remote_frontend.rs:2285`（`refresh_ime_label`）。

Unix frontend（`src/platform/adapters/unix/frontend/`）**没有 IME 状态段**：
只处理 preedit（`render.rs:1122 render_ime_preedit`，走 winit 事件），
`StatusBarView` 无 ime 字段，也无 `last_ime_label`。

**含义**：adapter 现在会如实报数，但 **macOS GUI 上仍看不到**。
N1 原叙述「Unix 状态栏永远 `IME: off`」**不够准确**——真实情况是
**状态栏压根没有这个段**。故 O1 拆为：

- **O1a adapter 半叶** — 已交付（`28d6959`），单测与任意消费者可用；
- **O1b 消费侧半叶** — 未做：Unix `StatusBarView` 加 ime 段 + 布局 + poll 刷新。
  须先对齐 Win 侧 `status_segments` 约定，否则两端 UI 不一致
  ——**这属于 §1「多平台 UI/UX 对齐」主题，不是纯 macOS 私事**。

O1b **未擅自开工**：要动 `unix/frontend/render.rs` 状态栏布局（跨平台 UI 决策面），
且 Win agent 正在同 crate 活动——先报用户再定。

### 11.8 O6 代码级定因（2026-08-05 · 诊断时未改代码；已由 `fb573f9` 落地，见 §11.13）

按 §1 O6 的 a–d 分类逐条查证（诊断结论；实现状态见 §11.13）：

| 疑点 | 结论 | 证据 |
|------|------|------|
| **b) shift 手势未建选区** | ✅ **坐实，即主因** | 全文件 `grep -i shift.*select` → **零命中**。`begin_terminal_selection`（`unix/frontend/mod.rs:2727`）只有 **双击选词** 与 **普通拖选** 两条分支，**没有任何 shift-extend 分支** |
| a) 松手 auto-copy 被吞 | 存在但非本例主因 | `complete_terminal_selection:2866` 确为 `let _ = self.copy_terminal_selection()`，**四处 `let _ =`**（2748 / 2763 / 2866 / 5137）会静默吞掉写盘失败——但 (b) 下根本走不到这里 |
| c) Cmd+C 未识别 | 未证伪，次要 | `copy_terminal_selection:2943` 要求 `terminal_selection` 有值；(b) 下它恒为 `None`，直接返回 `Err("no terminal text is selected")` |
| d) pbcopy 写失败 | **排除** | `macos/clipboard.rs:41` 走 `pbcopy`，本机 `pbcopy`/`pbpaste` 正常 |

**因果链**：Shift+点击 → 无 shift-extend 分支 →
`terminal_selection` 未建立 → `copy_terminal_selection` 早退 `Err` →
调用点 `let _ =` **吞掉错误** → **状态栏无任何反馈**，用户看到「按了没反应」。

> 注意 `forward_terminal_mouse`（`mod.rs:3180`）的注释说
> 「Shift bypasses reporting so local **selection** stays reachable（xterm 惯例）」
> ——即产品**自称**支持 shift 走本地选区，但本地选区侧没实现 shift 扩展。
> **文档与实现不一致**，不只是缺功能。

**建议修法（两叶，须用户拍板范围）**：

- **O6a 止血（小、纯收益、建议先做）**：把四处 `let _ = copy_terminal_selection()`
  改为失败时 `set_status_message`/feedback 带错误码。
  即使不实现 shift 扩展，用户至少能看到「no terminal text is selected」
  而不是**静默无反应**。符合 §1 O6 验收第 4 条「禁止静默」。
- **O6b 实现 shift-extend**：在 `begin_terminal_selection` 加分支——
  按住 Shift 且已有 `terminal_selection` 时，**扩展**锚点到当前 cell 而非新建手势。
  需先定：锚点取上次选区的 start 还是 end（xterm 取「较远端」）。

**历史注记**：诊断阶段曾「未擅自开工」；后由 agent(cc) 在用户阻塞优先级下
落地 O6a+O6b（`fb573f9`），§11.13 为交付记录。

### 11.9 O4 合成路径对照（2026-08-05 · 只读核查，**结论：无需改动**）

O4 的原始担心是「winit IME 事件是否有缺洞」。逐条查证后**担心不成立**——
Unix 侧合成路径是**完整实现**，不是半成品：

| 环节 | 位置 | 状态 |
|------|------|------|
| 事件入口 | `unix/frontend/mod.rs:4995` `PixelWindowEvent::Ime → handle_ime` | ✅ |
| 事件分类 | `mod.rs:964 handle_ime` → 复用**平台中立**的 `ime::classify_event` | ✅ 与 Win 共用同一状态机 |
| preedit 渲染 | `render.rs:1122 render_ime_preedit` | ✅ 含光标 |
| 候选框定位 | `mod.rs:3959 set_ime_cursor_area`（每帧） | ✅ |
| 锚点计算 | `mod.rs:3334 ime_anchor` | ✅ 覆盖 terminal / composer / 新建终端模态 / 侧栏标签编辑器；模态开启时正确返回 `None` |
| 启用 | `mod.rs:568 with_ime_allowed(true)` | ✅ |

**因此 macOS `composition()` 返回 `None` 是设计正确、不是缺口**：
preedit 经 winit 事件推送（`ImeEvent::Preedit`），不走轮询；
契约注释本就写明「winit hosts deliver the same data as ImeEvent::Preedit
and report None here」。同理 `set_anchor_position` 为 no-op 也正确——
winit 通过 `set_ime_cursor_area` 代劳，**且已被逐帧调用**。

> **这修正了 N1 的一处隐含误读**：N1 说 macOS IME「只有 30 行 stub」，
> 容易读成「macOS 输入法整体没做」。实际上**合成/候选框/preedit 全都有**，
> 缺的**只是状态查询**（O1 已补）。macOS 用户本来就能正常打中文，
> 缺的是状态栏显示——影响面比 N1 描述的小。

**不抄 Win 的 `WM_IME_*` 进 macOS**（O4 原则），因为 winit 路径已覆盖且更合适。

### 11.10 O5 尾账实测（2026-08-05）

**CPU**：O5 的触发条件是「仅当用户仍报高 CPU 再回归测」。
`plan/archive/osx-cpu-improve.md:3` 记 `P0–P3 all shipped`，本机实测现网 v0.1.14：

```
$ ps -Ao pid,pcpu,pmem,comm | grep agenterm
48771   6.0  0.9  .../releases/0.1.14-macos-aarch64/agenterm
```

**6.0% CPU（空闲态）**——不是该计划针对的病态占用，触发条件不成立，
**本版不开工**。若用户后续再报卡顿，回归入口仍是 `plan/archive/osx-cpu-improve.md`。

**physical pointer acceptance**：属 parity 矩阵尾账，无用户诉求驱动，
优先级低于 O6（用户阻塞）与 O2（复制粘贴刚需），**本版不做**。

### 11.11 O2 粘贴真机诊断（2026-08-05 · **诊断完成，判定不改码**）

本机实测五例（含一例超纲）：

| 用例 | 实测结果 | 分类 |
|------|---------|------|
| 大段 CJK/emoji（230,000 B） | `Ok(text)`，往返完整 | ✅ 正常 |
| SGR 转义序列 | `Ok("\x1b[31m…")`，ESC 是合法单字节 UTF-8 | ✅ 读盘层正常 |
| **截图独占剪贴板** | `pbpaste` 输出 **0 字节 / exit 0** → `Ok("")` | **C 无投递** |
| 真空剪贴板 | 同上 `Ok("")` | 基线 |
| **非法 UTF-8**（`68 69 FF FE 41`） | **`pbpaste` 自己静默降级为空**，非法字节**根本没到进程** | **C**（比预期早一层） |

**核心发现：`Ok("")` 是三态重叠**——真空 / 图像独占 / 非法文本，
在 `clipboard.rs` 内部**无法区分**。

**已独立复核**（我本人跑，非仅采信 agent）：

```
$ osascript -e 'clipboard info'
«class utf8», 5          ← 剪贴板确实有 5 字节 utf8 数据
$ pbpaste | wc -c
       0                 ← pbpaste 却给 0 字节
```

**结论：本叶不改码。** 要区分「有图无文」与「真空」，
必须查 pasteboard **类型**（`pbpaste` stdout 给不了）——
那**就是 T2（多 MIME/类型感知）换个说法**,而 T2 被 P-P1 明确门控。
现有四变体 `ClipboardError` 对**它能观测到的**一切结构上是够的;
在没有新类型信息前继续打磨文案**不会增加真正的区分力**。

> **升级给用户的政策输入（P-P1）**：
> - **T2 确有必要**——上面是「结构上不可能」级证据,不是偏好问题;
> - **T1（图像写临时路径）无证据支持**——本次诊断未产生任何需要它的数据,
>   两者应**分开拍板**,不要打包。

### 11.12 O3 install/升级 UX（2026-08-05 · `ee41cc6`）

**G7a 已修（仅文案）**。修前完成横幅止于 PATH 提示——
**不提有 server 在跑、不提会继续用旧版本、不给下一步**。修后：

```
==> A running AgenTerm server was detected; it will keep using its already-loaded version
==> To switch a running window to v0.1.14: close it choosing "stop server and exit"
    (not "keep server running"), then reopen AgenTerm
==> Alternatively run: agenterm-cli shutdown  (then reopen AgenTerm)
```

实现是 `install.sh` 末尾 6 行 `pgrep` 判断（已复核 diff）：
**不杀进程、不改默认安装行为、不预判 G-P2**。

**G2 无事可做**：本机 `current` → `0.1.14-macos-aarch64` 存在，
五个 bin 软链全部解析到真实文件，**零断链**。

> ⚠️ **§8 有一条已过时**：§8 8.1/8.2 第 3 点称
> 「旧 `agenterm-script` BIN 链断链残留，需手动 `rm`」——
> **本机现已不存在该软链**（只在旧 local release 载荷目录里有同名文件，
> 那是死重量，不是断链）。该条应标注为「已解决/会话特定」。
> §8 其余条目（G1 命令、G3 `--version`、G7 默认）**复核仍准确**。

**G1 仅探测，未动默认**（G-P1 未拍板）。实测：

- 失败（今日默认 `curl | bash`，无 env）：404 → fail-closed
  `error: signed macOS asset is unavailable; set AGENTERM_ALLOW_UNSIGNED_PREVIEW=1 …`
- 可用：`AGENTERM_ALLOW_UNSIGNED_PREVIEW=1` → 下载 unsigned-preview zip，SHA-256 校验通过

**G-P1 三选项（交用户拍板，agent 未替你定）**：

| 选项 | 含义 | 代价 |
|------|------|------|
| **A** 保持 fail-closed，只补文档/文案指向 env | 风险最低，不动信任模型 | 首次安装仍需用户读文档 |
| **B** 无签名资产时 macOS 自动回落 unsigned + 大声警告 | 最方便 | **默认把「跑未签名预览版」正常化** |
| **C** 保持 fail-closed，但把错误文案本身写成完整政策说明 | 读起来像刻意设计而非 bug | 仍需 env |

**未做**：G7b/G7c（GUI 版本不匹配横幅、关窗对话框感知升级）——
需动 frontend 且受 G-P2 门控；未对真实运行中的 server（PID 48771）
执行 `shutdown` 实测——那会打断用户正在用的会话，改用源码+pgrep 推证。

### 11.13 O6 交付（2026-08-05 · `fb573f9` + `53a3749`）

> ⚠️ **验证状态必须如实说明（我先前说过头了，此处更正）**：
> 我一度写「O6 已修/用户阻塞项已解」，但**当时只跑了单测，没跑 GUI**。
> 用户随后反馈「选中文复制还是不成功」——**根因是他跑的二进制不是修复后的**：
> 安装版 `0.1.14-macos-aarch64` 构建于 **8/4 19:14**，
> 而 O6 修复提交于 **8/5 22:44**。**旧码不含修复**。
>
> 现已 `cargo build --release` 出新二进制并在独立 instance（`--instance o6test`）
> 拉起验证进程存活、CLI 可驱动。但**「Shift+鼠标选中文→复制成功」这一条
> 端到端真实手势仍未由我亲手点击验证**（GUI 选区只能鼠标驱动，
> `ui-interact select` 是窗口选择不是文本选区）。
> **待用户在新二进制上实测确认后方可标记为已验证。**

**O6a 止血**：四处 `let _ = copy_terminal_selection()` 全部改为
`Err` 时 `set_status_message("Copy failed: {error}")`，
Cmd+C 那处另加 `request_redraw()`（对齐紧邻 `Paste` 分支的写法）。
**四处无一是「合法静默」**——每处都在「选区刚刚赋值成功」的分支内，
此时复制失败是真错误，不是空操作。

**O6b shift-extend**：在 `begin_terminal_selection` 顶部新增分支
（先于双击/选词判断）。锚点按 xterm 惯例取**离点击较远的那个端点**
（行距优先，同行时列距决胜），抽成纯函数 `shift_extend_anchor`，
配 3 个单测（`shift_extend_anchor_flips_when_click_is_above_selection` /
`_keeps_the_far_endpoint_when_click_is_below_selection` /
`_breaks_ties_on_same_row_by_column_distance`，实测全绿）。

> **修饰键无需新增管线**：`self.pointer_modifiers` 早已由
> `PointerButton::Pressed` 处理器在调进本函数前更新——
> **事件循环一直把 shift 状态送到了门口，只是没人读**。
> 这也解释了为什么 §11.8 的「文档与实现不一致」成立得如此彻底。

**§11.8 定因全部成立**，无一条被推翻。

#### 中文场景的追加核查（`53a3749`）

用户报「选含中文的复制不成功」，我逐层查证**中文本身不是断点**：

| 层 | 实测 |
|----|------|
| 剪贴板写入 | `set_text("中文测试")` → `pbpaste` 完全一致；多行中文、emoji 混排同样 `match=true` |
| 选区取文本 | `terminal_selection_text` 用 `is_wide_continuation` 跳过续格，**不会把中文切半**；新增单测 `all_wide_selection_returns_whole_characters_and_counts_chars_not_bytes` |
| 命中测试 | `terminal_cell_at` 按网格列算，宽字符无特殊路径 |

**但查出一个真 bug 并已修**（`53a3749`）：
`copy_terminal_selection` 用 `text.len()` 报长度——
**Rust `String::len()` 是字节数不是字符数**。
选 4 个中文会显示 **`Copied 12 characters`**，
**复制其实成功了，但反馈数字是错的**，容易被读成「复制坏了」。
已改为 `text.chars().count()`，并加单测钉住 4 chars / 12 bytes 的区别。

#### ⚠️ 一处需要更正的测试归因（我复核后修正）

接手 agent 报告称全量 `cargo test` 有两个集成测试失败，属
「pre-existing / 与本改动无关的并行污染 flake」。**该归因不准确**，
我实测如下：

| 条件 | `tests/performance_summary.rs` | `tests/rhai_migration.rs` |
|------|-------------------------------|--------------------------|
| `fb573f9~1` 全量 `cargo test` | ✅ 全绿（625 passed） | ✅ 全绿 |
| `fb573f9` 全量 `cargo test` | ✅ 绿 | ❌ 2 failed |
| `fb573f9~1` **单独**跑 rhai_migration | — | ❌ **同样失败** |

**结论：确实不是 O6 造成的**（单独跑基线也失败，故与 O6 无因果），
**但也不是「并行 flake」**——它是**稳定失败**，真实根因是：

```
prd_alignment_public_command_missing:delete-buffer
```

`delete-buffer` 命令已存在于 `src/commands.rs:74,305,688` 与
`control_dispatch.rs:1365`，但 **PRD 公开命令目录里没有它**
（`grep -rn "delete-buffer" prd/` 零命中）。经 `git log` 追溯，
该命令随合并 `b15b145` 进入 main，**早于本次 O 泳道**。

> **归属：非 O 泳道，属他人未完成工作**（新增公开命令未同步 PRD 目录）。
> 此处只记录、不擅自改 PRD——PRD 公开命令面属产品契约，
> 且该命令的归属 agent 可能正在处理中。**但它现在是 main 上的红灯，
> 需要有人认领**。
>
> 教训：agent 报「pre-existing flake」时要核——
> 「不是我造成的」与「是随机 flake」是两个不同结论，
> 后者会让一个**稳定的真失败**被当噪音放过去。

### 11.5 激励

每一步都在把 macOS 从「stub 假平权」拉回 facade 真契约；本机验证是 Win 侧无法替代的价值。

### 11.14 编排拍板后的 OSX agent 下一刀（2026-08-05）

> **给仍在本机的 agent：以下为已授权动作，勿再问董事长。**

| 序 | 叶 | 动作 | 验收 |
|----|-----|------|------|
| 1 | **O1b** | Unix `StatusBarView` + 布局加 IME 段；poll/刷新 `ime::status()`；对齐 Win 文案风格 | 真机：微信输入法时栏上可见名；ABC 为 latin/布局名 |
| 2 | **O-fix** | 在 owning PRD（建议 `PRD_02_17` 或 control/CLI 契约段）**提及** B′ 公开命令：`set-buffer`/`load-buffer`/`show-buffer`/`list-buffers`/`delete-buffer`/`paste-buffer`（及已 shipped 别名），满足 `product_mentions` | `prd_alignment` / 相关 rhai 绿；**不**删 CLI |
| 3 | **G1**（若碰 install） | 无 signed asset → 自动 unsigned-preview + 多行警告 | `curl\|bash` 无 env 可装 macOS preview |
| 4 | **勿做** | T1 图路径；G7d 默认热切换；改 Win IME | — |

---

## 10. 粘贴失败问题树（2026-08-05 · 规划，未开工）

> 用户场景：在 **终端区** 或 **composer 输入区** 粘贴时经常失败。  
> 两类主诉均标 **硬骨头**——跨 OS clipboard + 归一策略 + UX 诊断，忌「顺手改 is_control」无夹具。

### 10.1 用户可见两类

| ID | 用户说法 | 更可能机制（待证） | 今日用户可见文案 |
|----|----------|-------------------|------------------|
| **P1** | 从别的 terminal 复制大段文字失败；疑特殊 UTF-8 / emoji | ① `from_utf8` 硬失败（非法字节）→ backend error；② 夹带 CSI/OSC/控制符归一后变空或异常；③ 超 256KiB；④ unix 异步 paste 丢 target；⑤ 焦点/模态拒绝。**Emoji 合法码点应能过 `!is_control()`**——若「只有 emoji 才挂」须另证读盘/截断路径 | `clipboard read failed: …` / `Paste failed: …` / empty 文案 / too large / focus… |
| **P2** | `clipboard text contains no pasteable characters` | 剪贴板 **无 Unicode 文本**（典型：截图/图像为主格式；或文本归一后长度为 0） | 字面量 **`clipboard text contains no pasteable characters`**（unix `TerminalPasteFailure::Empty`、composer `paste_clipboard_into_composer`、windows remote 同串） |

### 10.2 代码路径（验收时对照，非授权改点清单）

```text
粘贴入口
├─ 终端区 paste
│  ├─ Unix：request_terminal_clipboard_paste → worker get_text_bounded
│  │         → finish_terminal_clipboard_paste → normalize_terminal_paste
│  │         → terminal_paste_bytes (± bracketed) → tab.send
│  └─ Windows remote：paste_terminal_clipboard → clipboard::get_text
│            → normalize_terminal_paste →（空则 no pasteable…）
├─ Composer：paste_clipboard_into_composer → get_clipboard_text
│            → normalize_composer_paste → empty 同上文案
└─ 共享归一：src/ui_clipboard.rs
   normalize_*：CRLF 规范化；丢弃 is_control()（除 \t 与换行族）
```

平台读盘：`crates/agenterm-platform/**/clipboard.rs`（macOS `pbpaste` 字节 → `String::from_utf8` 失败即 Backend）。

### 10.3 为何「别的 harness 能粘、AgenTerm 透传不过」（2026-08-05 补）

用户观察：若干 agent/终端 harness **本身**已支持图片粘贴与复杂文本，但进 AgenTerm 后失败。  
**不是** OS 剪贴板能力不够，而是 **AgenTerm 链路只认 Unicode 纯文本**，中间被掐断：

```text
系统剪贴板（可同时有 text + html + rtf + png + …）
        │
        ▼
agenterm-platform clipboard API
  仅有：get_text / set_text / has_unicode_text
  无：get_image / 枚举 MIME / HTML·RTF
        │  ← 【断裂点 A】图像/非 text 在此不可见
        ▼
产品 normalize_*_paste（ui_clipboard.rs）
  只处理 str；丢 is_control()（除 \t/换行）
        │  ← 【断裂点 B】复杂文本控制/转义被剥；剥光 → empty
        ▼
PTY send / composer（字节或 String）
  终端：bracketed paste + UTF-8；无路径注入、OSC 图、临时文件
        │  ← 【断裂点 C】无「交给子 harness 的投递协议」
        ▼
tab 内进程（claude/codex/…）
  只吃得到父终端喂进 PTY 的字节
  父没喂图/富文本 ⇒ 子进程「会粘图」也收不到
```

| 层 | 别家 harness 常见做法 | AgenTerm 今日 |
|----|----------------------|---------------|
| 剪贴板读 | 按 UTI/MIME 取 png/html 等 | **只 get_text** |
| 图像粘贴 | temp 路径 / base64 / OSC / 内嵌 | **无通路** → empty |
| 复杂文本 | 保留或智能剥 SGR；lossy UTF-8 | 严 from_utf8 + 剥 control |
| 透传语义 | 完整用户意图给子进程 | 有界 Unicode 文本进 PTY/composer |

**推论**：子 harness 支持粘图 **≠** AgenTerm 已透传。要透传须在 A/C 增格式探测与投递；复杂文本失败多在 A+B。

| 产品选项（未拍板） | 含义 | 工作量 |
|--------------------|------|--------|
| **T0** 现状强化 | 仍只文本；图像/无文本 **显式文案**（P2/P3） | 小 |
| **T1** 图→路径 | image → temp → 插入路径字符串 | 中 |
| **T2** 真透传 | 多 MIME + 子进程协商 | 大，须 PRD |

本版 P 组默认 **T0→（可选）T1 调研**；T2 不进 v0.1.15。

### 10.4 为何硬（补充）

1. **异源语义**：他终端「复制」≠ 纯文本；常含 SGR/OSC/宽字符/非法序列。  
2. **错误折叠**：多种根因归一成 empty 或笼统 `terminal_paste_failed`，用户只能猜「emoji」。  
3. **图像 vs 文本**：图像在断裂点 A 静默不可见；应显式拒绝或走 T1。  
4. **跨端双实现**：unix embedded / win remote / composer 三入口，改一漏二。  
5. **异步与焦点**：unix worker 与 focus 竞态（StaleTarget）易被当成「偶发字符问题」。  
6. **能力错位**：子 harness 会粘图 ≠ 父终端已投递（§10.3）。

### 10.5 建议验收（开工后）

| 夹具 | 期望（建议策略） |
|------|------------------|
| 合法 UTF-8 + emoji + CJK 多行 | **成功** 粘贴进 terminal 与 composer |
| 带 `\x1b[31m` 的「假终端拷贝」 | 成功或剥 SGR 后成功；**不**误报 empty |
| 非法 UTF-8 字节序列 | 稳定 code：`clipboard_invalid_utf8` 或 lossy 成功且可观测替换 |
| 仅图像、无 text | code：`clipboard_image_only`（或 `clipboard_no_text`）；文案点明图像；**不得**静默 empty |
| 真空剪贴板 | `clipboard_empty` |
| >256KiB 文本 | too_large；文案含上限 |
| （若 T1）剪贴板 png | PTY/composer 出现可解析路径或约定标记；子进程可读该文件 |

### 10.6 非目标（本叶）

- **T2 真富文本/多 MIME 透传**（须 PRD；非 v0.1.15）  
- 默认放开任意 C0 控制进 PTY（安全与兼容风险）  
- 与 S 组微重构绑死——P 可在 GUI 域空闲时独立排期  
- 假设「子 harness 支持 ⇒ AgenTerm 已透传」（反例见 §10.3）

---

## 9. 结构微重构预备树（HOLD · 2026-08-05）

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
│  └─ 产出：一页「可开 / 让路 / 延后」三列（聊天或 §9 补记）
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
| 发布链 | `.github/workflows/*`, `scripts/rh/check*.rh` | A/B/E 组 |
| 安装更新 | `install.sh`, CLI update 相关 | G/H 组 |
| 结构闸 | `src/platform/boundary_tests.rs`, `plan/ARCHITECTURE.md` | **S 组自有** |
| 双主机 GUI | `unix/frontend/**`, `windows/remote_frontend.rs` | UX/parity 他方 |
| client | `src/client/**` | script/mcp 他方 |

---

## 7. Linux 云桌面实测意见（2026-08-04）

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

## 8. 安装与更新实测（2026-08-05，macOS aarch64）

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
| 旧 `agenterm-script` BIN 链 | ~~**断链残留**（装完后仍在，手动 `rm`）~~ → **2026-08-05 O3 复测：已不存在**。本机五个 bin 软链全部解析到真实文件，零断链；`agenterm-script` 只作为文件残留在旧 local release 载荷目录中（死重量，非断链）。**该条为会话特定状态，勿再当作现存缺陷**（G2 因此无事可做） |
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

## 12. CLI 输入面（ui-input）——已交付

### 12.1 背景：能看见，但按不下去

`ui-snapshot` 在 `projection: "embedded_gui"` 下早已输出几乎所有可点元素的像素
bounds（toolbar 各按钮、tab 行、close/new_child、disclosure_hit、滚动条
thumb/track、sidebar resize_grip、composer input），并带 `focus`/`caret`/
`anchor`/`selection`。**观察面是完整的**。

动作面却是空的：`send-mouse` 在 `commands.rs`、`control_authority.rs`、
`client/mod.rs` 共四处声明，`control_dispatch.rs` 里**零个 dispatch 分支**；
实测返回 `Unix GUI does not implement 'send-mouse' yet`。而且它的参数是
`-x col -y row`（终端单元格），本质是 tmux 的「往 PTY 写鼠标上报」，
**不可能点到工具栏按钮**。结论：agent 能看见按钮，按不下去。

### 12.2 交付内容

```
ui-input pointer --x PX --y PX [--button left|right|middle]
                 [--action press|release|move] [--count 1|2|3]
                 [--mods shift,ctrl,alt,meta]
ui-input wheel   --x PX --y PX --delta-y N [--units lines|pixels]
```

坐标与 `ui-snapshot` 同一像素空间，读了就能点，无需换算。

**关键约束**：请求被合成为真正的 `PixelWindowEvent`，走 `handle_pixel_event`
——窗口管理器喂进来的同一个入口。没有第二份 hit-test；多击是**真的发 N 次
press/release**，而不是去种 promotion 状态。理由是 composer 曾经在 Unix 有选区、
Windows 没有，正是两份实现漂移的结果。

### 12.3 实测证据（装进 `~/Applications/AgenTerm.app` 后跑）

| 手势 | 结果 |
|---|---|
| press→move→release 拖拽 | 选中 `hello wo`（anchor 0, caret 8） |
| `--count 2` | 选中单词 `hello` |
| `--count 3` | 选中整行含 CJK |
| `--count 2` 落在中文区 | 精确选中 `中文测试`（字符 12–16，非字节） |
| 读 `toolbar.tabs.bounds` → 点中心 | `tabs_visible` true → false |
| `--x NaN` / `--count 9` | 各自 typed 报错，不进 hit-test |

perceive→act 闭环成立。

### 12.4 目录登记

`ui.input.pointer` / `ui.input.wheel` 已入 `operations.rs` 与
`prd/PRD_02_15_command_line.md`（alignment 计数 69/97）。顺带补登记了四个
孤儿动词：`select-tab` / `new-child` / `toggle-tree` / `composer-send`
——它们此前只在 `control_dispatch.rs` 有实现、无 typed spec 无脚本面，
与 `delete-buffer` 是同一类漂移。

### 12.5 未决（决策项，待拍板）

1. **Windows 侧 `ui-input` 未实现**。解析层 `frontend/pointer_input.rs` 是平台中立的，
   可直接复用；但 Windows 的 composer 是原生 `EDIT` 控件，合成 press 送到窗口
   **不会**像 Unix 自绘 composer 那样落到输入框，需要单独决定怎么处理。
   已在 `remote_frontend.rs` 的 `fn event` 上留 `REVIEW(macos → windows owner)`。
2. **headless 跑不了闭环**。`server_app.rs:1908` 的 headless 快照硬编码
   `composer.visible:false`、`focus.surface:null` 且不带几何；macOS/Linux 的
   `screenshot` 还需要活着的渲染进程。所以 9.3 那套验证目前**只能在有窗口的会话里跑，
   CI headless 跑不了**。要不要让 headless 也供几何，是架构取舍。

## 10. 整体 review（2026-08-06 下午）

### 10.1 集成测试首次全绿

`cargo test --test rhai_migration` 从长期 2 红变成 **22/22 全绿**。这两盏灯挂了
很多轮没人认领，查下来**都不是 bug，是断言过期**：

| 测试 | 真因 |
|---|---|
| `artifact_manifest_...` | `78357dd` 删掉了 `agenterm-server.exe`（authority 改成 `agenterm server` 子命令），`scripts/artifacts.json` 从 7 个可执行文件变 6 个，但测试里 pin 的 "defines 7" 没跟着改 |
| `child_id_remains_public_...` | 断言 `window_supported == cfg!(windows)`，但 macOS 现在**真的实现了** process-window 自动化（`adapters/macos/process_window.rs` 返回 `supported: true`），是断言在描述一个已经过时的平台切分 |

第二条改成 `cfg!(any(windows, target_os = "macos"))` 而不是直接删检查 ——
Linux 还没有 adapter，这个缺口应该继续可见。

### 10.2 输入区「选中后打字不替换」——已修

**发现路径值得记一下**：clippy 报 `text_selection::insert` 从 `c5b31ee` 起就是
死代码。追下去发现这个函数本来就是为「打字替换选区」写的，只是从没接上。
拿真机验证确认了缺陷：选中 `hello` 打 `X`，草稿纹丝不动。

**为什么不是简单接一下**：共享 key 路径只有 `select_all: bool`，表达不了 range，
而且总是往草稿**末尾**追加。所以第一版「先删选区再交给共享路径」的做法，把
`hello world` 选中 `hello` 打 X 变成了 `" worldX"` —— 字符跑到末尾去了。
正解是在 frontend 里用 `text_selection::insert` **原地替换并吃掉整个按键**。

只有文本 / Space / Backspace / Delete 触发替换；方向键、Escape、
primary-shortcut 组合键（复制、全选）必须移动或执行而不毁草稿。

### 10.3 新增 `ui-input key`

上面那个缺陷**当时无法从 CLI 验证** —— `send-keys` 打的是 pane，往 PTY 写字节，
永远到不了 composer 的按键处理。所以补了：

```
ui-input key --key NAME [--mods shift,ctrl,alt,meta]
```

命名键大小写不敏感（`Enter`/`esc`/`ArrowDown`…），其余按字面文本处理。
同样走 `handle_pixel_event`，合成 press+release 一对完整按键。

至此「人类能做的 CLI 都能做」这条线上，**鼠标和键盘都通了**。

### 10.4 Win agent 的 ui-action catalog：问题已闭环

我上轮报的「24 个 action 被误标成 WINDOWS_ONLY」，对方在 `531032c` / `6b3e80d`
里修完了：SHARED 从 16 → **44**，WINDOWS_ONLY 从 41 → **14**，
重新量过**误标数为 0**。这个协作回路是有效的。

### 10.5 仍未决 / 待认领

1. **`install.sh:157` 仍硬性要求 5 个可执行文件**。mux/mcp 子命令已经能用且
   输出逐字节一致，但要真省下 1.6 MB 得允许不装那两个二进制 —— 这会破坏现有
   `agenterm-cli mux` 调用方。**已拍板（2026-08-06）**：不保留独立
   `agenterm-mux` / `agenterm-mcp` PE，仅 CLI 子命令。
2. **Windows 侧 `ui-input` 未实现**（pointer 和 key 都是）。解析层平台中立可直接
   复用；难点是 Windows 的 composer 是原生 `EDIT` 控件，合成 press 送到窗口不会
   像 Unix 自绘 composer 那样落进输入框。已在 `remote_frontend.rs` 留
   `REVIEW(macos → windows owner)`。
3. **headless 跑不了闭环**（`server_app.rs:1908` 不带几何）。要不要让 headless
   供几何是架构取舍。
4. **Linux 缺口**：`process_window` 无 adapter；`linux/font.rs` 硬编码 Debian 路径
   无 fontconfig，非 Debian 系发行版会静默掉到 8x8 点阵字体。
5. 去中心化网络选型见 `plan/research-decentralized-network.md`（**建议等 CC
   产品形态清楚再动**）。

## 11. OSX ↔ Win UI/UX 差距清单（2026-08-06 晚）

### 11.1 测试基线

`cargo test --lib` **691 绿**。集成套件修好 2 盏因 `7b930b9`（删掉 mux/mcp PE）
而过期的计数 pin；仍红 2 盏，**实测与本轮改动无关**（把改动 stash 掉照样红）：
`linux_package`（缺 `dist/agenterm-sbom.spdx.json`）和 `supply_chain`
（`supply_chain_notice_count`）——都是发布链的，**需要有人认领**。

### 11.2 关键发现：状态机早就是共享的，缺的是 Unix 侧的「画」和「接线」

这轮最重要的结论。逐项实机探过 14 个 `WINDOWS_ONLY_UI_ACTIONS`，
**macOS 上全部返回 unknown**，是真缺；但源码看下来：

| 共享模块 | 行数 | Win 引用 | Unix 引用 |
|---|---|---|---|
| `src/frontend/settings.rs`（`SettingsScope`/`AppearanceField`） | 555 | 有 | **0** |
| `src/frontend/instance_picker.rs` | 284 | 79 | **1**（只有报错） |
| `src/frontend/server_strip_ui.rs` | 98 | 1 | **0** |

也就是说**难的部分（状态机）已经在共享层写好了**，Unix 缺的是模态渲染、
命中测试和 ui-action 接线。这直接影响排期估算——不是「重写一遍」。

### 11.3 建议的 osx 对齐顺序

| 优先级 | 项 | 用户视角缺什么 | 规模 | 理由 |
|---|---|---|---|---|
| **1** | Settings 作用域 + 逐字段继承 + reset-overrides（6 个 action） | macOS 用户**无法只改当前终端的外观**，只能改全局；也看不到某个终端的字体/字号/主题是继承还是覆写 | **小–中** | 状态机全在 `settings.rs`，只差一个作用域选择器 UI + 6 个 dispatch 分支。性价比最高 |
| **2** | Instance picker（6 个 action） | 现在直接给用户看 `"instance picker is Windows-first in this build"` 这句错误 | **大**（纯渲染） | 模型已共享，但 Unix 没有这个模态的渲染骨架 |
| **3** | Server strip + 右键菜单 + 新建服务器对话框 | 整个「一眼看到多个 server 实例并切换」的 UX 在 macOS 完全没有 | **大** | 几何计算已在 `ui_geometry.rs` 共享（`server_strip_height`），但绘制/输入要从零写 |
| **4** | `open-instance`（在新窗口打开实例） | 没有它，2 和 3 是死路 | **中** | 缺 Unix 侧的「按实例拉起新 GUI 进程」helper |
| **5** | macOS 原生菜单 | Unix 只算出 `system_menu_json` 喂给自动化，**没有接到真的 macOS 菜单栏** | **未定** | macOS 习惯是应用级菜单栏而非 Win 的窗口系统菜单，**不能照抄**，建议先做 scoping spike |

### 11.4 反向：Unix 有而 Windows 没有的

- **工作区自动保存**（`unix/frontend/mod.rs:3860-3910`，约每秒 + 关闭时持久化 tab 树/草稿）。
  `remote_frontend.rs` 里找不到对应调用。但这是架构差异——Windows 那边是瘦客户端，
  持久化可能在 server 侧，**未确认**，需要 Windows owner 确认后再说是不是缺陷。
- New-Terminal 对话框的字段级 action（`shell-*`、`new-terminal-set-*`）在 Unix 是
  一等 ui-action，Windows 用原生控件驱动同一个共享 `NewTerminalDialog`。
  **不是 Windows 用户的功能缺失**，是驱动方式不同。

### 11.5 决策项

优先级 1 我可以直接开工（改动小、状态机现成、用户可感知）。
2/3/4 是「要不要把 Windows-first 的多实例 UX 补到 macOS」的**产品排期问题**，
5 需要先定 macOS 菜单的形态——这三类都**等你拍板**，我不自己定。

## 12. OSX 对齐 Win：优先级 1 已交付（2026-08-06 深夜）

### 12.1 Settings 逐终端外观 —— 已上线

macOS 用户以前**只能改全局外观**：Settings 没有作用域切换，无法给某个终端单独
设字体/字号/主题，也看不到某个字段是继承还是覆写。Windows 从共享对话框写出来
那天起就有。

三个缺陷层层遮掩，逐个揭开：

1. `open_settings` 传 `target_tab_id: None`，而 `switch_scope` **在没有目标终端时
   一律拒绝** —— 所以 Current Terminal 作用域根本进不去，尽管状态机支持。
2. 六个 ui-action **哪里都没有 dispatch 分支**。我加在**共享 `control_dispatch`**
   而不是 Unix adapter 里，这样两端共用一份实现，也堵死了将来冒出第三份的路。
3. `close_settings` 只应用了 `changes.default_appearance`，**丢掉了
   `changes.override_draft`** —— 即使作用域切过去了，Apply 也会静默吃掉用户的修改。

**状态机本身（`src/frontend/settings.rs` 的 `SettingsScope`/`AppearanceField`/
`switch_scope`/`toggle_inheritance`/`reset_overrides`）一行没改** —— Unix 只是
从来没调用过它。这印证了 §11.2 的判断。

实机验证：开 → `scope=defaults, target=@5` → `settings-current` →
`scope=current-terminal` → toggle + apply → `current_terminal_override =
{appearance_preset: "classic-night"}` → reset + apply → `None`。

catalog：SHARED **44 → 50**，WINDOWS_ONLY **14 → 8**。

### 12.2 顺手修掉一个会静默失败的安装事故

`install.sh` 被 `6b7ea4d` 改成了**全文 601 行 CRLF**，在 macOS/Linux 上直接死在
shebang：

```
env: bash\r: No such file or directory
```

**而且丢弃输出时它是静默失败的** —— 我自己就为此丢过一次构建，直到发现装进去的
镜像比 `target/release` 老了几个钟头才察觉。凡是 `./install.sh ... >/dev/null`
的调用都会「看起来成功、实际没装」。

`.gitattributes` 本来就把 `Cargo.lock` 和 JSON 清单钉成 LF，但**没有 shell 脚本
规则**，已补 `*.sh text eol=lf`。install.sh 是唯一受影响的脚本。

### 12.3 剩下的 8 个 WINDOWS_ONLY 全是多实例 UI

`instance-picker-*`（5 个）、`open-instance-picker`、`open-instance`、
`select-server-tab`。对应 §11.3 的优先级 2/3/4，**都是「要不要把 Windows-first
的多实例 UX 补到 macOS」这一个产品排期问题**，等拍板。
