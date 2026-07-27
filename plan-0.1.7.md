# AgenTerm v0.1.7 产品与交付计划（讨论稿）

状态：等待产品评审
建议主题：**Governed Automation & Fast Delivery**
基线：v0.1.6 已发布
产品事实源：[PRD.md](PRD.md) 及其链接的 `prd/PRD_*.md` 模块

## 1. 文档职责与对齐规则

本文件是 v0.1.7 的执行投影，用于管理范围、依赖、工作包、里程碑、风险和
验收顺序，不是第二份 PRD。

- 产品定位、版本归属、能力边界和最终验收以 PRD 产品集为准。
- 本计划不得单独创造产品承诺。范围变化先更新对应 PRD 模块，再同步本计划。
- 能力实现状态只在 PRD 中使用 `[x] / [~] / [ ]`；本计划使用项目状态。
- 主要对齐节点：
  - [版本路线](prd/PRD_02_18_roadmap.md)
  - [Rhai scripting](prd/PRD_02_10_rhai_scripting.md)
  - [Human workspace](prd/PRD_02_06_human_workspace.md)
  - [Agent control plane](prd/PRD_02_07_agent_control_plane.md)
  - [Observable Fleet](prd/PRD_02_08_observable_fleet.md)
  - [Self-hosted development](prd/PRD_02_09_self_hosted_development.md)
  - [Delivery and quality](prd/PRD_02_17_delivery_quality.md)
  - [Executable family](prd/PRD_02_02_executable_family.md)

## 2. 版本命题

v0.1.6 证明了 AgenTerm 可以安全地执行受监督、可审计的 `pure` 与 `observe`
脚本，但脚本仍主要是一次性开发者工具。v0.1.7 要把它升级成用户能够发现、
保存和主动调用的自动化产品，同时保持默认拒绝、资源有界和 GUI 故障隔离。

版本还必须修复两项直接影响信任的交付问题：

1. 正式发布在本地和 CI 重复冷编译、重复门禁，反馈周期过长。
2. 当前 GUI、IPC server、PTY owner 和 workspace authority 同进程；旧 server
   被重新显示时仍执行旧 UI。产品必须诚实显示运行版本与 staged 版本，并形成
   GUI/server 分离的迁移决策，不能把隐藏窗口误述成可热升级 GUI。

## 3. 目标用户与关键任务

### 3.1 键盘优先的终端用户

希望不离开 AgenTerm 即可搜索并运行命名命令，看到脚本生成的简洁状态，但不希望
后台脚本抢焦点、卡住渲染或偷偷发送终端输入。

### 3.2 自动化作者

希望把经过验证的 `.rhai` 保存为稳定名称，离线检查 manifest/API/capability，
通过同一命令 ID 从 GUI、CLI 或 agent 调用，并得到一致的结果、审计和错误。

### 3.3 维护者与发布者

希望正式发布命令在短暂 preflight 后返回 tag、commit 和 CI 地址；CI 对同一批
二进制完成完整验证并原样发布，失败时直接指出 job、step 和诊断证据。

### 3.4 升级中的用户

希望知道当前窗口实际由哪个 build 托管、磁盘上是否已有更新版本，以及为何当前
版本需要重启 server。未来 GUI/server 分离后，升级 UI 不应牺牲活 PTY。

## 4. 产品原则

1. **主动调用先于后台自治。** v0.1.7 先交付用户明确触发的命名命令；持续事件
   handler 留给 v0.1.8。
2. **默认拒绝。** `observe` 不增权；`control` 必须逐 operation allowlist，
   不存在“获得 control 就能调用所有 mutation”的通配权限。
3. **人和 agent 共用一个目录。** GUI、CLI、Rhai discovery 和未来 MCP 引用同一
   stable command/operation ID。
4. **结构化结果，不执行 UI 文本。** 状态 provider 返回受限 segment 数据，
   不能绘图、创建 HWND 或控制布局。
5. **发布一次构建、一个事实。** 被测试、被哈希和被发布的是同一 artifact。
6. **诚实描述连续性。** restart、gap、stale build、PTY 重建和停机期事件缺失都
   必须显式呈现。

## 5. v0.1.7 功能树

### A. Fast and Trustworthy Release Pipeline（P0）

- `release.ps1` 只执行 clean-tree、branch、version/tag、远端冲突、认证和输入
  preflight，然后原子推送 `main + tag`。
- 本地 preflight 输出 commit、tag、CI URL、下一阶段和失败恢复提示。
- CI 将 static/unit、唯一 release artifact build、desktop/functional smoke 和
  event-journal stress 拆成安全的依赖图。
- Cargo registry、Git sources 和兼容的 build outputs 使用有界、可失效缓存；
  clean runner 永远可以从零完成。
