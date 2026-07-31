# AgenTerm v0.1.12 公开计划

状态：规划启动（2026-07-31）
工作主题：**收敛 v0.1.11 基础、折叠候选到发布的等待时间，并让三平台
Control Center / native IPC 进入可持续演进状态**

本文是执行计划和决策记录，不替代产品事实。接受后的产品范围、状态与
验收证据必须同步进 `PRD.md` 及对应 `prd/PRD_*.md`；实施中允许按证据调整
波次，但不得用计划中的愿景冒充已经发布的能力。

## 一、树式精华

```text
v0.1.12  Convergence & Fast Promotion
│
├─ P0：候选只验证一次，发布只提升已验证字节
│  ├─ 普通 CI、候选 qualification、tag promotion 职责分离
│  ├─ 同一 commit 不再本地、CI、Release 三次重复完整门禁
│  ├─ exact-SHA receipt + artifact hashes + SBOM + provenance 成为提升凭证
│  ├─ 六平台候选产物在批准前生成；tag 后只验证并发布
│  ├─ tag-to-Release 目标：热缓存 p50 ≤ 3 分钟、p95 ≤ 6 分钟
│  ├─ 失败在 tag 前暴露；失败 tag / 半成品 Release 不进入正常流程
│  └─ 缓存、runner、队列、编译、测试、打包、上传均有分段计时
│
├─ P0：native IPC 与 LogicalInstance 发布后收敛
│  ├─ main/dev 单例、隔离、发现和显式 endpoint 行为一致
│  ├─ Windows named pipe 与 Linux/macOS Unix socket 权限事实可验证
│  ├─ stale socket / stale registration / PID reuse 安全恢复
│  ├─ schema-v1/v2、旧 TCP 与新 native endpoint 混合升级/回滚
│  ├─ server-list 不把测试残骸长期显示成真实 server
│  └─ 所有 GUI/CLI/CC/MCP/Mux/Script 继续复用同一 resolver
│
├─ P0：三平台 GUI 与 Control Center 可用性收敛
│  ├─ 主工作台工具栏、Tabs、Composer、locale、font 行为对齐
│  ├─ Control Center 选择调用者的同一 logical instance
│  ├─ Cockpit 从壳升级为有用的只读 Fleet 诊断面
│  ├─ Unix Control Center 获得真实 snapshot + renderer-owned PNG 证据
│  ├─ incompatible / renderer failure / server-retained 故障矩阵补齐
│  └─ GUI/CC 仍是可替换投射，server/PTY/workspace 权威不进入 UI
│
├─ P1：开发反馈继续折叠
│  ├─ lint → 定向测试 → 平台 quick → candidate 的逐级反馈
│  ├─ Rust/Cargo 缓存按 OS、arch、toolchain、lock、profile 正确分层
│  ├─ 评估 sccache/远程缓存，但缓存 miss/损坏不能改变正确性
│  ├─ GUI 黑盒按隔离资源并行，禁止窗口风暴、固定 sleep 和残留进程
│  └─ 慢门、缓存命中率、CPU/IO 利用率进入机器可读报告
│
├─ P1：付费/自托管 runner 有证据试验
│  ├─ 先用当前 workflow 记录 3 次冷/热基线
│  ├─ 比较 GitHub 8-core larger runner、Depot 与可信自托管 Windows
│  ├─ 用真实 AgenTerm qualification 比较时间、价格、队列和失败率
│  ├─ 第三方 action 固定 commit，最小权限，不向不可信 PR 暴露凭据
│  └─ 只有端到端收益显著且可回退时才切换默认 runner
│
├─ P1：脚本与二级产品继续准备
│  ├─ agenterm-script 交付真正的持久 REPL
│  │  ├─ ReplSession 会话内核与 CLI 输入适配解耦，可供 CC/Agent 复用
│  │  ├─ 变量、函数、多行单元、错误恢复、reset 和内存 history
│  │  ├─ TTY 提示与 pipe/NDJSON 自动化输出分离
│  │  ├─ 单元失败不提交语言状态，外部真实副作用不伪装回滚
│  │  └─ 普通 worker 与 REPL 复用同一 Engine/API 配置
│  ├─ 评估把 canonical Rhai 入口从 agenterm-script 重命名为 agenterm-rhai
│  │  ├─ 名字直接表达语言/runtime 身份，不再暗示抽象的通用脚本沙箱
│  │  ├─ 先冻结 CLI、task、worker、包名、文档和第三方调用者影响清单
│  │  ├─ 若本轮实施，agenterm-script 作为有期限的兼容转发入口，不复制 runtime
│  │  └─ 构建、测试、Candidate、Promotion 必须在 canonical 名切换后保持自举
│  ├─ Control Center 的 Workflows/Extensions/InfoHub 保持真实空状态
│  ├─ agenterm-net 继续独立研究，不进入稳定 server 热路径
│  └─ WebView host 继续 renderer-neutral，不因“界面丰富”仓促替换原生壳
│
└─ 明确延后与未来计划
   ├─ executable consolidation 决策树
   │  ├─ 首选：共享 Rust runtime/library + 多个职责清晰的薄入口
   │  ├─ 可研究：agenterm-rhai 被宿主进程内嵌，但独立 CLI 合同仍可用
   │  ├─ 可研究：兼容入口按使用证据退场，而不是永久增加同义 EXE
   │  └─ 不做：为“少一个文件”牺牲 GUI/Console 子系统、管道退出码或故障隔离
   ├─ 完整 Workflow/Pipeline 设计器、调度器与跨机恢复
   ├─ PluginHub/AppHub 公共市场、交易、静默安装与自动更新
   ├─ InfoHub 自动执行外部信号
   ├─ libp2p/IPFS 常驻 full node、DHT/pubsub/relay 与远程 Fleet attach
   ├─ 未经生产证据即把系统 WebView 设为唯一 Control Center renderer
   ├─ 未经认证/加密/威胁模型即开放公网远程控制
   └─ Agent harness 的权限、审批、凭据与策略
```

