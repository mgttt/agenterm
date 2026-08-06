# AgenTerm v0.1.12 公开计划

> ## ⚠️ 已归档（2026-08-05）
>
> **本文是 v0.1.12 时期的历史执行记录，保留仅为追溯，不要作为执行依据。**
> 其里程碑证据仍被 `prd/PRD_02_18_roadmap.md` 引用，故整档保留原文未改。
>
> 注意：v0.1.12 与 v0.1.13 虽有完整 plan，但**从未公开发布**——
> 其 Candidate 被放弃/取代，公开序列为 v0.1.11 → v0.1.14。
>
> - 上一已发布版本复盘：`plan/archive/plan-v0.1.14.md`；在制：`plan/plan-v0.1.15.md`
> - 在制版本：`plan/plan-v0.1.15.md`
> - 发布链要求（版本无关权威处）：
>   `prd/PRD_02_17_delivery_quality.md` §Release-chain operating requirements
> - 结构 SSOT：`plan/ARCHITECTURE.md`


状态：v0.1.12 产品收口（2026-08-02）；进入最终 qualification 前的版本冻结。
Candidate/Promotion 仍是独立授权门，不因本状态自动触发。
工作主题：**收敛 v0.1.11 基础、折叠候选到发布的等待时间，并让三平台
Control Center / native IPC 进入可持续演进状态**

本文是执行计划和决策记录，不替代产品事实。接受后的产品范围、状态与
验收证据必须同步进 `PRD.md` 及对应 `prd/PRD_*.md`；实施中允许按证据调整
波次，但不得用计划中的愿景冒充已经发布的能力。

## 当前执行主线（持续更新）

```text
已完成前置
└─ revision-4 Platform Facade 全量 OS 抽象
当前产品纵切
├─ [~] Control Center Cockpit 可用只读事实
│  ├─ server/build/epoch/sequence
│  ├─ running/dead/active tab health
│  └─ component availability + native renderer agreement
├─ native IPC / LogicalInstance 行为收敛
├─ Script REPL hardening（主体已 shipped）
├─ agenterm-net N2 experimental 纵切
└─ system-WebView research（不得替代 native CC）
远期交付门
└─ Wave D Candidate → 人工批准后的 Promotion/Release
```

近期顺序以产品价值和依赖为准：先让共享 Cockpit 合同成为可用诊断面，再
推进可独立验证的 Script、network 和 Web host 纵切。Candidate workflow
合同可以维护，但在产品阶段成果、三平台证据和用户明确意图之前不 dispatch，
也不以“具备 RC 条件”替代 v0.1.12 产品建设。

2026-08-01 新增主线：把现有内部 Platform Facade 收敛为 workspace member
`crates/agenterm-platform`，供外部仓库按 exact Git SHA 依赖。依赖图冻结为：

```text
zero-dependency contract/status
├─ process ─┬─ pty
│           └─ clipboard helper/process tree
├─ filesystem ─┬─ locking ── ipc
│              ├─ screenshot/font
│              └─ webview
└─ window ── input ── ime / activation

主 crate product extensions
├─ Windows/Unix AgenTerm frontend + renderer
├─ Control Center native shell
├─ AgenTerm endpoint/instance/workspace policy
└─ Fleet/Script/UI protocol and semantic snapshots
```

首个叶已进入实现：根 package 成为 workspace member，新增默认零 feature 的
`agenterm-platform` package；`process` contract、facade、private selected 与三平台
adapter 已从 `src/platform` 单一迁入新 crate，主程序通过 path dependency 真实消费，
没有保留第二套 process 实现。硬编码 sibling `agenterm-server.exe` 的 autostart
policy 留在主 crate，只调用平台 crate 的 generic detached-command mechanism。
`--no-default-features`、`--features process` 与主 `agenterm --lib` compile checks 已通过。
其余 feature 仍是声明中的迁移槽，在对应实现和 contract tests 落地前不得冒充完成。