- release artifact 只构建并上传一次，生成 commit/version/hash provenance；
  后续测试下载该 artifact，publish job 原样晋升，禁止重编译。
- 4,128-write event-journal saturation 每个 release SHA 恰好执行一次。
- GUI smoke 使用独立 address/workspace/settings 并保持 `no-activate`；会竞争同一
  desktop 的旅程串行，不为表面并发制造互扰。
- 工作流记录 queue、cold/cache-hit、各 job、tag-to-Release 和失败诊断时间。

### B. Local Script Registry（P0）

- 默认目录 `%LOCALAPPDATA%\AgenTerm\scripts`，并支持测试专用显式路径。
- 每个注册项使用稳定 ID、显示名、描述、入口脚本、API version、profile、
  capability 请求、参数 schema、启用状态和 source fingerprint。
- 第一版 manifest 格式在实现前冻结并版本化；未知字段/version fail closed。
- 支持 `script list`、`script inspect NAME`、`script check NAME` 和
  `script run NAME -- ARGS`，同时保留显式文件/stdin入口。
- registry 只负责读取源文件，不因此授予脚本 `fs.read`。
- 采用原子刷新；坏 manifest/脚本只使对应条目 degraded，不破坏其他命令。
- GUI 首窗不扫描脚本目录、不创建 Rhai engine；registry 按需或后台加载。
- 可选 compiled-AST cache 以 fingerprint + API version + capability profile 为
  key，并有明确字节/条目上限；它不是 v0.1.7 发布阻塞项。

### C. Named Commands and Command Palette（P0）

- 命名命令是 registry 条目的产品入口，不是另一套脚本格式。
- GUI 提供可键盘发现的 Commands 入口和命令面板，建议快捷键
  `Ctrl+Shift+P`；必须与 PTY/原生 Edit 快捷键仲裁。
- 面板支持名称/描述搜索、capability 标识、参数输入、运行中/成功/失败反馈和
  Esc 取消，不显示 source 正文或 secret。
- CLI、GUI semantic action 和未来 agent 使用同一 stable command ID。
- 同一命令同时只能有一个默认 invocation；并发必须显式配置且受 supervisor
  ceiling 限制。
- 命令运行不自动切换 tab、抢前台或覆盖 Composer，除非 manifest 与 capability
  明确声明并通过对应 policy。

### D. Reviewed `control` Preview（P0）

- 在现有 `pure|observe` 之后加入显式 `control` profile，但默认仍为
  `observe`/拒绝 mutation。
- 首批只允许逐项评审的低破坏 typed operations，建议候选：
  - tabs show/hide/toggle/set-width；
  - select tab；
  - rename/note；
  - set Composer draft，但不得默认发送。
- `send-keys`、`send-composer`、close、kill、shutdown、filesystem、environment、
  process execution、network 和任意 UI message 均不进入首批 allowlist。
- manifest 请求、用户/CLI invocation 授权、broker policy 和 operation 自身分类
  四层都必须允许；任意一层缺失即 typed denial。
- 每次 mutation 返回可验证 post-state/event position，并写隐私有界 audit。
- scripts 不能授予自身 capability，也不能通过 legacy alias 绕过 operation ID。

### E. Bounded Dynamic Status Providers（P1）

- provider 是 registry 中的只读 `pure|observe` 特化命令；v0.1.7 不允许 provider
  使用 `control`。
- 返回结构化 segment：稳定 ID、短文本、tone、tooltip、freshness 和可选的命名
  command action；不返回坐标、字体、颜色值或 Win32 handle。
- 状态栏继续拥有布局、优先级、截断、hit target 和主题；Tabs/CWD/Proxy 的宿主
  恢复入口永远优先。
- 建议初始预算：最多 8 个 provider、刷新间隔不短于 5 秒、单次 wall time
  不超过 500 ms、结果不超过 4 KiB、文本字段有固定长度上限。
- timeout/crash/invalid result 显示 degraded 状态并保留 last-known-good；连续失败
  触发退避，不形成 worker storm。
- provider scheduler 不在 GUI thread 执行；关窗 detach、server stop、设置 reload
  和 parent exit 均可取消且无 orphan。
- 若 P0 完成后 provider 对首窗、渲染或 worker churn 的证据不达标，允许降为
  v0.1.7 preview，但不得拖延 P0 发布。

### F. Upgrade Truth and GUI/Server Separation Gate（P0 决策，P1 产品切片）

- `server-list`、protocol snapshot 和 launcher guidance 暴露运行中 server/UI host
  的 build identity，并能与 `dist/agenterm.json` 比较。
- staged build 与 live host 不同时明确显示“当前窗口仍由旧进程绘制”，并给出
  preserve/stop/cancel 语义正确的升级引导。
