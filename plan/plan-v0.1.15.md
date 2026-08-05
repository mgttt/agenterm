# AgenTerm v0.1.15 公开计划

状态：**已定稿，待授权开工**（2026-08-05 定版；此前为占位稿/思维工作树，
素材保留在 §三·五 / §五 / §七 / §八 / §九 / §十）。
不改变任何已发布/在途版本的授权状态；不创建 tag/Candidate/Release。

**主题：发布链降本（cache 优先）+ 交付后 install 卫生。**
比占位稿的「反馈左移 + 发布链降本」**更窄**：反馈左移只保留最便宜的两叶，
夜间彩排与自动派发推 v0.2.x——理由基于实测数字，见 §二。

开工前需人工拍板 §五 5.7 的政策决策项（阻塞关系见 §二·五）。

## 数据来源与关键事实（全部实测，可复现）

v0.1.14 发布日 ~10 轮 gate 级遥测，加 2026-08-05 对成功 Candidate
`30942173420` 的逐门/逐 job 分析（详见 `plan/plan-v0.1.14.md` §七）：

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

## 一、目标树素材全集（**非执行清单**——执行看 §一·五）

> 本节保留占位稿的 A–H + P + S 全部原始条目与 review 行，作为**素材与依据**。
> 取舍结果见 §一·五；未纳入本版者的推迟理由见 §二·六。

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
│        建议 cron 保留 14 天；runbook 素材见 `prd/PRD_02_17_delivery_quality.md`
│        §Release-chain operating requirements（v0.1.13/v0.1.14 两轮坑
│        已合并去重为版本无关要求）
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
├─ P. 粘贴失败硬骨头（终端区 + 输入区/composer；2026-08-05 用户实测，详见 §十）
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

## 一·五、v0.1.15 收敛工作树（**这是可执行清单**；上面 §一 是素材全集）

§一 的 A–H + P + S 共约 30 叶，是多轮追加堆出来的，含大量「观察」而非
「可执行」。本节是取舍后的定稿：**只列进入 v0.1.15 的叶**，每叶带动机、
可证伪验收、成本、依赖。未列入者一律见 §二·六（推迟表，含推迟理由）。

选择原则（v0.1.14 教训）：**宁可少而全绿，不要多而半途**——发布日 5–6 小时
耗在从未跑过的车道上，根因不是做得少，是同时开了太多没验证的面。

### R. 发布链降本（本版第一优先；全部有实测收益）

- [ ] **R1 cache 配额治理** ★最高性价比
  - **动机**：9.9/10GB 撞顶 → LRU 驱逐 → Candidate cache 每轮全 miss，
    bootstrap 47s→81s **单调恶化**（见头部实测块）
  - **做法**：CI 的 debug target cache 限制保留份数或缩小缓存路径；
    必要时给 candidate 车道独立前缀，确保关键路径不被挤掉
  - **验收（可证伪）**：连续两次 Candidate 的 timing artifact 中
    `bootstrap.worker.state == "reused"`（当前恒为 `"rebuilt"`）；
    且 `gh api .../actions/caches` 总量 < 8GB
  - **成本**：小（改 workflow cache 配置 + 一次清理）
  - **收益**：≈3min/次 Candidate；**依赖**：无
- [ ] **R2 `cargo-home-candidate-v2` 补 `restore-keys`**
  - **动机**：它只有 `key` 无 `restore-keys`（对照 `cargo-target-v2` 有），
    hashFiles 一变即彻底 miss、无近似回退
  - **验收**：版本冻结提交后首轮 Candidate 日志出现前缀命中，
    而非 `Cache not found`
  - **成本**：极小（一行）；**依赖**：R1 先腾配额，否则命中也会被驱逐
- [ ] **R3 net-research 移出 release 门**（原 B1）
  - **动机**：142.2s／16.4%，耗时第二名，却与发布产物正确性关系最弱
  - **做法**：改为 push CI 跑一次；**不是删除**——保留验证，只换车道
  - **验收**：release 门不再含该 gate 且 push CI 含之；
    `qualification-gates.json` 声明同步（fail-closed 不破）
  - **成本**：小；**依赖**：无
  - **PRD 核对**：已 grep，无任何 PRD 要求它必须在 release 门（见 §二·七）