第二个独立叶已把 PTY neutral contract、public facade、private target selection 和
Windows ConPTY/Linux POSIX/macOS POSIX adapters 单一迁入新 crate；主 crate 的
`pty` compatibility projection 现在真实指向 workspace dependency。既有 reader/wait
clone、terminate-to-EOF 与 close-pseudoconsole 行为由原 adapter 原样保留，typed
`Unsupported`/`Failed` 成为公开错误。平台 shell runtime defaults 同步归入
`process` feature，产品默认 tab/command policy 仍留在主 crate。`pty` feature 的
5 项 crate tests 与 Agenterm all-target compile check 已通过；filesystem/locking
审计确认现有 paths/audit 文件混有产品命名和 Script policy，必须先拆机制，禁止整文件硬搬。
第三个叶将纯平台中立的 DPI/geometry contract 移为 `window` feature 的公开 API；
Linux/macOS native scale adapters 经 workspace dependency 消费同一实现，旧 `src/platform/scale.rs`
已删除。4 项 window tests 和 Agenterm all-target compile check 通过；AgenTerm 专属的
320×240 CLI resize policy 继续留在主 crate，没有被包装成通用平台规则。
第四个叶新增零产品命名的 `filesystem` host-directory/executable conventions，以及
调用方提供 path、namespace、limit 的 `locking::{PathLock, SlotPermit}`。Unix 使用
`flock`，Windows 使用命名 mutex 并增加进程内 slot reservation，避免同线程 mutex
重入绕过全局并发上限。Script supervisor 现在真实消费该公开 locking API；
`AgenTerm` 目录、audit 扩展名、supervisor namespace 和并发错误映射仍由主 crate
决定。12 项 all-feature crate tests、warnings-denied Clippy 与 Agenterm all-target
compile check 通过。
第五个叶把 transport-qualified `IpcEndpoint`、parser、local validation 和可选 serde
表示单一迁入 `ipc` feature；主 crate 的 LogicalInstance、scope hashing、workspace
placement 和 legacy instance discovery 保持产品 ownership。未来 endpoint variant 在主
crate 旧 transport 遇到时返回 typed Unsupported，不使用 wildcard 静默降级。14 项
all-feature crate tests、warnings-denied Clippy 与 Agenterm all-target compile check 通过；
native byte listener/stream 仍待下一叶迁移，因此 `Capability::Ipc` 继续真实报告
`capability-not-yet-implemented`。
第六个叶把 `IpcTransportError{Code}`、Windows named-pipe listener/stream、Unix
socket listener/stream（含 private-directory、peer UID、stale socket lease identity
与 bounded timeout）单一迁入 crate，并公开 `NativeListener`/`NativeStream`、trusted
user identity 与 native runtime directory。主 crate adapters 现在只组合 AgenTerm pipe/
socket/workspace 名称；IPC capability 已有真实实现并报告 Available。14 项 all-feature
crate tests、warnings-denied Clippy、Agenterm all-target compile check，以及新 crate
反向产品耦合静态门禁均通过。
Workspace 交付卫生同步更新：Windows/Unix bootstrap worker identity 现在把整个
`crates/` 树纳入 tracked、worktree 与 untracked content fingerprint，并提升 schema，
避免平台 crate 改动复用陈旧 worker。crate README 已记录默认空 feature、当前依赖
DAG、三平台矩阵、typed failure 约束、公共 endpoint 示例和 exact Git revision 依赖方式。
第七个能力组完成 clipboard、screenshot encoding 和 font candidates：clipboard read
budget 由调用方传入，不再从 terminal paste policy 反向污染 native adapter；Windows
Unicode/Wayland-X11 helper/macOS pasteboard failures 统一映射公开 Unsupported/Failed。
截图 crate 只接收 caller-owned XRGB frame/path/clip，Windows HWND/GDI window capture
保持产品私有；三平台共享 bounded PNG encoder。font feature 暴露中立候选描述，
Windows GDI font handle/metrics 仍留产品 extension。21 项 all-feature crate tests、
warnings-denied crate Clippy 与 Agenterm all-target compile check 通过。
第八个能力叶完成 normalized input：平台中立 `ModifierState`、非穷尽
`KeyClassification`、committed-text-first 分类与有状态 UTF-16 decoder 进入公开
contract/facade；Windows adapter 明确保留 Control/AltGr 仲裁，Linux 与 macOS
分别保留 Control/Super、Command/Control primary-shortcut policy。主 crate 的
frontend event 翻译保持产品 extension，只通过公开 facade 消费机制，不保留第二套
分类实现。24 项 all-feature crate tests、warnings-denied crate Clippy 与 Agenterm
all-target compile check 通过；IME composition 仍是下一依赖叶，不由 input 状态冒充。
第九个能力叶完成 IME composition：公开非穷尽 `ImeEvent`/`ImeAction`、editable-anchor
preedit 仲裁、committed-text 分类及 display-aware status；Linux/macOS adapter 在有显示
后端时 Available、headless 明确 Unsupported，Windows 继续以
`ime-preedit-not-yet-adapted` 明确 Unsupported，不静默声称对等。主 crate 原 Linux/
macOS 状态机已替换为薄兼容投影。26 项 all-feature crate tests、warnings-denied crate
Clippy 与 Agenterm all-target compile check 通过。
第十个能力叶完成 activation：公开 `ActivationPolicy`、非穷尽 request/error 与 opaque
`NativeWindowHandle`，Windows show-without-activation/new/restore native 操作单一迁入
crate；Linux/macOS 的 winit active/application intent 由 target-isolated adapter 接管。
主 crate 只保留产品 capability-status 映射和 live handle 生命周期责任。activation
feature 的 target 依赖不进入默认、process 或 filesystem 最小配置；Unsupported/Failed
不降级为成功。
Process-tree 去重叶随后把 Script worker 的 owned-command configuration、Windows Job
Object 与 POSIX process-group guard 全部改为消费 crate `process` facade；三平台 root
supervisor adapter 只保留 AgenTerm audit 路径命名和产品错误投影。worker supervisor
聚焦测试与 Agenterm all-target warnings-denied Clippy 通过，root 不再重复 Win32/libc
进程树实现。
Filesystem 产品组合叶删除三套 root paths adapter：host config/local-data roots 与
executable suffix 只取自 crate `filesystem` selected adapter，AgenTerm 大小写目录、
workspace/instance/settings 文件名和 macOS 默认字号留在无 OS cfg 的产品 service。
paths/settings/workspace 聚焦测试与 Agenterm all-target Clippy 通过。
WebView 与 native-font crate surfaces 随后落地：WebView2/WebKitGTK/WKWebView 被动
探测统一返回 public presence/probe 与 typed Missing/Failed；font 扩展为 discovery、
metrics、opaque window token 和 RAII `NativeFont`，Unix metrics 的 `ab_glyph` 与
Windows GDI 依赖均按 target+feature 隔离。29 项 all-feature crate tests 及 crate/root
warnings-denied Clippy 通过；root Windows font hot path 去重仍是下一提交，本文不提前
宣称已删除。
Native font hot path 随后完成：Windows remote renderer 持有 crate `NativeFont`，设置
失败与替换均由 RAII 精确释放，不再手工 `DeleteObject(HFONT)`；Linux/macOS capability
和 primary-family 走同一 facade，三套 root native font 文件删除。activation 的 winit
类型也从 public facade 移到 adapter-owned extension trait，使 crate contract/service
静态门禁通过。crate/root warnings-denied Clippy 与聚焦平台边界测试通过。
Script clipboard native leaf 改为直接消费 crate facade，删除三平台 root selector/
adapter；公开 API 新增 caller-supplied open deadline，使 GUI 默认 500ms 与 Script
Runtime 既有 2s 健壮性契约同时保留。Script 调用仍是无限制本地能力，不加入路径、
内容或权限 allowlist。30 项 all-feature crate tests、Script clipboard contract test 与
两 crate warnings-denied Clippy 通过。
Script files/stream native leaf 完成：atomic replace、parent sync、link/reparse detection
进入 `filesystem` facade；Windows `PeekNamedPipe` 以 opaque `PipeProbeToken` 和 typed
Closed/Failed 进入 `process` facade。三平台 root script_files/script_stream adapters 删除，
Rhai 注册、capture/delivery limits 与 receipts 仍属产品层且不形成授权策略。5 项 stream、
14 项 unrestricted filesystem stdlib tests、30 项 crate tests 与 warnings-denied Clippy
通过。
Script child-window leaf 完成：public `ProcessWindowFacts/Rect/Key/PointerAction/Message/Error`
与 facade 进入 `window` feature，Windows EnumWindows/input/control/resize 实现物理迁入
crate adapter，Linux/macOS 保持 typed Unsupported。三平台 root script_window adapter
删除；Rhai 参数/receipt 映射留产品层且 API 仍无限制。all-feature crate tests 与两 crate
warnings-denied Clippy 通过。
Toolbar 去耦叶确认三平台所谓 native toolbar 仅是 AgenTerm action-ID 映射，因此合并为
无 OS cfg 的产品 `NativeToolbarHit`，删除三套伪 native adapter；Windows/Unix hot path
与顺序测试消费同一表。它不会进入外部 platform crate，也不再冒充 OS mechanism。
Display discovery 随后进入 crate `window` facade：公开 X11/Wayland/headless facts 与
runtime capability status，环境探测只在三平台 selected adapters；root 删除自己的 facts
类型并消费公开 contract，为移除旧 native mod/scale/IME compatibility tree 建立前置。
Unix compatibility-tree 清理叶随后删除 root Linux/macOS 的 native activation/input/IME/
scale/screenshot 转发层；Unix frontend 与两套 Control Center shell 直接消费 crate 的
activation、input、IME、window 和 display facade，`platform_info_json` 也不再回调 root
selected native module。Windows-hosted all-target Clippy 与 31 项 crate all-feature tests
通过；Linux target probe 因本机缺少 `x86_64-linux-gnu-gcc` 在 `ring` build script 前置
阶段停止，因此 Unix frontend 的原生目标编译仍需 Linux/macOS host/CI 补证。生产边界
门禁已由 34 行收敛到 16 行，剩余归属明确为 Unix winit/softbuffer host、Windows
frontend/Control Center/remote renderer 与 root target selection，不能以本叶冒充总收口。
Windows native screenshot 叶把 bounded GDI window/client capture、BGRA conversion、
resource RAII 与 PNG 写入迁入 crate Windows adapter；公开 API 只暴露 unsafe-constructed
opaque window handle、neutral capture area、typed result/failure。Linux/macOS 同一调用明确
Unsupported。remote GUI 与 Control Center 已真实消费该 API，root screenshot 文件及旧
clip contract 删除；Control Center 的重复 MoveFileExW/activation 也改为 crate facade。
34 项 crate all-feature tests、root warnings-denied Clippy 与 Control Center 聚焦测试通过，
生产边界门禁剩余 13 行。
Windows native projection cleanup 随后让 remote GUI 直接消费 crate activation/input，
保留原有 Control/AltGr、UTF-16 decoder 和 show/restore typed diagnostics；root 最后的
`adapters/windows/native` 目录与 selected native module 已删除。48 项 root platform
聚焦测试和 warnings-denied all-target Clippy 通过。此叶去除重复包装，但 Windows GUI
host 自身的 Win32 event/render 类型仍待迁入 crate adapter。
Windows launcher-mechanics 叶新增 crate application-wake 与 parent-console diagnostic API：
`PostMessageW(WM_APP+1)`、standard-handle probe、attach-existing-parent-console 和 cleanup
都进入 target adapter，root launcher 只组合 `WakeSignal` 与产品参数/IPC handoff。两级
warnings-denied Clippy、35 项 crate tests 和 3 项 launcher parser/guidance tests 通过；
root `frontend.rs` 不再含 Win32 类型或调用。
Unix host 审计同时发现 macOS adapter 曾把 Control 与 Command 都判为产品 primary
shortcut，会抢占 Ctrl-C 等 terminal control keys。修复将 macOS policy 收紧为 meta/
Command-only，并加入平台中立回归测试；这不是授权策略，只是输入仲裁正确性。
Windows Control Center shell 叶新增通用 `NativeTextWindowHost` extension boundary：crate
Windows adapter 现在拥有 window class/create、timer/message loop、GDI text paint、focus/
close/title/invalidate；root 只把 Control Center 产品 host 的 title/lines/poll/screenshot
映射到中立 trait。旧 Win32 shell 文件被替换为产品 bridge。Linux/macOS runner 暂时
明确 Unsupported，不能冒充对应 shell 已迁移；下一叶将以 pixel-surface host 接入。
紧随其后的 Unix shell 叶已完成该接入：winit event loop/window、softbuffer surface、
raw window identity、resize/present、focus、200ms poll 与 renderer-owned frame receipt
迁入 crate 的 shared Unix adapter；root services 只保留一个三平台产品 bridge，三套
OS shell selector 文件删除。Windows host 上的 Linux-target crate all-feature Clippy
以 warnings denied 通过，并同时修复 PTY private-error destructure、clipboard timeout
调用与陈旧 IPC/product-path test 等此前未被 Windows 编译发现的问题。总边界门禁剩 8 行。
Dependency isolation 叶把 `windows-sys` 的全局 Win32 feature union 拆到各 capability
feature：process/filesystem/locking/ipc/window/clipboard/screenshot/font 只转发自己的
模块。默认、8 个单 feature compile checks 均通过；Windows 上 `cargo tree` 证明最小
process 与 filesystem 均只有 `windows-sys → windows-link`，不再隐式带 UI/GDI/clipboard。
Frontend host 前置开始收敛：`WindowSemanticState` 及 minimized-over-maximized precedence
从 root 产品 helper 迁入 crate `window` public contract；root 的 320×240 CLI resize policy
继续留在产品层，避免把 AgenTerm 参数规则伪装成通用平台限制。
Normalized frontend event 前置新增稳定 public `NormalizedKeyEvent`：logical named/character,
bounded physical identity、pressed/released、repeat、committed text 与 modifier snapshot 均不含
winit 类型。Shift+Tab contract test 固定为 named Tab + Shift，后续 Unix runner 与 Windows
host 必须在 adapter 内完成原生 event 转换；composer/tmux/PTY 字节策略仍属主 crate。
Linux/macOS selected input adapters 随后实现该转换：winit logical/named/physical key、
ElementState、repeat、committed text 全部在 crate shared Unix adapter 归一化；公共 extension
trait 的签名只含 crate 类型。Linux-target all-target Clippy 已编译 Shift+Tab/letter/digit
mapper test targets；本 Windows host 未执行 Linux test binary。root Unix consumer 集成叶也已
完成：事件循环入口把 native key/modifier 一次性归一化，composer、文本框、terminal shortcut
与 PTY byte policy 只接收 crate contract 类型，产品 input 模块不再导入 winit。Windows-hosted
all-target compile 与 39 项 crate all-feature tests 通过；Linux root compile probe 在进入本叶
源码前被缺少 `x86_64-linux-gnu-gcc` 的 ring build script 阻断，native execution 仍须 Linux CI
补证。严格生产边界门禁由 8 行降至 7 行，余项是 Unix window host、Windows remote GUI host
和最终 root selector。
Unix window-state product leaf 随后移除 `window_state.rs` 的 winit event/window/size
类型：产品 `ui-action`、snapshot 与 semantic tracker 只依赖中立 `UnixAppWindowHandle`
行为和 crate `WindowSemanticState`，原生适配暂集中到唯一剩余 frontend host 文件，等待
pixel-window runner 接管。Windows-hosted all-target compile 通过，严格生产边界门禁由 7 行
降至 6 行。
Unix wake integration leaf 把 `EventLoopProxy` 从 root wake bridge 移除：PTY/IPC producers
只调用一个 neutral, coalesced GUI wake callback，当前 frontend host 暂时安装 native closure，
后续直接换成 crate `WindowWaker`。同叶把 IME 与 wheel 产品处理入口改为 crate IME event 和
中立 vertical-delta/line-mode 参数，原生枚举只在剩余 host event boundary 转换。Windows-hosted
all-target compile 通过，严格生产边界门禁由 6 行降至 5 行。
Unix pixel-window host leaf 随后完成该接管：公开 `PixelWindowApplication`、中立窗口控制、
normalized event、XRGB frame、跨线程 `WindowWaker` 与 typed `Unsupported`/`Failed` contract
由 `agenterm-platform` 提供；Linux/macOS adapter 独占 winit event loop/window、softbuffer
surface、resize/buffer/present 与原生事件转换。root Unix frontend 只保留产品
layout、terminal、selection、command 和 screenshot policy，根 `Cargo.toml` 删除 winit/
softbuffer 直接依赖。Windows-hosted all-target check、crate all-feature warnings-denied Clippy
和 46 项 crate tests 通过；root Linux source compile 仍在进入源码前被本机缺少
`x86_64-linux-gnu-gcc` 的 ring build script 阻断。严格生产边界门禁由 5 行降至 3 行，剩余
仅为 Windows remote GUI host 和最终 root selector。
Windows remote GUI 的最终拆分采用独立 `control_window` host，而不把 Win32 child controls
硬塞进 Unix pixel runner。依赖顺序固定为：中立 control/control-event/canvas contract → crate
Windows class/window/child-control/message-loop/GDI host → root product state 改接 control IDs、
normalized events 和 canvas → 删除 root HWND/HDC/RECT/windows-sys → 删除最终 root selector。
crate host 独占 key preview（发生在 TranslateMessage 前）、capture loss、deferred destroy、一次
BeginPaint/double-buffer present、系统菜单、焦点/控件文本和 native capture；主 crate 保留
server/client、tabs/tree/composer/settings/theme、selection/scrollback、close policy、snapshot 与
绘制组合。该分层明确排除接受 raw integer/closure 的临时薄 facade，也禁止 crate 反向出现
Agenterm action、theme、Control Center、Fleet 或 protocol 类型。
该依赖图的 contract/native-host 叶现已落地：crate 公开 neutral control IDs、controls、
FocusTarget、pre-translation consumable key preview、minimized resize、pointer/double-click、
poll/system-menu events、control-window operations 和 `ControlCanvas`；Windows adapter 实现
class/window/child controls、timer/message loop、deferred destroy、UTF-16 text decoding、完整
terminal named-key normalization、capture/cursor/focus/control text 和单次 GDI double-buffer
present。零尺寸 paint 被跳过，surface/present/menu failures typed，class style 仅保留
`CS_DBLCLKS`。Linux/macOS 明确 Unsupported。随后 `d9138ab` 完成 root product controller
接入：remote frontend 只含 control IDs、normalized events、typed queries 和中立 canvas，
Win32 import/handle/unsafe 搜索为零，12 项 owning tests 通过。
后续 host 语义增量已以 `d81ce70` 推送：native EDIT copy/paste 保持选区和插入点，截图在
capture 前同步刷新待处理 redraw，避免 snapshot 与 PNG 错帧。`f85ffeb` 则把私有状态目录/
exclusive state file 建模为 filesystem feature 的公开能力，Unix 请求 `0700/0600`，Windows
保留继承 ACL；all-feature 50 项测试和最小 filesystem 3 项测试均通过 warnings-denied
Clippy。
同时，root selector 的 IPC、script-host、supervisor-audit 与 XRGB screenshot 分支已改为
cfg-free product policy/直接 crate facade，12 个重复 adapter 文件删除。`2644ba7` 随后完成
TLS、Control Center 和 frontend：删除 root `selected.rs`、三套 Control Center/TLS adapter
与两层 Unix wrapper；cfg-free service 按 `PlatformKind` 调用两套产品 extension，Windows
ureq 依赖树只含 NativeTls，Linux 只含 Rustls。根 `windows-sys`/`rmux-pty` 依赖删除。
Windows-hosted all-target check、warnings-denied Clippy、458 项 lib tests、80 项 Unix frontend
tests 和 7 项 strict boundary tests 通过，production native-boundary findings 为零。
Windows batch aliases 由 `.gitattributes` 强制 CRLF；下一步是集成后的 Quick/build/public
smoke 串行终检，而不是继续保留故意红色门禁。
首轮 `remote-ui-smoke` 又暴露两项真实兼容缺口：跨进程直接发送的 WM_KEYDOWN 绕过
pre-Translate preview，以及 host 重映射 system-menu ID 使稳定 Copy/Paste command 失效。
`a9f1c90` 仅为直接发送消息补 normalized preview、避免队列键双分派；`d056888` 验证并保留
`1..0xF000` 的稳定 menu ID。51 项 crate tests 通过，随后完整 `remote-ui-smoke` 通过 detach/
reconnect、树与滚动、Settings/CWD、terminal selection Copy/Paste 和 server recovery。