- v0.1.7 完成一个 state-ownership matrix：
  - server：tab tree、PTY/process、scrollback、workspace、journal；
  - GUI client：HWND、layout、theme preview、focus、modal、painting；
  - settings/workspace：少量跨 GUI 重连的持久偏好。
- 冻结 server/client version handshake、snapshot bootstrap、event follow、
  input/clipboard/control routing、disconnect/reconnect 和 rollback 语义。
- 交付一个隔离 prototype，证明 GUI client 可终止/重启而 server PID、tab IDs、
  PTYs 和 scrollback 保持；prototype 不等于默认架构切换。
- 是否在 v0.1.7 正式启用完整进程拆分，必须经过单独 Scope Gate；本计划默认只
  承诺架构决策、协议原型和升级事实展示，避免与 scripting 主线同时大爆炸。

## 6. 明确不进入 v0.1.7

- 默认或通配 `control`，以及 destructive automation policy。
- 后台事件 handler、持久 scheduler、重试图和 exactly-once side effects。
- `fs.*`、`env.*`、`proc.exec`、network、package/module resolver。
- MCP transport/client federation、brain/flow、自主 agent scheduling。
- Bash runtime 分发、optional component 联网安装、installer/updater。
- 默认切换到拆分式 GUI/server，除非独立 Scope Gate 和完整黑盒均通过。
- 用删减 fmt/Clippy/unit/public UX/stress、放宽 size/startup 或关闭安全来换速度。

## 7. 依赖关系

```text
PRD scope + metrics freeze
├─ Release pipeline DAG ────────────────┐
├─ Typed operation coverage → policy ──┼→ control broker/API ─┐
├─ Registry schema → registry loader ──┼→ named commands ─────┼→ status provider
├─ Observable Fleet (shipped) ─────────┘                      │
└─ GUI/server ownership ADR → handshake prototype             │
                                                               ↓
                         integrated public gates → RC → release
```

事件 handler 依赖 registry、policy、Observable Fleet 和 scheduler，但整体属于
v0.1.8，不进入上图的 v0.1.7 RC 依赖链。

## 8. 工作包与并行边界

| WP | 工作包 | 主要所有权 | 依赖 | 可并行 |
|---|---|---|---|---|
| WP0 | 产品范围、指标、schema freeze | PRD 产品集、plan | 无 | 主代理串行 |
| WP1 | Release Pipeline v2 | `release.ps1`, workflow, build scripts | WP0 | 可独立 |
| WP2 | Operation/policy coverage | `operations.rs` + 新 policy 模块 | WP0 | 与 WP1/WP3 |
| WP3 | Registry/manifest | 新 `script_registry.rs` + unit tests | WP0 | 与 WP1/WP2 |
| WP4 | Control profile/broker | script protocol、sidecar、audit | WP2 | 与 WP5 后半 |
| WP5 | Named Commands UX/CLI | commands + 独立 palette/geometry 模块 | WP3 | 接口冻结后 |
| WP6 | Status providers | provider/scheduler 模块 | WP3、WP4/5 | P1 |
| WP7 | GUI/server architecture gate | protocol spike、ownership ADR、黑盒 | WP0 | 独立 |
| WP8 | Integrated public evidence | focused smoke + `check.ps1` | WP1–7 | 最终串行 |

共享 checkout 下 `src/lib.rs`、`commands.rs`、`check.ps1`、PRD 和 smoke 文件均为
hot files，同一时刻只能有一个 owner。并行开发优先新纯模块和只读调查；禁止竞争
同一个 Cargo target。

## 9. 里程碑与退出准则

### G0 — Scope Gate

- PRD 与本计划对 v0.1.7/v0.1.8 归属、非目标和 success metrics 完全一致。
- manifest、command ID、capability/policy、provider result 和 build identity
  schema 有版本化草案。

### G1 — Delivery Gate

- 本地 preflight 不执行完整冷门禁。
- CI 明确显示 cache/cold、job/step timing 和直达失败日志。
- 测试与发布 artifact SHA 完全一致；stress 对 release SHA 恰好一次。

### G2 — Registry Gate

- list/inspect/check/run NAME 通过 public black-box。
- 坏条目隔离、原子 reload、路径隔离、fingerprint 和无启动扫描均有证据。

### G3 — Control Gate

- discovery 展示 exact allowlist；默认 deny。
- 每个允许 operation 有 success/denial/scope/post-state/audit 黑盒。
- destructive、send、filesystem/process/network 均不可通过别名或参数逃逸。

### G4 — Named Command UX Gate

- 人、CLI 和 agent 使用同一 ID；命令面板可发现、可取消、无焦点泄漏。
- 参数错误、worker timeout/crash 和 server restart 有明确恢复状态。

### G5 — Provider Gate