## 二、版本 outcome

> v0.1.12 发布候选应在用户批准 tag 前，已经由同一 Git commit 产出并验证
> 六平台归档、完整 Windows 行为 qualification、平台原生 smoke、SBOM、
> provenance 和 exact-byte receipt。用户批准后，tag workflow 只验证 tag
> 与候选身份并提升已有字节，正常情况下数分钟内出现完整 Release。与此同时，
> v0.1.11 引入的 native IPC 和 Control Center 在 Windows、Linux、macOS 上
> 具备一致、可诊断、可恢复的基础行为，而不会把 UI 生命周期重新耦合到
> server/PTY。

## 三、为何本轮优先做“收敛与提速”

v0.1.11 的实际发布给出了可量化事实：

- 本地普通全套门禁约 **376.8 秒**；
- 本地 stress-inclusive qualification 约 **512.7 秒**；
- tag workflow 中 Windows x64 又执行一次完整 release quality gate；
- 同一 tag 同时触发普通 CI 和 Release workflow；
- Linux/macOS 构建先完成，最终 Release 被 Windows x64 重复门禁串行阻塞；
- Linux GUI wrapper 缺陷直到首个 tag matrix 才被真实 Unix package step
  发现，说明“tag 前六平台候选”仍未形成闭环。

这不是单纯的“机器慢”，而是工作被放在了错误时间重复执行。优先级应为：

```text
先消除重复与 tag 后发现
  → 再建立正确缓存
    → 再调整并行拓扑
      → 最后用更快 runner 放大已经正确的流程
```

## 四、候选与发布流水线

### 4.1 建议拓扑

```text
push main
├─ Fast CI
│  ├─ lint / fmt / PRD alignment
│  ├─ warnings-denied Clippy
│  └─ 定向 unit + representative public smoke
│
└─ Candidate workflow（显式触发或满足候选条件）
   ├─ Windows x64：一次完整 stress-inclusive qualification
   ├─ Windows ARM64：build + package
   ├─ Linux x64/ARM64：native/cross build + package + archive fixture
   ├─ macOS x64/ARM64：native build + package + unsigned/signed lane
   └─ aggregate
      ├─ exact-SHA qualification receipt
      ├─ six-platform asset manifest
      ├─ hashes / SBOM / provenance
      └─ immutable candidate run identity

用户明确批准
└─ tag vX.Y.Z 指向同一 exact SHA
   └─ Promotion workflow
      ├─ 验证 tag/version/SHA/candidate run/receipt
      ├─ 下载并复核候选字节
      ├─ 可选 GitHub artifact attestation
      └─ 创建 Release，不重新 Cargo build、不重跑完整 GUI suite
```