- [ ] **R4 promotion dry-run**（新增叶，v0.1.14 直接教训）
  - **动机**：`release.yml` 首跑即藏 4 个缺陷；dry-run 可在几十秒内
    暴露其中 4/8
  - **做法**：加 `-f dry_run=true`，跑完 verify 全部断言但不建 tag/release
  - **验收**：`dry_run=true` 跑完 verify 且仓库无新 tag、无新 draft
  - **成本**：中；**依赖**：无
  - ⚠️ **本叶自身就是「没跑过的车道」**，必须先自证，别重蹈覆辙

### A′. 反馈左移（只保留最便宜的两叶；A1/A2 推迟见 §二·六）

- [ ] **A3 script-smoke 左移进 push CI**（debug 版，实测 ~7s）
  - **动机**：v0.1.14 发布日它贡献 2 次腐化；左移后 6 分钟内暴露
  - **做法**：并入 `94c3227` 已建的 windows CI release-lane-smokes 步骤
  - **验收**：push CI 含 script-smoke 且 CI 总时长增幅 < 30s
  - **成本**：极小；**依赖**：无
- [ ] **A4 per-gate timing 写进 `GITHUB_STEP_SUMMARY`**
  - **动机**：现在要下载 artifact 才能看每门耗时；R1 的验收也依赖它可读
  - **验收**：Candidate 运行页直接可见逐门耗时表，无需下载 artifact
  - **成本**：小；**依赖**：无（但先于 R1 做更好验证）

### G′. 安装/更新卫生（用户真机踩坑，详见 §八；与发布链正交，可并行）

- [ ] **G3 版本可观测性**：`agenterm --version` 打印即退 + 写 `installed.json`
  - **动机**：用户/agent 无法确认「窗口是不是旧版」；G7a 的判断也依赖它
  - **验收**：GUI 二进制 `--version` 不启窗口即输出版本；
    `current/installed.json` 含 version/channel/variant/source_commit/
    sha256/installed_at
  - **成本**：小；**依赖**：无（是 G7a/H3 的前置）
- [ ] **G2 升级后孤儿 symlink 清理**
  - **动机**：实测 `~/.local/bin/agenterm-script` 断链残留（改名后遗留）
  - **验收**：装完后 BIN 下无「指向 current/releases 但目标不存在」的链
  - **成本**：小；**依赖**：无
- [ ] **G7a 升级后自适应文案**（install 收尾）
  - **动机**：用户点名——磁盘已 0.1.14、running server 仍 0.1.12，
    关窗默认 keep-server → 再进仍旧版，用户无从得知该怎么做
  - **验收**：探测到 live server 且版本低于已装时，收尾文案明确给出
    「要启用新版该做什么」；**用户无需读文档**
  - **成本**：小；**依赖**：G3
  - **注**：纯文案，**不受 G-P2 阻塞**（G7b/c/d 才受）
- [ ] **G6 releases 目录保留策略**（current + N，默认 2）
  - **动机**：`0.1.11-local` / `0.1.12-local` 永久堆积
  - **验收**：装新版后旧目录按策略修剪，且不删仍被非 current 链引用者
  - **成本**：小；**依赖**：无

### H′. 分发面地基（只做**纯派生 + 补值**，不建服务；详见 §一 H 组）

- [ ] **H4 补齐 Linux/Windows 的 `provenance.sbom_sha256`** ★先做
  - **动机（已修正，见 §二·七）**：实测六平台——**macOS 两个 arch 已正确填入**
    `65c32add…`，**Linux 两个为空串、Windows x86_64 缺字段、
    windows-aarch64 为空串**。`PRD_02_17:237-240` 只要求「macOS 双档
    provenance 携带同一 SBOM 摘要」，故**当前实现并未违反 PRD**；
    本叶是**把该保证扩展到全部六平台**，因为 M14 Hub 信任分级要对
    所有平台复用这个字段
  - **验收**：新 Candidate 六平台 provenance 的 `sbom_sha256` 均 ==
    实际 SBOM 摘要（windows-x86_64 从「无此字段」变为有值）
  - **成本**：极小（纯补值）；**依赖**：无
  - **PRD 联动**：落地后应同步把 `PRD_02_17:237-240` 的 macOS 限定
    升级为六平台（见 §二·七 建议 2）