2026-08-01 首个建设期增量：Cockpit snapshot 新增明确的
`tab_counts.{total,running,dead}`，native shell 同源显示 logical instance、
server PID/version、build commit/profile/cleanliness、epoch/sequence、active stable tab ID/title 和四类 component
availability。390 项 Quick tests、七产物 dev build 和完整 Windows
`control-center-smoke` 通过；加入 build identity 行后的 renderer-owned
760×480 PNG 为 43,509 bytes，
no-activate、因果刷新、server recovery、typed close 与 orphan-free cleanup
保持通过。native renderer inspect/select 导航和 Linux 原生 renderer 证据仍是后续叶。

2026-08-01 第二个 Cockpit 纵切已合入前验证：`agenterm-cc inspect/select --tab
@ID` 只接受 canonical stable ID；inspect 保持当前 selection，select 复用
server-owned `select-window` typed control receipt，并在同一 PID/epoch 上重读
权威状态后返回 typed tab facts 与 `post_state_verified`。聚焦 parser/contract
测试 28 项、391 项 Quick tests、七产物 dev build 和完整 Windows
`control-center-smoke` 通过；新增 `control-center.typed-navigation` 证据，
renderer-owned 760×480 PNG 为 58,125 bytes。headless server 下 missing target
保持 active tab 且不创建 CC registry。native Cockpit pointer/keyboard 导航仍
未实现，不以 CLI entry point 冒充 renderer 交互。

2026-08-01 live dogfood 新增阻断项（结论与修复证据必须回写对应 PRD）：

本轮收口依赖图冻结为：`既有修复/证据复核 → {渲染时间域与字号 resize，
输入/选区，server detach/Error 5}`；三支只读审计可并行，但 Windows remote
frontend 是共享热文件，任何实现必须由主线串行集成。渲染支先证明 invalidate、
paint、focus、font 与 PTY resize 的因果链，再补真机 telemetry/PNG；输入支先区分
named-key 编码、Win32 投递和 selection 生命周期，再补真实 terminal byte/动态输出
黑盒；server 支先复核独立 spawn、instance discovery、lease detach 和 workspace
恢复，再以隔离地址验证同 PID/epoch/tab。最终串行路径固定为聚焦单测 → Quick →
dev build → 直接归属的 Windows smoke → clean diff/status；Candidate、tag、RC 和
Release 全部是本轮明确非目标。