### 4.2 安全合同

- 候选资格绑定完整 commit SHA，禁止只按 branch、tag 名或“最近成功”选择；
- promotion 必须验证 receipt、Cargo.lock、artifact manifest、SBOM 和每个
  archive hash；候选 artifact 缺失或过期时 fail closed，回到 candidate
  workflow，不现场悄悄重建另一套字节；
- release approval 仍是用户动作；优化等待时间不降低发布权限门槛；
- GitHub Actions 引用继续固定到完整 commit；
- 第三方缓存只影响速度，cache miss、eviction、poison 或服务不可用不得改变
  required gates、产物身份和结果；
- public PR 不接触 release token、签名材料、自托管可信 runner 或可写缓存；
- promotion 只拥有创建 Release 所需的最小 `contents: write`，candidate build
  默认只读源码并上传工作流 artifact；
- macOS signed stable 与 unsigned preview 继续严格分流。

### 4.3 SLO 与观测

每次 workflow 输出以下时间，不再只报告总时长：

```text
queue
checkout/toolchain
cache restore + hit/miss + bytes
compile
unit/Clippy
GUI/public smoke
stress
package
artifact upload/download
promotion
tag-to-public-Release
```

首轮先记录当前 runner 三次冷/热基线，再接受目标：

- 普通 push 首个有用失败：p95 ≤ 90 秒；
- 候选 workflow：热缓存 p50 ≤ 8 分钟，p95 ≤ 12 分钟；
- 已有完整候选的 tag-to-Release：p50 ≤ 3 分钟，p95 ≤ 6 分钟；
- 无 tag 后才首次发现的 package member、权限或 launcher 缺陷；
- exact SHA 正常路径只执行一次完整 stress-inclusive qualification。

## 五、缓存与 runner 试验

### 5.1 先做的无供应商优化

1. 审计当前 workflow 是否缓存 Cargo registry、Git checkout、toolchain 与
   `target`；先区分下载缓存和编译产物缓存。
2. key 至少包含 OS、architecture、Rust version、Cargo.lock hash、profile
   和影响 feature/target 的版本化 salt。
3. 不在互不兼容的 host/target/profile 间共享 `target`；只允许安全的
   fallback key。
4. 候选 workflow 与普通 CI 可复用依赖/编译缓存，但候选 archive 必须由
   exact SHA 的受控 job 产生。
5. 评估 `sccache` 时记录命中率、传输字节、压缩时间与总 wall clock，
   不能只看“cache hit”文本。

### 5.2 付费方案试验顺序

| 方案 | 优点 | 前提/风险 | 试验结论门 |
|---|---|---|---|
| GitHub larger runner | 官方托管、Windows 4–96 vCPU、自定义镜像 | 需要 GitHub Organization 的 Team/Enterprise；始终按分钟付费 | 先试 8-core Windows，端到端至少快 35% |
| Depot | Linux/Windows/macOS、快速缓存、按秒统计、改 runner label 较小 | 仓库需属于 GitHub Organization；供应商与镜像差异需验证 | 7 天试用跑真实冷/热候选各 3 次 |
| WarpBuild | 多规格 runner、兼容 Actions 生态 | Windows cache 支持边界需按当前文档核实 | 只在 Windows 实测胜过官方方案时进入候选 |
| 自托管 Windows | 持久 warm cache、硬件可控、可能最快 | 安全隔离、维护、可用性、密钥和公开 PR 风险最高 | 仅可信 push/tag；优先 ephemeral VM，不直接暴露开发机 |

`BuildJet` 不进入候选清单：其 GitHub Actions runner 服务已宣布于
2026-03-31 停止。

选择不是“最快一次”，而是：

```text
端到端 p50/p95
+ 排队时间
+ 每候选成本
+ 缓存冷启动
+ 六平台可用性
+ 故障率与诊断
+ 权限/供应链风险
+ 一行回退到 github-hosted 的能力
```

## 六、native IPC 与实例收敛

本轮不再新增第三种实例语义，集中把 `main|dev` 做实：

- 在真实 Windows/macOS/Linux 上并发启动同 role，恰有一个 authority；
- 不同 role 的 endpoint、registration、workspace、settings、epoch 严格隔离；
- Unix socket 父目录 owner/mode、socket mode、symlink 拒绝与路径长度均有
  native evidence；macOS `/tmp` 与 `/private/tmp` canonicalization 不造成
  同一 authority 的双重身份；