- [ ] **H1 生成 `releases.json`**（CI 静态产物，纯派生）
  - **动机**：install.sh 靠字符串拼 artifact 名 + latest 重定向猜版本；
    未来 update/下载页/Hub 会各自再 scrape 一遍 → 四个真相源
  - **验收**：release 成功后 Pages 上有 `releases.json`，字段全部可由
    现有 provenance/candidate-manifest 派生（**不新造事实**）
  - **成本**：中；**依赖**：H4（sbom 摘要要能写进索引）
- [ ] **H3 provenance 用户可见化 + `installed.json`**
  - **动机**：`.provenance.json` 每包都发但用户端零消费（install.sh 只校 sha256）
  - **验收**：install 收尾打印 commit/tag/build_log/signed/notarized，
    且校验 provenance 与实测摘要一致；写入 `installed.json`
  - **成本**：中；**依赖**：G3（共用 installed.json）、H1（读索引取 variant）


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
    （WM_IME_* → 内联 preedit，见 §三·五 3.5.3 I1）；本叶补的是
    **Unix 侧的状态读取**。两者在 facade 的不同侧，互不触碰对方文件。
  - **若工期紧**：可只做 macOS 档（Linux 留 stub 并注明），仍然兑现
    「三平台平权」的一半，且我方能真机验证

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
  - 执行计划：[`plan/plan-skins-v1.md`](plan-skins-v1.md)
  - **与 §五 5.4 L-EXT 的关系**：这是**内置**皮肤，外部 SkinHub 包仍归
    M14／v0.2.x——即 P6（Hub 单一 kind 底座）**未被本次落地预判**
- [x] **X2 Windows IME 内联合成 + 协议兼容 UX** — 已落地 `83843ea`
  - 内容：见 §三·五 3.5.3（I1 候选条锚点／I2 ui-hello 版本分类 + 原生
    MessageBox，新增 platform `alert` 能力）
  - 证据：607 lib tests 绿 + `incompatible_ui_contract_names_the_stale_side`
  - 待办：两项均**待真机回归**（中文输入、MessageBox 路径）
- [ ] **X3 Control Center UX 设计** — 设计中，**明确归 v0.2.0**
  - [`plan/plan-control-center-ux.md`](plan-control-center-ux.md) 标题即
    「L-CC · v0.2.0」，不占本版工期；本条仅登记指针

> **规模影响**：X1+X2 已消耗的工期不小（约 2900 行入 main）。若把它们计入，
> v0.1.15 实际范围已**超过**我在 §二 主张的「窄」。这不改变 R 组的排序理由
> （cache 仍是最便宜的杠杆），但**应当据此更保守地对待 H1/H3**——
> 见 §二·二 序 7 的「工期紧则优先砍」已预留该出口。

**规模自查（2026-08-05 补记后）**：

| 泳道 | 叶数 | 性质 | 状态 |
|------|-----|------|------|
| R / A′ / G′ / H′ | 13 | 修补 · 降本 · 地基 | 待授权开工 |
| **N** | **1** | **新功能** | 待授权开工 |
| X（并发 agent） | 2 已完成 + 1 归 v0.2.0 | 功能 | 约 2900 行已入 main |

对照 v0.1.14 的教训（发布日 5–6 小时耗在从未跑过的车道上），14 叶宽度本身
可控；但**加上 X 组已落地部分，本版实际范围已不算窄**。因此若工期吃紧，
砍叶顺序：**H1/H3 → R4 → N1 的 Linux 档**，绝不砍 R1/R2。

### 为什么 v0.1.15 不推进 L-NET（ipfs/libp2p）

用户 2026-08-05 原话：本想督促 ipfs/libp2p 功能，但认同「先把底子弄好」——
多平台 UI/UX 对齐、稳定性增强、功能补丁优先。这个判断与实测证据一致：

