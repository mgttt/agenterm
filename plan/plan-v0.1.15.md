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
└─ G. 安装/更新体验（2026-08-05 macOS aarch64 真机：0.1.12-local → v0.1.14，详见 §八）
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
   ├─ [ ] G4 升级后运行态提示：装完未告知「已开窗口仍旧码」
   │     动机：install 成功后 symlink 已指 0.1.14，但既有 GUI/server
   │     进程仍跑旧映像；本机实测窗口仍 0.1.12 直至重开
   │     建议：收尾 say「若 GUI 已在运行请完全退出后重开」；可选探测
   │     本机 agenterm/agenterm-server 进程并提示 pid（不强制杀）
   ├─ [ ] G5 无 first-class 更新入口 / 无 old→new 摘要
   │     动机：无 `agenterm-cli update` / `install.sh --check`；不打印
   │     当前已装版本、channel（unsigned-preview vs signed）、是否已最新
   │     建议：resolve 后对比 current；已最新则 no-op 退出 0；否则打印
   │     `0.1.12-local → 0.1.14 (macos-unsigned-preview)` 再下载
   ├─ [ ] G6 releases 目录不修剪：0.1.11-local / 0.1.12-local 永久堆积
   │     建议：保留 current + N 个历史（默认 2）或 `AGENTERM_KEEP_RELEASES`
   └─ [ ] G-P1（政策）macOS 长期 channel：unsigned-preview 是否为默认
         公开通道，还是必须等 Developer ID 签名 asset 才算 stable
         （影响 G1 默认行为与 Promotion 文案）
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
7. **G2 + G4 + G3（文档级）**：安装卫生三件套，不碰发布链语义，可与
   A 组并行；G1/G-P1 等 macOS channel 政策拍板后再改默认回落行为。

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
| `plan/ARCHITECTURE.md` | 结构 SSOT；本文不重画结构树 |
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

### 5.7 决策项（需人工拍板，agent 不自主执行）

| ID | 决策 | 影响 |
|----|------|------|
| P1 | agenterm.work 与 agenterm.mega.tech 的归属/迁移（Pages CNAME 还是独立服务） | 决定 E1 走向 + L-PKG 基建 |
| P2 | Control Center 是否改名、改什么名 | 影响 PRD_02_21 标题/命名、可执行族与文档 |
| P3 | 「皮肤」扩展面与 theme/plugin 打包的边界 | 决定 L-EXT 的范围与版本归口 |
| P4 | computer-use 是否立项、归口 PRD、首发平台与证据门 | 决定 L-CU 是否进 v0.2.0 或更后 |
| D1–D3 | 见 §一 D 组（发布链政策） | 与产品主线独立，但 A2/B1 落地依赖 D1 取向 |
| G-P1 | macOS unsigned-preview 是否为默认公开通道（无 signed 时自动回落 vs 强制 opt-in） | 决定 G1 默认行为与 install/README 首屏命令 |

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

6. **无 update 语义**  
   不比较已装版本；不打印 channel；已最新仍会重下重装（本轮因
   显式 `AGENTERM_VERSION` 未踩，但 `resolve_version=latest` 路径
   同样缺 no-op）。releases 下旧 local 目录永留。→ **G5 / G6**

### 8.3 建议落地切分（给 v0.1.15）

| 优先级 | 项 | 改动面 | 风险 |
|--------|----|--------|------|
| P0 | G2 孤儿 symlink 清理 | `install.sh` 收尾 | 低：仅删指向 current 且 target 缺失的 agenterm* 链 |
| P0 | G4 重启提示 | `install.sh` say 一行 | 低 |
| P1 | G3 VERSION 文件 + `agenterm --version` | install + GUI launcher 早退 | 中：launcher 参数解析需测 |
| P1 | G5 old→new / already-latest | `install.sh` | 低 |
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