- [~] P0：Windows terminal 内容与 native frame 持续闪烁；先区分无状态变化的
  redraw/invalidate loop、背景擦除和 resize/DPI feedback，不以降低刷新率掩盖。
  白箱审计已定位 replaceable GUI 直接在 window HDC 上清空四区后逐层画回，
  没有 offscreen frame + single BitBlt；高输出 delta 约 10Hz 暴露半成品帧。
  此外 2 秒 lease heartbeat 被误算为 visual change，idle 也触发全窗重绘；
  `CS_HREDRAW|CS_VREDRAW`、NULL dirty region 与同尺寸 resize 是次级放大器。
  第一叶已实现 heartbeat/redraw 类型解耦：lease maintenance 仅返回成功/失败，
  tick 只有收到真实 delta 才报告 visual change；Windows paint 先在兼容 memory DC
  组成完整 client frame，再以单次 `BitBlt` 提交；像素预算、back-buffer 分配或提交失败
  都 typed fail 并关闭受影响的 replaceable window，不静默退回会重现半帧闪烁的 direct
  paint 路径。聚焦双缓冲边界测试与 UI-client tests 已通过。
  集成态 `check.cmd --quick` 已通过 repository lint、fmt、alignment、warnings-denied
  all-target Clippy 与 396 项 library tests，七产物 dev build 通过；两次直接归属的
  `remote-ui-smoke` 均完成 resize/minimize/restore、Settings、PTY 和 renderer-owned
  screenshots，但随后在“关闭 GUI 后保留 server”阶段发现 server 已退出；
  2026-07-31 的保留运行在同阶段同样失败，证明它不是本次帧提交回归。用户 live
  dogfood 随后确认默认 Keep Server Running 也真实丢失旧 server/session。白箱定位
  GUI 自启动 server 未脱离 Script harness 的 kill-on-close Job；修复让 GUI 复用
  `platform::process::autostart_server`，Windows adapter 统一赋予 null stdio、
  `CREATE_NO_WINDOW|CREATE_BREAKAWAY_FROM_JOB`。修复后首轮已通过 retain/replacement
  阶段但在更晚 scrollbar return-live 检查波动失败，第二轮完整 `remote-ui-smoke`
  通过同 PID/epoch/tab/PTY/draft 接回、scroll/selection/clipboard、server crash
  recovery、Stop Server 和 orphan-free cleanup，重新产出 `ui.replaceable-client`
  等 15 项 evidence。
  第二叶已在本地集成：resize 先比较权威 screen 网格，并以 server epoch + stable
  tab ID + rows/columns 去重尚未进入 delta 的在途请求；同网格不再穿过 IPC，新的
  epoch/tab/grid 仍会发送。Win32 class 同时移除与 `WM_SIZE` 显式 invalidation 重复的
  `CS_HREDRAW|CS_VREDRAW`。纯测试覆盖 current/in-flight 去重及 epoch/tab 失效。
  后续白箱 diff 又确认平台 host 抽取时丢失旧实现的 `WS_CLIPCHILDREN`：父窗全客户区
  `BitBlt` 会覆盖 native EDIT/BUTTON，再与 child 自绘交替，直接造成边框和内容闪烁。
  top-level style 已恢复 child clipping；重复 layout 对未变化 child bounds/visibility
  也会 no-op，不再无条件触发 `MoveWindow`/`ShowWindow` paint storm。Windows host 的
  style/geometry contract tests 固定这两条边界。
  用户观察到 smoke 的白底阶段明显更卡；同一 window/modal/screenshot 路径的两组
  Dark/Light A/B 为 528/572 ms 与 663/553 ms，Light 没有稳定慢路径，更不是 4x。
  smoke 恰在持久化 Light 后进入 CWD/OSC7、层级 mutation、8-tab dense fixture、80 行
  output、scroll/selection 和 server recovery 的重负载半程，颜色与负载阶段高度混杂。
  为避免未来再次用阶段位置解释主题体感，`remote-ui-smoke` 现增加同窗口、同操作的
  Dark/Light/Light/Dark 对消测量，记录耗时、redraw 与 paint 增量；双向 2.75x 耗时
  阈值会拒绝稳定 3--5x 主题差异，并以 `ux.theme-render-parity` 进入 qualification/alignment。
  首次 current-tree 复验同时修正了 render activity sampler 接受任意历史
  `sequence > 0` 的陈旧基线：采样现在先读取当前 sequence，再要求锁存结果严格递增。
  `06149c6` dirty=false dev artifact 的完整 journey 用时 41.2 秒，ABBA totals 为
  Dark 1095ms / Light 1203ms、双方 10 redraw / 8 paint；zoom 为 23/7、idle 为 1/1、
  high-output 为 3/4，并成功发出含 `ux.theme-render-parity` 在内的 16 项 evidence。
  最新 clean `78eac9e` dev artifact 又以 63.4 秒完成同一 owning journey：ABBA totals
  为 Dark 1349ms / Light 1237ms、双方 10 redraw / 8 paint；zoom 23/7、idle 1/1、
  Light 高输出 3/3，并继续通过选择复制、普通与 bracketed paste、同 server/PTY
  detach/reconnect、server recovery 和最终无 orphan 清理。这是当前 SHA 的自动化时间域
  回执；持续高输出肉眼确认仍保持开放，不能由计数器替代。
  修改后的 owning journey 又以 60.3 秒通过：连续 z/Z 和 grid 收敛后直接经 GUI
  Shift+Tab 路径向 PTY 精确增加 3 bytes，再继续通过 shell marker、selection/copy、
  paste 与 detach/reconnect。selection 的结构状态会先于低优先级 `WM_PAINT` 发布，
  PNG 回执现 bounded 等待真实像素变化，整个 deadline 内不变化仍 fail closed。
  后续按 `commands.json` 时间线确认用户体感真实：三轮黑底阶段 15.0--18.3s，白底后
  108.6--122.5s，相邻 snapshot 中位间隔上升约 1.8--2.1x。根因不是 palette，而是
  harness 每条 CLI 都 parse + pretty-write 全部历史形成 O(n²)；每 50 条命令的中位
  间隔从 213--243ms 恶化到 815--1000ms。记录器现改为单条 bounded JSONL durable
  append、即时 bounded checkpoint、cleanup 时一次 compact JSON array promotion；旧日志
  延迟掩盖的 server→GUI 依赖也改为 observed-sequence barrier。相同完整 journey 连续
  通过，精确计时由 169.9s 降至 36.787s（4.62x），且仍切换 Light、保留原 15 项
  evidence。这解释了“白底测试慢 3--5 倍”，同时不替代 paint/invalidate 独立观察。
  原生 host 现增加单调 `redraw_requests`、`parent_paints`、child bounds/visibility
  update/skip 计数；仅由测试消息主动采样并锁存到 `ui-snapshot`，读取计数不会形成
  snapshot→redraw→snapshot feedback。精确集成树的两轮完整 `remote-ui-smoke` 分别以
  126.5 秒和 134.8 秒通过。第二轮在 19 次 native z/Z 后实测 23 次 redraw request、
  8 次 parent paint、0 次 child bounds/visibility 更新且 no-op skip 增长；随后 500ms
  idle 仅 1 次 redraw/paint、仍为 0 child 更新。paint/invalidate storm 的自动化关闭
  条件已满足。用户已接受最新 dogfood 的持续输出视觉表现作为 v0.1.12 结果。后续 journey 又在持久化 Light 主题下对真实 80 行 PTY burst 取样：先等
  GUI lease observed server position，再等 250ms paint queue 收敛，实测仅 4 redraw/4
  parent paint、0 child update。自动化现覆盖 zoom、idle 与 high-output；后续视觉回归
  作为普通维护项处理，不把计数器冒充视觉感受。
- [x] alternate-screen harness 无法本地向上滚动。byte-level ConPTY probe 证明旧 Windows
  会吞掉 `1049h/l` 并只输出与普通 clear 同形的 full-frame repaint，因此删除了依赖
  `alternate_screen=false`、`max_scrollback==0` 或 repaint pattern 的猜测。platform PTY
  contract 现发布 `Cooked/RawVt/RawNative` 输入所有权和 typed logical Up/Down：cooked
  shell 不接收历史键，raw child 由 ConHost 按内部 application-cursor mode 编码 CSI/SS3；
  POSIX adapter 对 native console key 明确 Unsupported，Unix 继续使用 parser-owned mode。
  Windows owning journey 在保留普通 scrollbar/wheel assertions 的同时，用真实 raw
  PowerShell PTY 验证每个上下 wheel notch 分别交付三组 `ESC O A` / `ESC O B`，并在
  169.9 秒 integrated `remote-ui-smoke` 中通过后续 selection、server recovery 与 cleanup。
  raw-mouse reporting arbitration 仍是独立后续 slice，不反向否定本次 paging 收口。
- [x] terminal focus 下 `Shift+Tab` 修复正在集成：共享 named-key encoder 已按
  xterm modifier 参数覆盖 Tab/方向/Home/End/Insert/Delete/Page/F1–F12；Unix
  两条输入路径保留 modifiers，Windows `WM_KEYDOWN` 显式处理 Tab/Insert/Delete
  并屏蔽 Tab/Escape 的 `WM_CHAR` 重复回声。commands 与 Windows mapping 聚焦
  单测、395 项 Quick library tests、all-target Clippy、alignment partial
  fail-closed 集成测试与七产物 dev build 通过。owning Windows journey 现会在
  terminal focus 下投递 Shift+Tab window shortcut，并从公开 pane counter 断言 PTY
  恰好多收到 3 bytes；编码契约固定内容为 `ESC [ Z`。用户已接受最新 dogfood 结果作为
  v0.1.12 物理交互收口；不再把自动化投递冒充物理证据。白箱复核同时确认这不是可用另一种 headless
  API 补齐的假缺口：Win32 `GetKeyState` 读取目标线程消费 keyboard message 时的状态，
  `SetKeyboardState` 只修改调用线程，而 `SendInput` 写入全局 foreground input stream；
  为制造“物理 Shift”而抢前台会直接违反全 smoke 的 `AGENTERM_NO_ACTIVATE=1`。因此保留
  automation 的精确三字节证据，并把真实键盘验收明确交给最新 dogfood binary。
  Windows host 的 Linux `cargo check` 仅因缺少
  `x86_64-linux-gnu-gcc` 停在 `ring` 构建脚本，Unix adapter 仍需原生 CI/host
  证据。后续 macOS control-key 修复曾把所有 named key 提前于 committed text，导致
  无修饰 Space 从 `TextCommit(" ")` 回归成 `ControlKey("Space")`；精确 HEAD 的
  `--skip-smoke` 捕获该失败。共享契约现只让可打印 committed text 优先，Enter/Backspace/
  Escape 的 native control-character echo 仍保持 named control。crate/root 两级聚焦表驱动
  测试固定 Space、Enter 与既有 Shift/Unicode 语义。
- [x] Windows toolbar `z/Z` 字号按钮造成 terminal 看似“无响应”的代码路径与自动化证据已闭环：native
  child button 点击后曾持有 Win32 keyboard focus，而 terminal input 只在 top-level HWND
  获得 focus 时消费；即时 toolbar action 现在显式归还 terminal focus，modal-opening 与
  Control Center action 不会被错误抢回。native focus query 不再用旧 logical surface 掩盖
  child-control focus，`remote-ui-smoke` 在字号点击后直接断言真实 focus。绘制同时恢复选入
  HDC 的旧 font/background mode，避免反复换字号时旧 `NativeFont` 无法删除。聚焦 unit test
  、warnings-denied all-target Clippy、七产物 dev build 与完整 `remote-ui-smoke` 通过；后者
  在真实 native `Z` click 后验证 terminal focus，并继续完成 PTY 输入、字号继承、GUI detach、
  同 server/session 重连及最终 Stop Server cleanup。`WM_COMMAND → synchronous PTY resize`
  的白箱复核又发现 native resize 一次失败会污染 terminal fatal error、永久拒绝后续输入，
  同时 server 仍提交虚假的 `terminal.resized`。该错误边界现已修正：native 接受后才提交
  parser 网格、`last_size` 与 journal；拒绝返回 retryable typed failure 且不毒化 terminal。
  两项聚焦回归覆盖失败保留旧网格与成功后原子提交。GUI resize 现也完成独立异步硬化：
  Win32 event thread 只计算目标 grid 并覆盖一个 latest-only 待发送槽，单一 owned worker
  串行执行 bounded IPC；每个结果绑定 lease/client PID/server epoch/tab/grid，重连前或已被
  更新尺寸取代的结果不会污染当前状态。公开 `ui-snapshot` 同时报告 current/desired grid 与
  pending convergence。新增 worker 单测证明首个 IPC 被阻塞时调用侧不阻塞、相邻中间尺寸被
  丢弃且最终尺寸必达；462 项 library tests、all-target warnings-denied Clippy、七产物 dev
  build 及 95.9 秒完整 `remote-ui-smoke` 通过。owning journey 连续操作 native z/Z 18 次，
  等待 grid 精确收敛后立即验证 PTY 输入，并继续通过选择复制、detach、同 session 重连、
  server fault recovery 与最终 cleanup。后续原生绘制计数 smoke 又固定 19 次 z/Z 的
  23 redraw/8 paint、0 child update，以及 500ms idle 的 1 redraw/1 paint；持续高输出的
  人工视觉确认仍归上方闪烁验收，不再把它混入字号输入失响应的代码状态。