- **L-NET 的下一关不是写代码，是定形态**。§5.2.1 实查表明 research spike
  已自证完备（进程隔离／CID／block store 全绿，每轮 release 门真跑 142s），
  但 `src/` **零 import**——卡点是 N3「产品消费者以什么形态存在」
  （Script API？InfoHub？CC 诊断？），那是**拍板题不是工程题**。
  在形态未定前投工程，做出来的接口大概率要返工。
- **底子确实欠账**：N1 揭示 `ImeStatus` 契约只有 Windows 实现、Unix 两档全是
  stub；§八 实测的安装/升级体验有 G2/G3/G6/G7a 四处硬伤；cache 撞顶正在
  单调恶化。这些都是**用户每天碰得到**的，而 L-NET 目前无人使用。
- **结论**：v0.1.15 做底子，L-NET 保持 research 车道（R3 只是把它从 release
  门移到 push CI，**验证不减**）。待 N3 形态拍板后，L-NET 作为 v0.2.0 主线开工。

## 二、排序与理由（**基于实测数字，非直觉**）

### 二·一 为什么主题从「反馈左移」改为「发布链降本（cache 优先）」

占位稿把 A1（夜间彩排）排第一，理由是「腐化攒到发布日爆雷」。这个判断
**方向对但排序错**，因为当时还没有 §七 的逐门/cache 实测。三点修正：

1. **A1 成本远高于收益密度**。夜间 release-stress 每晚 ~1 runner-hour，
   且 win-full-gate 的 concurrency group 是 `win-full-gate-{ref}` +
   `cancel-in-progress: true`——同 ref 连跑会互相 cancel，落地前还得先改
   并发语义（§一 A1 已核）。**投入是本版最大的一项，收益是概率性的**。
2. **R1（cache）投入最小、收益确定且可证伪**。9.9/10GB 撞顶是**已复验的
   常态**，bootstrap 47s→81s 是**单调恶化**的实测曲线。改 cache 配置属
   配置级改动，收益 ≈3min/次 Candidate，按 v0.1.14 的 6 次 Candidate 计
   约省 18min ——**且它同时止住恶化趋势**，不做的话下一版更贵。
3. **A3/A4 才是「反馈左移」里真正便宜的部分**。A3 实测 ~7s、A4 是纯输出改动，
   两者合计成本远低于 A1，却覆盖了「腐化早暴露」的主要价值。

> 结论：反馈左移的**思想**保留（A3/A4 + R4 dry-run 都是它的实现），
> 但**最贵的实现方式（A1 夜间彩排）推迟**。主题相应改为「发布链降本」。

### 二·二 执行顺序（建议）

| 序 | 叶 | 理由 |
|----|-----|------|
| 1 | **R1 → R2** | 最便宜、收益确定、且在恶化；R2 依赖 R1 腾出的配额 |
| 2 | **A4** | 让 R1 的收益可直接在运行页读出（验收工具先于验收对象） |
| 3 | **R3、A3** | 各自独立、成本小，可并行 |
| 4 | **H4** | 纯补值、零依赖，且是 H1 的前置 |
| 5 | **G3 → G7a、G2、G6** | 安装卫生泳道，与发布链正交，可与 1–4 并行 |
| 6 | **R4** | 中等成本，且**自身就是没跑过的车道**，放在链路稳定后做 |
| 7 | **H1 → H3** | 本版最大的两叶；若工期紧，这两叶优先砍 |

### 二·三 明确不做速度优化的部分

- **gate 分片**（39 门串行 869s，理论可压到 7–9min）：收益最大，但要重排
  windows job 结构，属结构性改动，**推 v0.2.x**——本版不碰关键路径结构。
- **artifact-build / artifact-build-fast 合并**（合计 339s / 39%）：
  已核 release-fast = release + lto=false + codegen-units=16 + incremental，
  产物不可互换；真省法是共享增量缓存，而那正是 R1 的副产品——
  **先做 R1 再测命中率**，不单独立叶。
- **smoke 并行分片**（原 D2）：14 门合计仅 124.4s，现值低，维持不做。

## 二·四 与 v0.1.14 教训的对应