- timeout、crash、invalid、truncation、reload、last-good、degraded、backoff 和
  no-orphan 全覆盖；host segments 始终可恢复。
- 若未达到 gate，只降级 provider preview，不降低 P0 门禁。

### G6 — Upgrade Architecture Gate

- 运行/staged build 差异公开可见。
- ownership/handshake/compatibility/rollback 决策已写入 PRD。
- prototype 证明或否定 GUI-only restart，并据证据确定正式拆分版本。

### RC — Release Candidate

- fmt、Clippy、unit、public CLI/UX/fleet/script/privacy/stress 全绿。
- 首窗 <1 秒；GUI <4 MiB；sidecar 保持现有预算或经评审的显式预算。
- remain-on-exit、explicit-close、tree safety、detach/stop/cancel 无回退。
- PRD capability/evidence contract、README、AGENTS、API discovery 与实际一致。

## 10. 成功指标

### 10.1 发布

v0.1.6 基线：

- 本地 `release.ps1`：约 4 分 20 秒。
- GitHub tag workflow：约 4 分 11 秒。
- 端到端：约 8 分 30 秒，且存在重复 qualification/build。

v0.1.7 目标：

- 本地 preflight p95 ≤ 15 秒，不含交互认证和网络重试。
- cache-hit tag-to-Release 最近 3 次中位数 ≤ 2 分 30 秒，较基线至少改善 35%。
- cold run 不慢于 v0.1.6 的 4 分 11 秒。
- 首个可操作失败诊断 ≤ 90 秒。
- 发布资产与被验证 artifact hash 100% 相同。

### 10.2 脚本产品

- 离线 registry list/inspect/check 不启动 GUI。
- 普通命名命令 invocation 不阻塞 GUI thread，取消/超时后无 orphan。
- 未授权 control 成功率为 0；每个授权 mutation 都有 post-state 和 audit。
- provider 失败不移动宿主 segment，不清空 last-good，不影响 PTY/rendering。
- GUI 正常启动不扫描脚本目录、不加载 Rhai，首窗预算不回退。

## 11. 风险登记

| 风险 | 影响 | 缓解/决策 |
|---|---|---|
| control + registry + provider 范围过大 | 延期和安全回退 | P0/P1 分层，provider 可降 preview |
| operation catalog 覆盖不足 | legacy alias 绕过 policy | 先补 typed coverage，再开放 API |
| manifest 变成隐式 fs 权限 | 权限升级 | source loading 与 runtime authority 分离 |
| provider worker storm | CPU/进程抖动 | 最小间隔、并发 ceiling、退避、取消 |
| status output 泄密 | UI/截图泄漏 | 结构化 schema、长度限制、redaction/audit |
| stale Cargo cache | 错误 artifact | lock/toolchain/profile key；cache miss 正确 |
| 并行 GUI 测试互扰 | flaky release | 独立环境；desktop journeys 串行 |
| artifact build/test 不同一 | 发布信任破坏 | 单次上传、hash provenance、只做 promotion |
| GUI/server 拆分吞噬版本 | 主线失控 | v0.1.7 默认只决策+prototype，另设 Scope Gate |
| event handler 语义被误承诺 | 丢事件/重复副作用 | v0.1.8 再做；不承诺 exactly-once |

## 12. 发布与回滚

- v0.1.7 tag 仍由仓库原生 `release.ps1` 原子推送，不依赖本地 `gh` 或 PR。
- publish job 只有在所有 required jobs 和 artifact hash gate 成功后执行。
- tag 已推送但门禁失败时不覆盖 tag、不手工替换资产；修复后提升版本或按明确的
  release recovery policy 处理。
- registry/control/provider 均带 feature availability；严重问题可以禁用新入口，
  但不得关闭核心 terminal/control plane。
- post-release review 记录各阶段耗时、cache hit、失败/重试和用户可见回退。

## 13. 待产品确认

1. 是否接受 v0.1.7 主题为“受治理的主动自动化 + 快速可信发布”，把持续事件
   handler 明确移到 v0.1.8？
2. 动态 status provider 是 v0.1.7 P1/可降 preview，还是必须成为 release blocker？
3. 命令面板是否采用 `Ctrl+Shift+P`，以及是否需要一个常驻可点击入口？
4. 首批 control allowlist 是否接受 tabs layout、select、rename/note、set Composer
   draft，并继续禁止 send/close/kill？
5. 脚本 registry 是否允许一个 manifest 声明多个 named commands/providers？
6. GUI/server 完整拆分是否提升为 v0.1.7 P0；若提升，应相应缩减 scripting
   范围，不能两条大型架构主线同时满载。
7. staged build mismatch 的提示放在状态栏、Settings/About，还是仅 launcher/CLI？