- [x] 默认 `Keep Server Running` 不再因调用者 Job cleanup 杀死独立 server/PTYS；
  GUI 与 CLI 统一走 platform process facade，完整 replaceable-UI 黑盒已证明退出、
  detached lease、同 server/session 接回与最终显式 Stop Server。live dogfood 又发现
  从不允许 breakaway 的上层 Windows Job 内启动时，`CREATE_BREAKAWAY_FROM_JOB` 会直接以
  error 5 拒绝创建 server。platform process facade 现在只对该精确错误重试 caller-job
  fallback，并以 `DetachedSpawnMode` 和 parent-console diagnostic 明示降级；其他 spawn
  failure 不会被吞掉。40 项 crate tests、两级 warnings-denied Clippy、七产物 build 和
  isolated native GUI/server/PTY 启停 probe 通过；fallback server 可能随上层 owning Job
  结束，不能被文档冒充为完全 independent。
- [x] terminal 鼠标选区建立与 copy/paste 的本轮 dogfood 阻断已闭环；复查
  selection ownership、drag threshold/capture、raw-mouse arbitration 和复制黑盒。
  白箱审计定位当前选区绑定整个 `screen.generation`：持续 output delta 在 100ms
  reconcile 时清空 drag/completed selection，paint/copy 也因 generation 不等而
  拒绝；drag 中取消还可能漏掉 `ReleaseCapture`。修复需区分 same-grid 内容推进
  与真实尺寸/tab 失效，缓存完成态复制文本，并在 drag 中注入输出验证 phase、
  Ctrl+C/system-menu Copy 和 capture release。现有“先等输出静止再同步拖拽”的
  smoke 不足以证明该行为。generation/capture/cached-text 修复已经存在，但 owning
  smoke 现已补成 pointer down 后经 public CLI 注入唯一 PTY delta、等待 GUI reconcile，
  再观察 native capture + paint 共源 highlight bounds、same-event-position PNG 差异和
  completed capture release；direct GUI Ctrl+C 更新 clipboard 且 PTY input bytes 不增加，
  system-menu Copy 仍保持等价。只有 non-empty completed cached selection 才接管 Copy，
  prepared/empty state 不吞 PTY interrupt。498 项 lib tests、warnings-denied Clippy、dev
  build 与 47.7 秒完整 Windows `remote-ui-smoke` 通过。拖出 viewport auto-scroll、
  双击/三击与完整 CJK 物理交互仍是上层
  professional-selection 的独立未完成叶，不反向否定本轮选择复制修复。
- [~] 本地开发缓存膨胀的第一段已闭环：实测 `target/` 15.2 GiB，其中
  `target/debug/incremental` 10.53 GiB；一次显式 `cargo clean` 已回收全部可再生缓存。
  dev `build` 现在只在七产物成功 staging 后调用 `prune-target-incremental`，持有真实
  `debug/.cargo-lock` 并逐项取得 rustc session lock，按 compilation-unit root 保留最新
  finalized session、删除可证明失效且超过 60 秒的旧 session；缺锁、锁占用、working、
  reparse/symlink 或变化中的目录均 fail closed。3 项隔离测试覆盖 newest retention、
  Cargo/rustc lock contention 与释放后重试。此前审计估算该层可回收约 4.30 GiB；仍约
  6.23 GiB 来自不同 root generation。prune consumer 现已接受严格 versioned manifest，
  仅让 `touched=false`、before/after/锁内二次全树 metadata identity 一致、无 working/
  indirect/special entry 且全部 session locks 可持有的 root 删除；5 项 Windows 隔离测试
  覆盖成功删除与 touched/缺失/损坏/无 rustc/incomplete/mismatch/active-lock 保留。mtime
  只作 TOCTOU identity，绝不推断 usage。native Windows dev lane 现已用 repo 外稳定
  bootstrap worker 作为真实 `RUSTC_WRAPPER`：首次带 `-C incremental` 的 rustc 调用在
  Cargo lock 与 producer barrier 内冻结 before snapshot，逐次记录精确 touched root，成功
  staging 后才原子生成 manifest 并交给既有 consumer；hot build 没有 rustc invocation 时
  manifest 明确无效且授权删除 0 个 root。wrapper/finalizer 隔离测试、9 项 producer/consumer
  回归、warnings-denied Clippy 与连续两次 `build.bat` 已通过；第二次复用相同 worker，bootstrap
  Cargo 为 0 ms，总时长从 31.3 秒降到 11.8 秒。首次构建另回收 69 个旧 session、约
  1.20 GB logical bytes；真实工作树上可删除的不同 root generation 数量与实际字节回收量
  尚未出现，因此缓存膨胀叶仍为 partial，不以隔离 fixture 冒充生产回收证据。后续 clean
  `c65bf36` 原生 dev build 在 `target/debug/incremental` 为 2,281,521 KiB 时又真实删除
  46 个 finalized sessions、590 个文件、402,592,437 logical bytes；本次没有 rustc invocation，
  whole-root manifest 明确为 invalid、授权删除 0 个 root。该回执证明普通热构建会安全回收
  session 垃圾，同时如实保留“整 root generation 尚无生产删除样本”的剩余证据缺口；不为
  制造 `removed_roots > 0` 重跑构建或伪造 touch manifest。
- [x] `agenterm-platform` workspace 抽取后的集成门禁已真实收口：供应链任务不再假设单
  workspace package，而是动态排除全部 workspace members、验证两个 crate 的外部直接依赖
  并集并生成 275-package SPDX；补齐 macOS `objc2-app-kit`/`objc2-foundation` MIT notices。
  qualification evidence declaration 校验前移到 repo lint 后、主编译前，Control Center 的
  `typed-navigation` 已进入精确清单；递归 quality-timing 测试从 broad Cargo invocation
  分离并在同一 unit gate 串行执行，消除 target-lock 竞态。build identity 冻结为
  `f0f0248` 的 `check.cmd --skip-smoke` 212.7 秒通过，包含 463 library tests、all-features integration
  tests、七产物 dev build、MCP、migration、SPDX、qualification/package/cleanup self-tests；
  按约定不写 qualification receipt，也未触发 Candidate/Release。其后并发合入的
  `9f3f9de` hardware-only crate 增量另由 dirty=false 七产物 build（24.1 秒）、完整 Windows
  `remote-ui-smoke`（130 秒）、platform all-feature warnings-denied Clippy 与 68 项 tests
  覆盖；不把跨并发提交的分层证据伪称为同一次 exact-tree full gate。
  后续 native IPC descriptor interop 增量的整合 run `30718584882` 被既有静态门禁
  精确拦截：public `ipc.rs` 出现 `cfg(windows)` 与 `std::os::windows`。修复没有扩白名单，
  而是把 borrowed/owned handle/fd 标准 trait 和 `NativeStreamExt` 物理移入 selected
  Windows/Unix adapters，facade 只重导出目标 trait 并保留中立 wrapper；platform 161 项
  tests、native round-trip 与 macOS cross-target Clippy 是重新验证门槛。

这些 dogfood 缺陷优先于新增 Cockpit 装饰和远期 Candidate 工作；修复必须保留
结构化 snapshot 与 PNG/公开 input journey 证据，并避免多个 agent 并发编辑
Windows remote frontend 等热文件。

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
│  ├─ [x] agenterm-rhai 已交付真正的持久 REPL
│  │  ├─ ReplSession 会话内核与 CLI 输入适配解耦，可供 CC/Agent 复用
│  │  ├─ 变量、函数、多行单元、错误恢复、reset 和内存 history
│  │  ├─ TTY 提示与 pipe/NDJSON 自动化输出分离
│  │  ├─ 单元失败不提交语言状态，外部真实副作用不伪装回滚
│  │  ├─ 普通 worker 与 REPL 复用同一 Engine/API 配置
│  │  └─ [~] 长驻 REPL hardening：supervision/Ctrl+C 已交付，箭头键编辑/history 仍开放
│  │     ├─ bounded foreground worker 已证明同 PID 顺序调用、typed crash/EOF、显式 replacement 与 reap
│  │     ├─ child-session wire、O(1) validator 与 worker session thread 已覆盖 Open/Inspect/Evaluate/Query/Reset/Cancel/Close
│  │     ├─ worker 内已证明 state persistence、pre-start Cancel、broker/legacy 隔离与 Close/EOF join
│  │     ├─ `e731ee3` 已接入 parent supervisor；公开 direct/Windows hosted 测试证明 32 cells 同 PID、cell 33 前换代、新 PID/generation 与 fresh receipt/no-replay
│  │     ├─ Windows `script.repl-supervision` 证明 hosted Ctrl+C cooperative recovery、150ms non-cooperative hard kill/reap、nested child 无 orphan、同一 outer CLI fresh-session 续跑
│  │     └─ Linux/macOS 只声明 direct `agenterm-rhai` protocol 与 compile/unit coverage；Unix hosted CLI/native interactive Ctrl+C 仍需各自 public journey
│  ├─ [x] v0.1.12 保留 canonical `agenterm-rhai` 名称
│  │  ├─ 当前没有完整外部调用者使用量与迁移/移除证据
│  │  ├─ 收敛期不新增同义 executable、package 和 Candidate surface
│  │  ├─ 未来 rename 必须先冻结 CLI/task/worker/package/docs 调用者清单
│  │  └─ 兼容入口只能转发到同一 unrestricted runtime，不能复制实现
│  ├─ Control Center 的 Workflows/Extensions/InfoHub 保持真实空状态
│  ├─ agenterm-net 启动 N2 受控纵切（独立常驻 full node）
│  │  ├─ 显式 start/status/stop；持久身份、block store 与可观测资源账本
│  │  ├─ DHT、pubsub、relay 各自 capability / budget / 失败语义，不以编译成功冒充可用
│  │  ├─ 仅用户拥有节点间、显式配对的 read-only Remote Fleet attach
│  │  ├─ GUI / server / PTY 不链接 libp2p；网络 node 崩溃不得影响本地 Fleet
│  │  └─ 先证明本机与两节点闭环，再讨论公网默认、远程控制或稳定发布
│  └─ system-WebView / Tauri-compatible host spike 启动
│     ├─ 首个本地打包、只读 Cockpit Web UI；native CC 仍是可靠 fallback
│     ├─ Windows WebView2 / macOS WKWebView / Linux WebKitGTK 分别实测
│     ├─ 量化 EXE、archive、runtime 依赖、冷启动、RSS、DPI、截图与崩溃恢复
│     └─ bridge 只允许 versioned facts / fleet snapshot；无 eval、shell、网络逃逸
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
   ├─ 未经 N2 跨平台、恢复与资源证据即把 agenterm-net 宣称为 stable full node
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