| v0.1.14 教训 | 本版对应叶 |
|--------------|-----------|
| release.yml 首跑藏 4 个缺陷（「没跑过」≠「没问题」） | **R4** dry-run |
| 腐化在最贵车道才暴露 | **A3**（左移）+ **A4**（可见） |
| bootstrap 恒 rebuilt、cache 全 miss | **R1 + R2** |
| provenance 有字段没填、用户端零消费 | **H4 + H3** |
| 升级后不知道要 stop-server | **G3 + G7a** |

## 二·五 决策项阻塞关系（**需人工拍板，agent 不自主执行**）

政策项全文见 §五 5.7；此处只列**它阻塞了本版哪些叶**：

| 决策项 | 阻塞的叶 | 不拍板的后果 |
|--------|---------|-------------|
| **P1 / P5**（agenterm.work 归属） | H5（本版未纳入）、间接影响 H1 的托管位置 | H1 仍可做（产物发到现有 Pages），但落地 URL 待定 |
| **G-P1**（macOS unsigned 是否默认通道） | G1（本版未纳入） | 不阻塞已纳入的 G2/G3/G6/G7a |
| **G-P2**（升级遇 running server 的默认策略） | G7b/c/d；**G7a 不受阻**（纯文案） | 只做 G7a 即可交付主要价值 |
| **D1**（preflight 放宽 HEAD 约束） | 本版无叶依赖 | 不阻塞；但若拍板通过会弱化 D3 |
| **P6**（Hub 是否单一 kind 底座） | 本版无叶依赖（H 组只做地基） | 不阻塞 v0.1.15 |
| **P-P1**（粘贴 lossy vs 硬失败） | P 组全部（本版未纳入） | 不阻塞 |

> **本版可在零决策拍板的情况下启动**：R 组 + A3/A4 + H4 + G2/G3/G6/G7a
> 全部不依赖任何政策项。这是刻意的——不让计划卡在等拍板上。

## 二·六 推迟表（**明确不进 v0.1.15，含理由**）

| 叶 | 推去 | 理由 |
|----|------|------|
| A1 夜间彩排 | v0.2.x | 本版最贵一项（~1 runner-hour/晚）且需先改 concurrency 语义；收益概率性 |
| A2 Candidate 自动派发 | v0.2.x | 触发器分钟级延迟 + 授权链敏感；D1 未拍板前不动 |
| B2 cache key 版本行归一化 | v0.2.x | 需六 workflow 共享算 key 脚本，一致性维护成本高；R1 已拿走大部分收益 |
| B3 双构建复用审计 | 合入 R1 | 已核产物不可互换；真省法是 R1 的增量缓存副产品 |
| C1–C4 竞态收口 | v0.2.x | 均已止血，剩根因排查；C4 明确说了要先观察复发率 |
| D1–D3 政策 | 等拍板 | 见 §二·五 |
| E1 Pages 噪音 | 等 P1 | 与域名归属绑定，先拍板再动 |
| E2 旧 run 清理 | v0.2.x | 纯卫生，无阻塞；moltbaby 已有脚本可随时搬 |
| F1/F2 云桌面快照 | 环境维护 | **不走 PR**——是环境快照尾账，不是代码叶（见 §七） |
| G1 macOS 默认回落 | 等 G-P1 | 政策未定 |
| G4/G5 | v0.2.x | G7a 已覆盖主要价值；G5 是 G7a 的锦上添花 |
| G7b/c/d | 等 G-P2 | 碰 keep-server 默认语义，须人工拍板 |
| H2 install.sh 消费 releases.json | v0.2.x | 依赖 H1 落地并稳定一版后再改消费端 |
| H5 agenterm.work 接通 | 等 P1/P5 | 政策未定 |
| P 组（粘贴） | v0.2.x | 用户高频但与本版主题正交；且 P1 需跨平台夹具，工作量不小 |
| S 组（结构 SSOT） | **HOLD** | 多 agent 在途，用户通知后先复审再开工（见 §九） |
| §三·五 UI/UX 观察 | 分散 | T2/SB1/W1 标「顺手做」，其余归 v0.2.0+；本版不单独排期 |
| §五 五条主线 | 各自版本 | L-NET/L-CC/L-EXT/L-PKG/L-CU 只做对齐记录与决策项 |