- Windows named pipe DACL、local-only、竞争创建和 stale registration
  有 typed diagnostics；
- schema-v1/v2 混合发现时，typed endpoint 优先；legacy handshake 不因
  `tcp:` 表达差异误判 stale；
- `server-list` 区分 live、unreachable、stale-test-fixture，并提供安全、
  显式、可审计的 stale cleanup，而不是自动 kill 不确定 PID；
- GUI、CC、CLI、MCP、Mux 和 Script 使用同一 selector/resolver 表面。

## 七、三平台 GUI 与 Control Center 收敛

### 7.1 主工作台

- 对齐 toolbar 顺序、`En|Zh`、字号动作、Tabs 双击编辑、tree lines、
  Composer 多行输入、scrollbar、selection、clipboard 和 no-activate；
- snapshot 的 semantic ID、bounds、visibility、focus 与 renderer PNG
  一致；高 DPI/Retina 使用 logical 与 physical 尺寸双事实；
- 平台适配器只拥有 OS 机制，产品动作与状态继续来自共享层；
- Linux/macOS 不为了“看起来相似”复制一套漂移的产品状态机。

### 7.2 Control Center

本版接受的首个深化仍是只读 Cockpit：

```text
Cockpit
├─ selected logical instance / endpoint transport
├─ server PID / build / protocol / health
├─ epoch / sequence / journal gap state
├─ tabs: total / running / dead / detached
├─ selected tab identity and health
└─ component capability / degraded reason
```

- open/focus/no-activate 必须选择调用者同一实例，不隐式启动另一 server；
- macOS/Linux 补齐真实 renderer-owned screenshot，而不是替代图或
  “capability available”假状态；
- 覆盖 incompatible sibling、renderer crash、server retained while GUI
  replaced、new epoch recovery 和 stale owner replacement；
- Workflows、Extensions、InfoHub 可以改进解释与导航，但没有 owning backend
  前继续显示真实 empty/unavailable，不造假数据。

## 八、并行实施波次

```text
Wave A（共享合同，可并行）
├─ A1：workflow timing + duplicate-work audit
├─ A2：candidate/promotion schema 与 threat model
├─ A3：native IPC mixed-version/stale matrix
└─ A4：Control Center/GUI parity gap matrix

Wave B（实现，可并行）
├─ B1：Fast CI 与 Candidate workflow 拆分
├─ B2：cache 基线与正确 key
├─ B3：Windows pipe / common resolver 收敛
├─ B4：macOS Unix socket + CC native evidence
└─ B5：Linux Unix socket + CC native evidence

Wave C（集成）
├─ exact-SHA candidate aggregate
├─ promotion workflow 与 fail-closed fixtures
├─ 三平台 mixed-version / no-activate / orphan matrix
└─ paid-runner A/B（不得阻塞免费默认路径）

Wave D（候选）
├─ clean main
├─ one complete qualification
├─ six-platform candidate assets
├─ non-publishing promotion rehearsal
└─ 用户批准后才 tag / Release
```

共享 workflow、artifact schema、endpoint resolver 与 PRD 文件由主代理协调，
平台代理优先提交自己 adapter、native fixture 和证据，避免同时大改同一
共享文件。每个小而完整的进展 review 后尽快进入 `main`，让其他平台及时
rebase 和验证。

## 九、完成定义

- `plan` 中接受的能力均已同步 owning PRD，状态不靠版本号猜测；
- `main` clean、已推送，普通 CI 六目标通过；
- exact-SHA candidate workflow 产出完整 qualification receipt 和六平台资产；
- tag promotion 不执行 Cargo build，不重跑完整 GUI/stress suite；
- promotion 对错 SHA、错 tag、缺 receipt、缺平台包、篡改 hash、过期 artifact
  和不完整 matrix 全部 fail closed；
- Linux/macOS/Windows 的 native IPC 与 Control Center 关键 journey 有各自
  原生证据；
- 没有新增窗口风暴、固定 sleep、测试残留 server/socket/pipe 或隐式
  foreground activation；
- 付费 runner 即使试验失败，也可通过一处 label/config 回退，不影响免费
  github-hosted 正确路径；
- 未经用户最后明确批准，不创建 `v0.1.12` tag 或 GitHub Release。