进入本节的代码结构前置已经收口：revision-4 Platform Facade 是生产原生
能力的唯一边界，IPC endpoint/transport 通过 typed contract、service、
selected adapter 装配；遗留的 Unix socket / Windows named-pipe 实现副本已
删除。这里剩余的是三平台原生运行证据与混合版本行为，不是再次创建平台分支。

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
- 2026-08-01 六平台 CI 复核发现 Unix `build.sh` 按契约只保留
  `target/<triple>/debug`，但 Linux/macOS compatibility 与 macOS Control Center
  步骤仍引用从未 stage 的 `dist/*`；现改为消费 matrix 的真实目标目录，并由
  `write-build-metadata` 按 `artifacts.json` 的精确 OS/arch executable set 生成
  commit/hash receipt，`native-ipc-compat-smoke` 显式校验该 receipt，未降低
  exact-source 门禁。macOS 七段计时也改用可移植的纳秒时钟。arm64 native IPC
  失败另被定位为 smoke 以 Linux/XDG 规则断言 macOS settings path；子进程现使用
  隔离 HOME，并按 `Library/Application Support` 原生约定验证。Windows owning
  native IPC smoke、平台 receipt 自测和完整 Quick（469 tests）已通过；这些修复
  仍须由新 main SHA 的 Linux/macOS matching-host CI 关闭，不能预先记为六平台绿。
- 2026-08-02 clean SHA `274f971` 的 Windows owning rerun 已再次通过：
  `native-ipc-smoke` 用 7.8 秒证明 named pipe、显式 TCP、selector precedence、
  schema-v1/v2 discovery、PID-reuse/stale cleanup 与 exact authority recovery；
  `native-ipc-compat-smoke` 用 20.7 秒校验 published v0.1.10/v0.1.11 exact bytes，
  并完成旧 TCP ↔ HEAD、v0.1.11 native → HEAD upgrade、HEAD state write 及
  v0.1.11/HEAD rollback read。Windows mixed-version 阻断关闭；Linux/macOS 新
  matching-host 回执仍开放，不能据此声明三平台完成。
- 2026-08-04 v0.1.12 Candidate 首跑（run 30868346027，冻结 SHA `026b294`）
  五平台 build 绿，windows-x86_64 release quality gate 红于
  `fs_copy: prior-agenterm.exe (os error 3)`。根因：gate 先 `build.bat
  release-fast` 产出 `target/release-fast/agenterm.exe`，随后 `build.bat
  release` 的 staged-release 流程以 `cargo clean --target-dir target`
  回收开发目标，upgrade fixture 复制发生在两次构建之后，源目录已被清空。
  修复（`6beacb1`，`scripts/rhai/check.rhai`）：fixture 复制提前到
  release-fast 构建之后、release 构建之前，独立为 `artifact-build-fast`
  gate；`artifact-build` gate 只保留当前产物。rhai repository lint 绿。
  main CI（18f74f7 push）仅 linux-x86_64 遗留已知
  `control_center_linux_native_pointer_navigation_timeout`（X11/XTest CC
  pointer smoke，独立归因，plan-v0.1.13.md 已登记）。重跑 Candidate 与
  Promotion 仍属独立授权门。

## 七、三平台 GUI 与 Control Center 收敛

### 7.1 主工作台

Windows/remote Windows/Unix 的窗口、render、input 与 wake 实现已经物理归属
selected adapters；产品层不再选择 winit/softbuffer/Win32 或 PTY backend。
本节只继续收敛用户可见的跨平台行为与原生证据。

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
- macOS 已有真实 renderer-owned screenshot；Linux production strategy 现已接到
  `RendererRequest`，无效 native handle 保持 typed `Failed`，但仍须在 X11/Wayland
  原生 journey 留存 snapshot + PNG，不能把 strategy 单测冒充交付证据；
- Linux native journey 现已作为 `control-center-linux-smoke` 接入 Xvfb/Openbox CI：
  X11 要求 compositor focus、同 owner snapshot/PNG/digest、server epoch、renderer
  replacement 与 orphan cleanup；Wayland 只声明可移植 owner/reuse，不伪造 focus。
  首次真实 runner 结果未绿前不改变完成状态；
- Native Cockpit input 已由 `agenterm-platform::window` 发布 typed pointer/key event，
  Win32/winit adapter 负责坐标、行命中和键规范化；产品层只维护 tab cursor 并异步
  复用既有 typed select operation。Linux X11 adapter 现从 EWMH client list 选择唯一、
  viewable、PID 精确的 client window，ambiguous fail-closed，并只向目标窗口发送 checked
  key/pointer events；Wayland 明确 Unsupported。macOS Quartz adapter 同样拒绝多窗口歧义，
  以 `CGEventPostToPid` 定向投递并先做不弹窗的 TCC preflight。两套 crate target 的
  warnings-denied Clippy 已通过；Linux smoke 正向验证 active-tab 变化和不抢焦点，macOS
  smoke 严格区分 `NATIVE_INPUT_POSITIVE` 与 typed TCC `permission_denied`。Linux
  positive 已有 matching-host 证据；用户接受 Windows keyboard dogfood，并将 macOS
  physical pointer 明确延后，不冒充 `NATIVE_INPUT_POSITIVE`；
- Windows owning journey 已修复连续键盘/指针输入的真实竞态：server active-tab 先于
  CC worker receipt 到达时，第二个选择不再以 busy 丢弃，而进入单槽 last-input-wins
  队列；smoke 按三行可见窗口推导命中坐标。单元契约与重新构建后的
  `control-center.native-cockpit-input` 黑盒 journey 均通过；
- macOS 通用 renderer smoke 不再把 `scale_factor >= 2` 当作 ARM64 产品不变量；
  runner 的 1x 虚拟显示与 2x Retina 都必须如实报告并保持 frame/PNG 尺寸一致。
  门禁同时校验 PNG 的实际 SHA-256，并在失败码中附带 owner、尺寸、scale、authority、
  title、digest 与 luminance，避免复合断言只能猜测；
- 新 main CI 暴露的三项门禁已定位并进入同一修复增量：`x11rb` 直接依赖补齐
  `MIT OR Apache-2.0` notice；Linux CC runtime 在 bind 前验证 effective UID + `0700`，
  不放宽 `UnsafeEndpoint`；macOS 正向输入按三行 viewport 计算 pointer 行，并把紧随
  keyboard transition 的选择交给已交付的 last-input-wins queue。新六格回执返回前
  仍保持未完成；
- 后续 matching-host run 又关闭三处测试观测错误：Windows process-tree cleanup
  以 PID + start identity 判断同一 descendant 是否仍存活，不再把已退出但仍可枚举的
  对象或 PID reuse 当泄漏；Linux X11 witness 用唯一 title + exact name 验证，不假设
  Xaw 发布 `_NET_WM_PID`；macOS pointer 将 framebuffer 行经真实 Quartz frame/client
  inset 转换，且只选择 viewport 内存活目标。三者均保持原 3 秒/状态等待、typed failure
  和正向行为门槛，不以延长 sleep 或放宽断言求绿；
- exact-SHA `eb45855` 的普通 CI run `30708799815` 已证明 Windows x64/ARM64、
  Linux ARM64、macOS x86_64 与全部 portable quality gate 通过；Windows process
  identity 假阳性已关闭。Linux x64 进一步暴露 winit 的 `active=false` 在 X11
  明确不受支持，现改为 hidden create -> `_NET_WM_USER_TIME=0` -> map，失败时
  typed fail；macOS ARM64 证明键盘正向与 live PTY 后，暴露 targeted click 未必先
  产生 `CursorMoved`，现从当前 `NSEvent.locationInWindow` 取得按下事件自身坐标。
  同时停止把 WindowServer outer frame 冒充 client rect。两项均已通过对应 target
  all-feature compile check，仍等待新的 matching-host 行为回执，不能提前记绿；
- exact-SHA `8d39af2` 的普通 CI run `30716123255` 随后给出两个独立失败：Linux
  x64 的 REPL hard-kill 暴露嵌套 Script command 新建 process group 后可能逃逸外层
  `killpg`，macOS ARM64 则已完成 keyboard active-tab 转换但 pointer 命中超时。
  `72eb861` 以传递后代快照 + start identity 复核清理跨 group 子进程，并用 10 秒
  内层 deadline / 2 秒 cleanup 门槛排除自退假阳性；`7a52d87` 用 WindowServer
  外框宽度与 renderer framebuffer 宽度推导唯一点击 scale，不加 sleep、不尝试备用
  坐标。Linux/macOS target tests 与 warnings-denied Clippy 已编译通过，新的普通 CI
  matching-host 回执返回前两项仍保持开放；