## 二·七 PRD 一致性核对（2026-08-05，逐叶 grep 实测）

对本版每一叶反查 `PRD.md` 与 `prd/*.md`，找**契约冲突**而非措辞差异。
结论：**一处需修正的是 plan 侧（已改），一处建议反向升级 PRD**。

| 叶 | PRD 侧相关条款 | 判定 |
|----|---------------|------|
| R1/R2 cache | `PRD_02_17:241-243`「Cache miss/corruption 只影响速度，不影响资格」 | ✅ **一致**。R1 纯提速，不碰资格语义 |
| R3 net-research 移出 | 全仓 grep：**无任何 PRD 要求它在 release 门**；唯一提及是 `PRD_02_19:562` 的二进制预算 | ✅ **无冲突**。且符合 §三「门的迁移要说明验证去哪了」 |
| R4 dry-run | `PRD_02_17:193-199` 已写「非发布彩排从未记录…dry-run 能力提为 v0.1.15 项」 | ✅ **PRD 已预留**，本叶正是它的落地 |
| A3/A4 | 无相关契约 | ✅ 无冲突 |
| G3 `--version` | `README:144` 记载 `agenterm-cc.exe` 已有 `--version` 信息命令；无 PRD 禁止 GUI 同样支持 | ✅ **有先例**，不冲突 |
| G2/G6/G7a | 无相关契约（属 install 脚本行为） | ✅ 无冲突 |
| H1 releases.json | `PRD_02_18` M13 已写入「machine-readable `releases.json` derived from existing provenance」 | ✅ **PRD 已归口**，本叶是其第一步 |
| H3 provenance 可见化 | `PRD_02_18` M13「supply-chain evidence becomes user-visible rather than CI-only」 | ✅ 一致 |
| **H4 sbom_sha256** | **`PRD_02_17:237-240`：Candidate aggregation 独立校验「两个 macOS archive provenance 携带同一 SBOM SHA-256」** | ⚠️ **曾误判，已修正** |

### 唯一的实质分歧：H4

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


### 3.5.3 Windows IME 与协议兼容 UX（2026-08-05 落地，随 v0.1.15 开发）

用户实测缺陷（2026-08-04/05）：终端区中文输入法候选条跟随光标但有
恒定偏移（约 3-4 个汉字，即合成串宽度）；且担心「新版 GUI 连旧版 server」
时只有 cryptic 报错。本轮两处落地：

| # | 改动 | 证据/验证 |
|---|------|----------|
| I1 | Windows 平台适配器在 WM_IME_START/COMPOSITION/ENDCOMPOSITION 时缓存合成串（GCS_COMPSTR + GCS_CURSORPOS）；GUI 在光标处内联渲染合成面板（镜像 Unix frontend preedit），并抑制 IME 自带浮动合成窗（WM_IME_SETCONTEXT 清除 IS_SHOWUICOMPOSITIONWINDOW），候选条锚点保持在光标 | cargo check / clippy -D warnings / 607 lib tests 绿；待真机中文输入回归（AGENTERM_IME_DEBUG=1 + PLATFORM_IME_DEBUG=1 落盘 %TEMP%） |
| I2 | `ui-hello` 拒绝时按 ClientTooOld/ClientTooNew 分类并带双方版本号生成可操作错误；GUI 启动失败与 launcher handoff 被拒时弹原生 MessageBox（新增 agenterm-platform `alert` 能力，走 selected/adapters 边界，product-neutral） | `incompatible_ui_contract_names_the_stale_side` 单测 + 607 lib 全绿；MessageBox 路径待真机验证 |

非目标：不改变 ui-bridge 协议版本（仍为 1）；不自动杀旧 server（保留用户
终端会话），错误文案明确指引用户重启/退出旧版。

## 四、与其它文档的关系

