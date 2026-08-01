# AgenTerm v0.1.12 公开计划

状态：产品建设期（2026-08-01 重基线）；远未进入 RC，Candidate/Promotion
仅是 Wave D 的远期交付门，不是当前实施主线，也不构成本阶段进展定义。
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
  组成完整 client frame，再以单次 `BitBlt` 提交，分配或提交失败时保留直接绘制
  fallback，并对像素预算 fail closed。聚焦双缓冲边界测试与 UI-client tests 已通过。
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
  该结论排除“颜色填充本身 4x”，但不替代 paint/invalidate 时间域观察。
  该证据只能证明源头被切断和帧提交结构，不能证明时间域视觉效果；新构建的
  高输出/idle 60fps 真机观察和 paint/invalidate telemetry 仍是关闭条件。
- [ ] alternate-screen harness 无法本地向上滚动；初步证据指向 `vt100`
  alternate grid 的零 scrollback，需要在 application raw-mouse ownership 之外
  评估把 wheel/PageUp 语义转交前台 TUI，不能破坏普通 scrollback。
- [~] terminal focus 下 `Shift+Tab` 修复正在集成：共享 named-key encoder 已按
  xterm modifier 参数覆盖 Tab/方向/Home/End/Insert/Delete/Page/F1–F12；Unix
  两条输入路径保留 modifiers，Windows `WM_KEYDOWN` 显式处理 Tab/Insert/Delete
  并屏蔽 Tab/Escape 的 `WM_CHAR` 重复回声。commands 与 Windows mapping 聚焦
  单测、395 项 Quick library tests、all-target Clippy、alignment partial
  fail-closed 集成测试与七产物 dev build 通过。owning Windows journey 现会在
  terminal focus 下投递 Shift+Tab window shortcut，并从公开 pane counter 断言 PTY
  恰好多收到 3 bytes；编码契约固定内容为 `ESC [ Z`。真实物理键盘 dogfood 仍待
  验证，不能仅凭自动化投递宣称交互已收口。Windows host 的 Linux `cargo check` 仅因缺少
  `x86_64-linux-gnu-gcc` 停在 `ring` 构建脚本，Unix adapter 仍需原生 CI/host
  证据。后续 macOS control-key 修复曾把所有 named key 提前于 committed text，导致
  无修饰 Space 从 `TextCommit(" ")` 回归成 `ControlKey("Space")`；精确 HEAD 的
  `--skip-smoke` 捕获该失败。共享契约现只让可打印 committed text 优先，Enter/Backspace/
  Escape 的 native control-character echo 仍保持 named control。crate/root 两级聚焦表驱动
  测试固定 Space、Enter 与既有 Shift/Unicode 语义。
- [~] Windows toolbar `z/Z` 字号按钮造成 terminal 看似“无响应”的主因已修：native
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
  server fault recovery 与最终 cleanup。剩余限制是时间域闪烁仍需真实高输出/idle 肉眼或
  capture 证据，不能由该状态收敛测试替代。
- [x] 默认 `Keep Server Running` 不再因调用者 Job cleanup 杀死独立 server/PTYS；
  GUI 与 CLI 统一走 platform process facade，完整 replaceable-UI 黑盒已证明退出、
  detached lease、同 server/session 接回与最终显式 Stop Server。live dogfood 又发现
  从不允许 breakaway 的上层 Windows Job 内启动时，`CREATE_BREAKAWAY_FROM_JOB` 会直接以
  error 5 拒绝创建 server。platform process facade 现在只对该精确错误重试 caller-job
  fallback，并以 `DetachedSpawnMode` 和 parent-console diagnostic 明示降级；其他 spawn
  failure 不会被吞掉。40 项 crate tests、两级 warnings-denied Clippy、七产物 build 和
  isolated native GUI/server/PTY 启停 probe 通过；fallback server 可能随上层 owning Job
  结束，不能被文档冒充为完全 independent。
- [ ] terminal 鼠标选区无法可靠建立，导致已实现 copy/paste 无法使用；复查
  selection ownership、drag threshold/capture、raw-mouse arbitration 和复制黑盒。
  白箱审计定位当前选区绑定整个 `screen.generation`：持续 output delta 在 100ms
  reconcile 时清空 drag/completed selection，paint/copy 也因 generation 不等而
  拒绝；drag 中取消还可能漏掉 `ReleaseCapture`。修复需区分 same-grid 内容推进
  与真实尺寸/tab 失效，缓存完成态复制文本，并在 drag 中注入输出验证 phase、
  Ctrl+C/system-menu Copy 和 capture release。现有“先等输出静止再同步拖拽”的
  smoke 不足以证明该行为。generation/capture/cached-text 修复已经存在，但 owning
  smoke 现已补成 pointer down 后经 public CLI 注入唯一 PTY delta、等待 GUI reconcile，
  再完成拖拽并从 system menu Copy；旧 generation 绑定会在该路径确定失败。另修正
  Ctrl+C arbitration：只有 non-empty completed cached selection 才接管 Copy，prepared/
  empty state 不再吞掉 PTY interrupt。13 项 frontend tests 通过；真实 Windows journey
  以及拖出 viewport auto-scroll 仍未闭环，因此保持未完成。
- [~] 本地开发缓存膨胀的第一段已闭环：实测 `target/` 15.2 GiB，其中
  `target/debug/incremental` 10.53 GiB；一次显式 `cargo clean` 已回收全部可再生缓存。
  dev `build` 现在只在七产物成功 staging 后调用 `prune-target-incremental`，持有真实
  `debug/.cargo-lock` 并逐项取得 rustc session lock，按 compilation-unit root 保留最新
  finalized session、删除可证明失效且超过 60 秒的旧 session；缺锁、锁占用、working、
  reparse/symlink 或变化中的目录均 fail closed。3 项隔离测试覆盖 newest retention、
  Cargo/rustc lock contention 与释放后重试。此前审计估算该层可回收约 4.30 GiB；仍约
  6.23 GiB 来自不同 root generation，必须等待精确 touched-unit manifest，不能用名字或
  mtime 猜测删除。
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
│  ├─ [x] agenterm-script 已交付真正的持久 REPL
│  │  ├─ ReplSession 会话内核与 CLI 输入适配解耦，可供 CC/Agent 复用
│  │  ├─ 变量、函数、多行单元、错误恢复、reset 和内存 history
│  │  ├─ TTY 提示与 pipe/NDJSON 自动化输出分离
│  │  ├─ 单元失败不提交语言状态，外部真实副作用不伪装回滚
│  │  ├─ 普通 worker 与 REPL 复用同一 Engine/API 配置
│  │  └─ [~] Ctrl+C、箭头历史与 kill/restart 长驻协议继续 hardening
│  ├─ 评估把 canonical Rhai 入口从 agenterm-script 重命名为 agenterm-rhai
│  │  ├─ 名字直接表达语言/runtime 身份，不再暗示抽象的通用脚本沙箱
│  │  ├─ 先冻结 CLI、task、worker、包名、文档和第三方调用者影响清单
│  │  ├─ 若本轮实施，agenterm-script 作为有期限的兼容转发入口，不复制 runtime
│  │  └─ 构建、测试、Candidate、Promotion 必须在 canonical 名切换后保持自举
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
- macOS/Linux 补齐真实 renderer-owned screenshot，而不是替代图或
  “capability available”假状态；
- 覆盖 incompatible sibling、renderer crash、server retained while GUI
  replaced、new epoch recovery 和 stale owner replacement；
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