- exact-SHA `af5ac62` 的后继 run `30717141687` 已证明 Linux portable gate（含
  REPL cleanup）通过，因此旧 process-tree 失败关闭；更晚的 Linux CC journey
  发现用于选择的两个 `/bin/echo` PTY 在输入前已正常退出，现改为长驻 `/bin/sh`
  并经 public `send-keys` 产出 marker。macOS ARM64 在统一 scale 后仍无 pointer
  transition，排除“仅 scale”结论；adapter 现把 CoreGraphics 的两个公开
  window-under-pointer fields 固定为唯一 exact-PID WindowServer ID，并保留单坐标、
  no-activate 合同。exact-SHA `bdc7c38` 的 run `30717496128` 已证明 macOS x86_64，
  但 ARM64 仍重复 pointer timeout，说明 window routing fields 也不是完整根因。
  `agenterm-platform` 现公开 selected adapter 实际消费的 typed pointer coordinate
  scale；macOS journey 用 renderer scale 完成 framebuffer -> WindowServer point，
  再用 adapter scale 完成 point -> input unit，不再从 outer/client 宽度猜同一个比例，
  仍保持单坐标、无 retry。首个 exact-SHA `88e5396` run `30718014866` 已通过
  macOS ARM64 build/Clippy/native IPC，但 smoke 在输入前因诊断 map 残留已删除的
  `client_left`/`client_top` 变量名而 typed runtime fail；该机械错误已改为新的 point
  变量，不能把本 run 计作坐标行为结果。后继 exact-SHA `caf3833` run
  `30718160436` 越过诊断错误、再次证明 keyboard selection 与三条 live PTY，
  但 pointer 仍未转换 active tab，因此 authoritative scale 假设被证伪。白箱追踪
  发现 `CGEventPostToPid` 的 windowless NSEvent 以屏幕坐标返回 `locationInWindow`，
  而 shell 曾无条件当作 client-local；adapter 现按 event/target windowNumber 仲裁，
  同窗直接消费、windowless 经目标 NSWindow 转换、foreign window fail-closed，仍不增加
  retry 或 sleep。integrated run `30718584882` 仍在该转换后 pointer timeout，因此该
  假设也关闭，不再继续盲改坐标。renderer screenshot snapshot 现记录 last native input
  kind/button/physical x-y/adapter line（或 key/repeat），macOS timeout 分支只抓一次该
  receipt；exact-SHA `d4dcad3` run `30719117149` 仍保留 `key-pressed/enter`，确定
  mouse event 未进入 target host，而非 hit-test 错。macOS process-window pointer 现返回
  typed Unsupported，不再静默 `Ok`；journey 保留 TCC-authorized keyboard positive，
  并证明 pointer Unsupported 不改变 CC/server/epoch/foreground，不冒充 physical pointer
  positive。Linux 正向结果已关闭；用户明确接受 macOS 真人 pointer 作为后续 follow-up，
  不阻塞 v0.1.12 收口；
- exact-SHA `ed30e82` run `30719411235` 已越过 macOS keyboard positive 与 pointer
  typed-Unsupported 合同，证明该输入分支按预期收口；随后暴露的失败属于下一阶段
  Control Center reuse：调用端已写入 owner-consumed focus mailbox，却又把 macOS 不支持的
  外部 native-handle activation 当成必需成功。现以 live window owner 的 200ms bounded
  event-loop mailbox 作为三平台唯一 focus authority，写入失败显式返回
  `control_center_focus_request_failed`，不再让冗余外部句柄调用否定已交付的 focus 请求。
  同一 run 的 Windows 静态门禁还发现新增 process-security facade 泄漏 Windows handle；
  native handle trait implementation 与 SID test 已重新下沉 Windows adapter，facade 保持
  平台中立；新的 matching-host run 返回前，这两项仍不冒充最终回执；
- `agenterm-platform` 的 process capability 新增独立 `process-reference` feature：Windows
  owned HANDLE、Linux pidfd 与 macOS kqueue `NOTE_EXIT` 都以 RAII reference 保持对象身份，
  避免只凭可复用 PID 观察退出。public facade 只保留 `open/id/is_alive` 与平台中立 handle
  retention trait；BorrowedHandle/AsHandle/AsFd 只在 adapters。169 项 all-feature tests、
  native static boundary 与三个已安装 x86_64 target compile checks 已通过；
- Windows `app-container-profile` feature 现由 adapter 直接创建并拥有三种 well-known
  network capability SID，借用输入先复制到对齐 storage 再进入 Win32，避免外部仓库复制
  SID 常量或依赖未对齐裸指针。API 保留 exact kind/string、显式 profile create/delete 与
  typed HRESULT/Win32 failure；它只是平台 lifecycle primitive，不给 Script Runtime 加权限、
  endpoint allowlist 或自动 sandbox policy。Windows-hosted 5/5 定向测试已通过，exact-SHA
  `c12b3a0` 的后继 ordinary CI 被更新提交正确 supersede，不将取消冒充失败或成功；
- 新增 dependency-free `process-conventions` feature，外部仓库可在任意 host 生成 Windows
  CRT command line 与 `CreateProcessW` Unicode environment block，而不复制 quoting 规则或
  打开进程。审查发现初稿错误保留输入顺序；Microsoft ABI 要求按 name 做 locale-independent
  case-insensitive Unicode sort，现改为 stable folded-key 排序，同 folded name 保持调用方顺序，
  malformed entry 仍显式 Reject/Skip。Windows minimal-feature 5/5 覆盖 typed NUL/index、
  terminator、排序/duplicate/Unicode 和原生 `CommandLineToArgvW` round-trip；该 feature 不拥有
  spawn、duplicate-value policy 或任何 Agent/Script 权限；
- Control Center evidence ownership 不再游离于 alignment：Windows required
  qualification gate 与 Linux/macOS host-native gate 分别登记，`prd-alignment`
  对三者的 evidence ID、脚本发射点和 partial PRD 状态做同一 exact parity。
  这只关闭静态所有权缺口，不把 cross compile 冒充原生 runtime receipt；
- Windows owning journey 已覆盖 incompatible sibling、进程内 renderer capture
  typed failure/recovery、CC process crash/replacement、Human GUI detach 时的
  same-CC/same-server/same-epoch retention、new epoch recovery 和 stale owner
  replacement；Unix 原生组合仍由各自 journey 证明，不能用 Windows 结果代替；
- terminal Paste 不再在 Windows 或 Unix GUI event thread 同步读取 native clipboard：
  单 pending worker 在完成时复核 server epoch、原 tab、terminal/window focus、modal 与
  bracketed mode，stale completion typed cancel。Windows integrated `remote-ui-smoke`
  已以真实 PTY 输入流证明普通异步粘贴和精确 `ESC[200~...ESC[201~` framing；Unix
  all-target compile/runtime 回执仍等待下一次 matching-host CI，不能由 Windows 冒充；
- exact-SHA `ceb41a4` ordinary CI run `30721132723` 给出三个独立、可行动失败，
  不是泛化超时：Windows quality gate 拒绝新增 transient `.ps1` 自动化引用，现复用已
  审计且前一 child 已退出的 run-owned fixture 路径，migration audit 保持 0 drift；
  macOS ARM64 证明单连接 malformed listener 在 renderer refresh 前退出会把 truth 从
  `server_incompatible` 变成 `server_unreachable`，现改为 bounded ready/stop listener；
  Linux x64 重现 winit 丢弃 background core events，现把 no-activate witness 与输入阶段
  分离，显式 foreground 后以 XTest 投递，并要求 renderer `last_native_input` 后恢复 witness。
  三项均等待后继 exact-SHA matching-host 回执，不能把本失败 run 记绿；
- 后继 exact-SHA `9669326` ordinary CI run `30721636280` 六格全绿：Windows x64
  quality gate 关闭 transient PowerShell reference，Windows/Linux ARM64 与 macOS x86_64
  portable cells 通过，Linux x64 原生 CC journey 真实通过 XTest keyboard/pointer receipt
  与 witness restore，macOS ARM64 原生 CC journey 通过持续 incompatible fixture、recovery
  和 owner mailbox。该 run 是这些叶子的 matching-host 完成证据，但不替代仍开放的 Unix
  主工作台 journey、真人视觉/物理输入验收、Candidate qualification 或 Promotion 授权；
- Unix 主工作台的下一叶已进入集成态：`ui.window.activate` 与 `terminal.paste`
  同时具备 CLI operation identity、真实 Script callable、异步 post-state 等待和跨 Windows/
  Unix 的 `terminal.pasted` 事件；Linux/macOS clipboard adapter 保留调用方 deadline 与
  typed Unsupported/Failed cause。一个共享 Rhai journey 由两个 host-native gate 分别拥有，
  覆盖 no-activate witness、native focus、snapshot/PNG、真实 clipboard-to-PTY 及 barrier
  stale cancellation。exact-SHA `78eac9e` run `30723737091` 的 macOS arm64/x86_64
  两格完整通过；Linux x64 到达 native paste 后因在 async snapshot 收敛前直接等待
  `terminal.pasted` 而超时。journey 现先通过公开 snapshot 等到 typed paste success，再读取
  已提交事件；若 clipboard 失败则保留具体 error，而非退化成事件超时。该 run 的 Windows
  quality 同时暴露 LF 下略小于 256KiB 的源文件在 CRLF checkout 后越过默认 Rhai string
  budget；lint task 现显式拥有 bounded 1MiB budget。本轮修复仍等待后继 exact-SHA Linux/
  Windows 回执，因此 capability 与 M11 保持 partial；
- 后继 `d7facf6` run `30724482279` 让 macOS 两格、Windows/Linux ARM64 继续绿，
  Linux x64 也越过 async post-state 并给出精确 `clipboard_backend_error`：Script Runtime
  的 owned process-tree 在 one-shot writer 返回后回收了 xclip background selection owner，
  因而产品 adapter 的真实读取 exit 1。fixture 现以前台 `xclip -silent -loops 2` 作为
  owned child，一次 native read 证明 readiness，第二次由 AgenTerm 消费，随后自然退出并
  进入同一 orphan-free cleanup。该根因修复仍等待下一 exact-SHA Linux 回执；
- exact-SHA `b4f1622` ordinary CI run `30724960474` 随后六格全绿：Linux x64
  原生工作台完整通过 no-activate/activation、renderer snapshot+PNG、真实 X11 clipboard
  到 PTY、stale paste cancellation 与 orphan-free cleanup，关闭 xclip owner 根因；同一 run
  的 Windows named-pipe、Linux/macOS Unix-socket native authority 以及所有适用的 published
  upgrade/rollback journey 全部通过。该回执关闭 Unix 主工作台和 native IPC 的 matching-host
  证据缺口，但不替代 Windows 持续高输出视觉/物理 Shift+Tab、macOS 真人 pointer、Candidate
  qualification 或 Promotion 授权；
- Workflows、Extensions、InfoHub 可以改进解释与导航，但没有 owning backend
  前继续显示真实 empty/unavailable，不造假数据。

## 八、`agenterm-net` N2 受控纵切

本版不把网络愿景继续停留在“研究”字样，但也不把一个能连网的二进制误称为
可公开运行的 IPFS 节点。交付对象是独立 `agenterm-net`：用户显式启动、显式
停止、可检查、可清理；它可以常驻，但安装、打开 GUI 或启动 server 均不会隐式
启动它。`agenterm.exe`、`agenterm-server`、`agenterm-cc` 不链接 libp2p。