| 文档 | 关系 |
|------|------|
| `plan/plan-v0.1.14.md` | 上一版执行记录；本文数据与止血项的出处 |
| `prd/PRD_02_17_delivery_quality.md` §Release-chain operating requirements | 发布链坑清单权威处（v0.1.13/v0.1.14 两轮合并去重，版本无关；runbook 素材，E2 配套） |
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

- **UX 设计 SSOT**：[`plan/plan-control-center-ux.md`](plan-control-center-ux.md)
  （Tab/布局线框、分阶段交付、设计师清单；2026-08-05 开工）。
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
| P-P1 | 非法 UTF-8：lossy 还是硬失败；图像粘贴是否永远拒绝（建议 hard-deny 文案） | 决定 P1/P2 默认策略与错误码集合 |
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
| 2026-08-05 | 用户报告终端/输入区粘贴常失败：异源 UTF-8 大段（疑 emoji/控制符）+ 截图类 `no pasteable characters`；写入 **§一 P 组 + §十**（硬骨头，未授权开工） |
| 2026-08-05 | 用户补：多 harness 已支持图/复杂文本却透传不过 → §10.3 断裂点 A/B/C（text-only API + 归一 + 无投递协议）；T0/T1/T2 选项 |
| 2026-08-05 | **定稿**：主题由「反馈左移 + 发布链降本」收窄为「发布链降本（cache 优先）+ install 卫生」；依据 §七 实测——cache 撞 10GB 顶致 bootstrap 47s→81s 单调恶化，治理成本最小收益确定（≈3min/次 Candidate），而 A1 夜间彩排是本版最贵项且收益概率性 → A1/A2 推 v0.2.x，保留 A3/A4 |
| 2026-08-05 | 定稿产出 §一·五 收敛工作树（13 叶，含动机/可证伪验收/成本/依赖）、§二 排序理由、§二·五 决策阻塞关系、§二·六 推迟表（含理由）、§二·七 PRD 一致性核对 |
| 2026-08-05 | **PRD 核对纠错**：H4 原称「sbom_sha256 空串违反声明」不成立——逐平台实测 macOS 两档已填、Linux/Windows 未填，而 `PRD_02_17:237-240` 只要求 macOS 双档，故当前实现合规。H4 改为「把该保证扩展到六平台」；PRD 侧待 H4 落地后再升级为六平台描述（先实现后改契约） |
| 2026-08-05 | **定稿后补记**：`fe51c7c` 合并带入并发 agent 的内置皮肤 v1（四预设，约 1600 行，`prd/PRD_02_06` §Built-in skins 已立契约）与 Windows IME/协议兼容 UX（见 §三·五 3.5.3）。二者已入 main 但**不在本次规划的 13 叶内** → 新增 §一·五 X 组登记，并据此调整规模自查：实际范围已不算窄，工期紧时的砍叶顺序定为 H1/H3 → R4，绝不砍 R1/R2。Control Center UX 明确归 v0.2.0，不占本版工期 |
| 2026-08-05 | 用户指出两点：(1) Windows agent 在修其 IME，osx 侧「要有自己的思路，这才是封装的意义」；(2) 工作树全是补丁，问「哪些是新开工的功能」。自查属实——13 叶无一新功能。实证核查发现 `ImeStatus` 契约仅 Windows 实现（286 行），macOS/Linux 各 30 行 stub 恒返回 None，状态栏在 Unix 侧永远 `IME: off` → 新增 **N 组 / N1** 补齐，并本机实测 TIS API 可行（`TISCopyCurrentKeyboardInputSource` 读到「微信输入法」/ zh-Hans）；同时诚实标注 macOS 无法获取 `open`/`full_shape`，按契约规定留空不猜 |\n| 2026-08-05 | 用户认同「先把底子弄好」优先于督促 ipfs/libp2p → 新增 §一·五「为什么 v0.1.15 不推进 L-NET」：L-NET 卡点是 N3 产品消费者**形态未定（拍板题）**而非工程量，形态未定前投工程会返工；底子欠账（IME 契约失衡、install 四处硬伤、cache 恶化）是用户每天碰得到的。L-NET 保持 research 车道，R3 只换车道不减验证 |\n
---

## 十、粘贴失败问题树（2026-08-05 · 规划，未开工）

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