```text
N2-M1：可控 full-node foundation
├─ identity / store
│  ├─ ephemeral 与 durable 身份显式二选一；备份/轮换/丢失诊断留痕
│  ├─ 有界 persistent block store：put/get/verify、pin、GC、损坏隔离
│  └─ node state、listener、peer、disk/RSS/连接数均为 typed snapshot
├─ mesh capabilities
│  ├─ Kademlia DHT：bootstrap / provide / find-provider（默认关闭公网 bootstrap）
│  ├─ GossipSub：具名 topic、消息大小/速率/队列上限、receipt
│  └─ relay：client 与受控 relay role 分离；不自动替用户公开 relay
├─ remote Fleet attach（只读）
│  ├─ 用户显式创建 pairing invite，绑定 peer identity、expiry、scope 与 nonce
│  ├─ 远端只投射 bounded Fleet snapshot / event digest；没有 shell、PTY 输入或控制动作
│  ├─ 双端签名/加密与 replay / wrong-peer / expired-invite 拒绝
│  └─ attach 断线、重连、node crash 的结果真实且不影响任一 local server
└─ evidence
   ├─ 两进程、两持久身份、DHT/pubsub/relay/attach 的 deterministic private-mesh fixture
   ├─ 无固定 sleep；超时、取消、kill、corrupt store、budget exhaust 均有 typed receipt
   ├─ Windows/Linux/macOS 的独立启动、停止、残留 listener/child 检查
   └─ package / SBOM / licence / binary and resource delta 先测量再决定稳定资产资格
```

边界：N2-M1 允许私有测试网和用户明确配置的监听地址；不默认连公共 bootstrap，
不自动 NAT 打洞，不承诺 Kubo API 兼容，不开放公网 Fleet control。真正“远程控制”
须另有 Agent/harness 的审批与凭据模型，不能借由网络 attach 绕过。

Durable identity lifecycle 现完成一个可独立验收的纵切：marker 绑定 PeerId，丢 key
typed fail；`identity status|backup|rotate|restore` 公开 receipt、旧 key 无损 marker
migration、轮换中断回滚和错误 backup 拒绝已有 14 unit + 12 CLI 测试。它仍是
experimental：备份托管/加密、multi-device 语义、reconnect/load 以及三平台 fault
injection 尚未完成，不能把这个纵切写成整个 N2-M1 stable。

Local sidecar crash recovery 也现有显式 `node recover` 纵切：活 control 或仅 control
失联但 PID 仍活均拒绝；确认 owner 退出后原子归档 descriptor，按记录 identity mode
启动 replacement，并在 receipt 前完成新 control status round-trip。真实强杀 CLI journey
证明 durable PeerId/store 连续与新 PID，但 remote libp2p peer/session reconnect、自动恢复
policy、rate exhaustion 和三平台负载仍未完成。

ordinary CI run `30725392424` 在 Linux 并行负载下暴露 N1 self-test 的真实阶段预算
不对称：listener 从 listen 前开始消耗同一 10 秒直到 ping，connector 却在 ready 后拿到
完整新预算；libp2p ping 默认 20 秒也越过外层 worker deadline。修复把 ready/ping 分成
各自 bounded phase，使 protocol timeout 严格短于 owner phase，并在 ping failure 时立即
typed fail；parent 先识别 `agenterm-net/error/v1`，不再把合法 child failure 降格成
`missing field event`。跨进程 public self-test、100ms hidden-listener typed deadline、parent
decoder、15 unit + 14 默认并行 CLI tests 与 owning Rhai task 均已通过；仍等待后继 Linux
matching-host 回执，且不因此把 experimental binary 升为 stable/release asset。

后继 exact-SHA `cf420d0` run `30726126583` 证明第一修复有效：listener 不再先耗尽预算、
child error 保留 typed code/message，失败精确移动为 connector fresh phase。随后 `a56144b`
run `30726492135` 在 30 秒预算下证明 connector 已成功 Ping、listener 却仍等待到 30.12 秒，
从而否定“仅仅负载慢”的假设。白箱核对 `libp2p-ping 0.47` 后确认：入站 Ping 会被回答但
不产生 behaviour success event；旧 fixture 错把 listener 再主动发一个 Ping 当作端到端成功的
必要条件。新证据模型由 connector 的 bounded Ping 证明真实往返，同时由 listener 的同一
PeerId connection lifecycle、双方 PID/PeerId 交叉匹配和 clean exit 证明监听侧归属；本地公开
self-test 在 343ms 完成。30 秒仍作为 public receipt 的明确上限而非性能目标；其他命令继续
10 秒，Cargo 300 秒/Rhai 120 秒 outer budget 不变。exact-SHA `1ef8e92` CI run
`30726883698` 随后在同一默认并行拓扑下通过 Linux x86_64 owning research gate，且六个平台
job 全绿；该 matching-host 回执已关闭，不依赖本地快速成功、串行替代或 failed-job retry。

## 九、系统 WebView / Tauri-compatible spike

先以一个独立的 `agenterm-cc-web` 实验宿主验证系统 WebView，而不是把 Tauri
塞进主 GUI；它也不替换既有 native `agenterm-cc`。它是未来独立应用
（Control Center 的可选扩展视图、PluginHub、InfoHub、Workflow 等）可复用的
宿主技术储备，加载仓库内打包的静态 HTML/CSS/JS，首页只读展示 Cockpit。主产品
模型、Fleet authority 和业务逻辑仍在 Rust 侧，Web UI 只是并列投射。

```text
Web host M1
├─ implementation choice
│  ├─ 先做最小 Tauri v2 spike，并记录其 Rust/JS toolchain、lockfile 与 build-time 影响
│  ├─ 同时保留 direct-WRY 作为可比较备选，不预设 Tauri 必然胜出
│  └─ 不把 Node、前端 framework 或网络页面引入 core build path
├─ local-only surface
│  ├─ versioned packaged asset manifest + integrity hash + custom local origin
│  ├─ host.ready / host.facts / fleet.snapshot 三个 bounded typed bridge call
│  ├─ origin / main-frame / nonce / request-id / deadline 严格匹配
│  └─ no eval / shell / process / arbitrary navigation / download / network bridge
├─ platform evidence
│  ├─ Windows: installed WebView2 与 missing-runtime fallback；不 bundling fixed runtime
│  ├─ macOS: WKWebView local asset, Retina screenshot, crash/reload/fallback
│  └─ Linux: WebKitGTK availability/package diagnostic, renderer PNG or explicit unavailable
└─ size and performance decision
   ├─ measure: binary, archive, installer/runtime dependency, cold/warm startup, RSS and first paint
   ├─ compare: native CC baseline vs direct-WRY vs Tauri experiment on each native platform
   ├─ publish machine-readable receipt and threshold decision
   └─ promote only if fallback/isolation/security and six-target packaging all stay truthful
```

The isolated fallback/core crate now owns a tested bridge-v1 admission state
machine with OS-random per-document nonces, exact origin/top-frame binding,
replay/deadline/message/concurrency/memory bounds and only the three read-only
methods above. The direct-WRY host deliberately does not install it yet and
continues to report `bridge=absent`; native adapter wiring, a real public Fleet
projection and three-platform crash/reload evidence remain before any adoption
decision can change from `defer`.

Tauri’s own model validates the system-WebView premise: it uses WebView2 on
Windows, WKWebView on macOS and WebKitGTK on Linux, dynamically linking the
system engine rather than embedding it in the executable. But packaging policy
matters: a Windows fixed WebView2 runtime alone can add about 180 MiB, so this
spike explicitly measures **installed-runtime / fallback** first and does not
bundle a browser. [Tauri process model](https://v2.tauri.app/concept/process-model/),
[Tauri Windows runtime options](https://v2.tauri.app/distribute/windows-installer/).

## 十、并行实施波次

```text
Wave A（共享合同，可并行）
├─ A1：workflow timing + duplicate-work audit
├─ A2：candidate/promotion schema 与 threat model
├─ A3：native IPC mixed-version/stale matrix
└─ A4：Control Center/GUI parity gap matrix

Wave N（网络纵切，与 UI/IPC 实现并行）
├─ N1：N2 node lifecycle、identity/store 和 typed local protocol
├─ N2：DHT/pubsub/relay private-mesh capability fixtures
├─ N3：只读 Remote Fleet attach pairing/snapshot contract
└─ N4：跨平台 resource/fault/isolation evidence 与 package decision

Wave W（Web host，与 network/IPC 实现并行）
├─ W1：Tauri v2 / direct-WRY dependency、toolchain、license 与 baseline measurement
├─ W2：local packaged Cockpit + bounded bridge + native fallback
├─ W3：three-platform runtime/PNG/crash/activation evidence
└─ W4：binary/archive/runtime/startup/RSS receipt 与 adopt/defer decision

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

## 十一、完成定义

- `plan` 中接受的能力均已同步 owning PRD，状态不靠版本号猜测；
- `main` clean、已推送，普通 CI 六目标通过；
- exact-SHA candidate workflow 产出完整 qualification receipt 和六平台资产；
- tag promotion 不执行 Cargo build，不重跑完整 GUI/stress suite；
- promotion 对错 SHA、错 tag、缺 receipt、缺平台包、篡改 hash、过期 artifact
  和不完整 matrix 全部 fail closed；
- Linux/macOS/Windows 的 native IPC 与 Control Center 关键 journey 有各自
  原生证据；
- `agenterm-net` 的 N2-M1 如在本版本宣称完成，必须有私有两节点 DHT/pubsub/
  relay/只读 attach、持久身份/store、资源与故障隔离的跨平台证据；未达此门则保持
  experimental，不能进入 stable asset 或远程控制面；
- system-WebView spike 必须给出 native CC、direct-WRY 与 Tauri 的可复核体积及
  启动/RSS 对比、三平台 runtime/fallback 证据和 bridge isolation 测试；否则只保留
  renderer-neutral 合同，不把 Web UI 宣称为 production renderer；
- 没有新增窗口风暴、固定 sleep、测试残留 server/socket/pipe 或隐式
  foreground activation；
- 付费 runner 即使试验失败，也可通过一处 label/config 回退，不影响免费
  github-hosted 正确路径；
- 未经用户最后明确批准，不创建 `v0.1.12` tag 或 GitHub Release。
