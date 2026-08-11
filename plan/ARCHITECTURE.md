# AgenTerm architecture map（现行结构 SSOT）

状态：active（2026-08-05；对齐机制/工具边界见 §8）  
权威范围：**代码分层、入口、所有权、禁令、结构如何被勾住**。  
非权威：发版资格、能力 shipped 状态（见 `prd/`）、波次任务列表（见 `plan/plan-v0.1.*.md`）、封装/复用改进建议的执行排期（版本 plan 记叶，不在本文重画）。

> **抗漂规则**：全仓库只维护 **这一份** 现行结构图。其它 `plan/*` 只链到本文，禁止再画第二棵「现行」树。  
> 结构变更与本文冲突时：同批改本文，或改代码；禁止第三现实。  
> 自动闸（**局部**，非全文双向）：`src/platform/boundary_tests.rs`。  
> 历史过程文 `plan/archive/platform-ui-ux-boundary-tree.md` = **superseded**，不得当现行权威。

---

## 1. 分层（验收尺）

### 1.0 三层边界（跨平台封装 SSOT）

| 层 | 路径 | 装什么 | 不装什么 |
|----|------|--------|----------|
| **机制** | `crates/agenterm-platform` | 窗/键鼠/IME/激活/剪贴板/截图/字体/IPC/PTY/进程/FS/shm… typed Available/Unsupported/Failed；**无** AgenTerm 产品名 | 工作台剧本、Fleet、ui-action 表、instance/server strip 产品策略 |
| **产品语义** | `src/frontend/*`、`src/ui_*.rs`、`src/ui_geometry.rs` | 手势含义、dialog 状态、geometry、action id、snapshot 字段 | 直接 `windows_sys` / winit / x11（boundary 闸禁止） |
| **Host present** | `src/platform/adapters/{windows,unix}/**` | 怎么画、收事件、接线 IPC、原生控件映射 | 新产品策略仅单端落地且不登记 catalog/`parity-gap` |

- **跨平台封装** = OS 差异停在 platform crate（agenterm + wbox 等 embedding 调用）。  
- **三端工作台手感齐** = 产品语义单点 + 两端 adapter 接线；**不**把 AgenTerm 工作台塞进 platform。  
- 产品 `ui-action` interim 集合闸：[`src/frontend/ui_action_catalog.rs`](../src/frontend/ui_action_catalog.rs)。  
- 机制漏点表：[`plan/plan-platform-encapsulation-gap.md`](plan-platform-encapsulation-gap.md)。  
- 可执行 goal：[`plan/goal-crate-platform.md`](goal-crate-platform.md)。  
- **Rhai ↔ Rust Facade 边界**（L3 pack / L2 catalog / L1 kernel）：[`plan/design-rhai-rust-boundary.md`](design-rhai-rust-boundary.md)。

### 1.1 目录树

```text
crates/agenterm-platform     机制：窗口/输入/截图/进程/IPC/PTY/字体/shm…
                             typed Unsupported / Failed；无 AgenTerm 产品名

src/platform/                产品平台 glue：FrontendHost、目录名、快捷键/CC、能力/IPC 命名
  policy/                    host 无关产品策略表
    input.rs                 shortcut / empty-copy 输入策略（Win/Unix 共用）
    control_center.rs         CC screenshot 策略（Win/Unix 共用）
    paths.rs                 product path naming / workspace / IPC workspace
    workspace.rs             workspace directory layout policy
    host.rs                   host predicates / shell command routing
    capability.rs             product capability status / platform_info JSON
    ipc.rs                    native IPC endpoint naming policy
    script_http.rs            Script Runtime HTTP TLS provider/root policy
    runtime.rs               hosted worker / test host / new-terminal shell argv 默认
    test_fixtures.rs         long-running process fixtures
                             策略表、services facade（应薄，勿第三套 OS adapter）

src/frontend/                产品 GUI 入口 + UI/UX 语义
  mod.rs                     parse / handoff / 统一结果码 / dispatch
  action.rs                  canonical action identities（toolbar/shortcut 共用）
  ui_action_catalog.rs       ui-action SHARED/host-only 集合闸（interim）
  toolbar.rs                 toolbar action 映射（Win/Unix 共用）
  window.rs                  client-size / window semantic state（Win/Unix 共用）
  interaction.rs             focus navigation / wheel accumulation / wheel routing / scrollbar thumb drag / modal/focus state + modal surface priority/snapshot naming + FocusSurface canonical names/IPC aliases（FocusState + adapter focus_gate() + ModalSurface/modal_surface_from_gate() + FocusSurface::as_str()/from_ipc()，Win/Unix 共用）；raw-mouse arbitration/report outcome 策略与 xterm mouse report 编码器（Unix embedded 与 Windows remote 共用）；alternate-screen wheel fallback 用 commands::alternate_screen_wheel_bytes 单点编码
  composer.rs                ComposerWriteMode（empty-only/append/replace）单点定义，embedded、remote UI、server dispatch 共用
  cwd_editor.rs             CWD editor modal 状态/action/snapshot 单点；Unix embedded 与 Windows remote 共用 CwdEditorDialog，adapter 只保留原生编辑控件/焦点与命令执行
  input.rs                  keyboard/composer/tab-editor/terminal-shortcut 输入语义单点；Unix embedded adapter 经 `frontend::input` 引用，Windows remote 保留原生控件映射
  new_terminal.rs           new-terminal modal 状态/校验/action 单点；Unix embedded 使用共享 dialog，Windows remote 仍用原生控件呈现，状态/校验/action/argv 与 Unix 共用共享 dialog
  settings.rs              settings modal 状态/校验/action 单点；Unix embedded 与 Windows remote 共用 SettingsDialog，adapter 只负责原生呈现/事件映射
  close_confirmation.rs    live-tab close confirmation 状态/快照单点；Unix embedded 与 Windows remote 共用 CloseConfirmation，adapter 只保留原生确认控件与关闭执行
  tab_editor.rs            inline tab editor 状态/校验/快照单点；Unix embedded 与 Windows remote 共用 TabEditorDialog，adapter 只保留原生编辑控件/IME/事件映射
  window_close.rs          window-close 状态/choice/snapshot 单点；Unix embedded 与 Windows remote 共用 WindowCloseDialog/WindowCloseChoice，adapter 只保留原生窗口执行与按钮呈现
  selection.rs               线性选区 / autoscroll / word-boundary 语义（SelectionGesturePhase + 泛型 SelectionGestureState<TabId, Point> 单份定义；TerminalCellSource + word_selection_bounds 让 vt100 与 snapshot cell grid 共用；Unix embedded 与 Windows remote 共用状态机、autoscroll_step）
  control_center.rs         Control Center 产品 facade（native 能力仍走 platform services）

src/frontend_server.rs       server 拉起 / 恢复（非 IPC 代理）

src/ui_*.rs + control_*      共享产品语义：geometry / snapshot / bridge /
                             clipboard / dispatch（terminal selection 语义已归 src/frontend/selection.rs）

src/platform/adapters/       主机实现（物理目录）
  windows/                   replaceable remote UI ↔ agenterm server
  unix/frontend/             embedded 窗口 + 产品状态机
  linux|macos/               契约/manifest 等（非第二套业务策略）
```

**妥当**：分叉停在「主机如何画 / 如何收事件」。  
**不妥当**：分叉停在「点了 Tab 算不算选中」——产品规则只应有一份。

---

## 2. 可执行入口（bins）

| 二进制 | 路径 | 角色 |
|--------|------|------|
| `agenterm` | `src/bin/agenterm.rs` | GUI 启动器；`server` = 无窗权威；`cli` = 共享控制平面入口 |
| `agenterm-com` | `src/bin/agenterm-com.rs` | 极简 Windows Console-subsystem 转发器；交付名 `agenterm.com`，同步等待 `agenterm.exe` |
| `agenterm-cc` | `src/bin/agenterm-cc.rs` | Control Center 投影 |
| `agenterm-con` | `crates/agenterm-con/Cargo.toml` + `src/bin/agenterm-con.rs` + `src/bin/agenterm-con/*` | 独立最小依赖 package；conhost 等价物（单 GUI 进程内多 PTY 树，无 server/Fleet/script；平台 pixel-window 直调；局部纯 UI 规则与适配状态机分离） |

`agenterm-con` 的窗口机制仍只能从 `agenterm-platform` 选择。Windows 已有
`native-pixel-window` host：直接使用 User32 消息泵、GDI XRGB buffer 与
`PixelWindowApplication` 中立合同，不把 HWND 或产品策略泄漏回 con。Linux/macOS
继续由 winit/softbuffer adapter 实现同一合同；未来原生 X11/Wayland/Cocoa host 也应
替换 adapter，而不是分叉产品状态机。Windows con 已默认选择 native host；主程序仍
选择 portable host。Native host 已实现 IMM32 preedit/commit、candidate client-anchor、
pointer capture/loss 和 DPI suggested-rect；中文输入法仍需真机人工验收，不能由合成
WM_CHAR 或文本快照替代。

Windows native pixel host 与主程序 control-window host 共享 platform-internal 的
有界重入队列机制，但保留各自的消息快照和产品事件策略。每个 HWND 拥有稳定
userdata 与独立队列；同步 User32/IMM FFI 触发的嵌套回调不得从裸指针重建第二个
`&mut State`，pointer-backed 参数只能在原回调内消费或复制，overflow、借用冲突和
非收敛 drain 都 typed-fail closed。该共享边界是原生机制复用，不把 con 或工作台
产品策略下沉到 platform。

**2026-08-09：** `agenterm-rh` / `agenterm-lua` / `agenterm-qjs` / `agenterm-sql`
四个独立 `[[bin]]` 已退役（commit `234b2f87`），改为主 `agenterm` PE 的
argv 透传子命令：`agenterm rh|lua|qjs|sql <args>`（rh 实现仍在
`crates/agenterm-rh`，qjs/lua/sql 同理各自 crate；只是不再各自产出独立
release 可执行文件）。

**构建自举：** `build.bat` / `build.sh` 仅定位或首次构建主 `agenterm`，
再以 `agenterm rh task run ...` 进入 `scripts/rh/` 的唯一构建政策。最近一次
通过 `agenterm rh version` 自检的主程序保存在 Cargo output 之外；源码身份
变化会尝试 seed 当前主程序，seed 失败则回退而不覆盖旧 LKG。clean clone 与
无缓存 CI 在 stage-0 执行 `cargo build --bin agenterm`，不恢复独立
`agenterm-rh` bin。

**rh 切换：** 宿主经 [`src/script_backend.rs`](../src/script_backend.rs) 选择 backend；详见 [`plan/design-rh-aot.md`](design-rh-aot.md)。

Authority entry plan: [`plan/archive/plan-agenterm-server-mode.md`](archive/plan-agenterm-server-mode.md)。

Cargo 版本号见根 `Cargo.toml`（与公开 tag 可能暂时脱节——发版以 Candidate/Release 链为准）。

---

## 3. 热文件（改前先认主）

| 区域 | 路径 | 备注 |
|------|------|------|
| GUI ingress | `src/frontend/`, `src/frontend_server.rs` | 参数/唤醒/结果码 |
| 共享 UX | `src/ui_geometry.rs`, `src/ui_snapshot.rs`, `src/ui_bridge.rs`, `src/control_dispatch.rs` | 对齐契约 |
| 产品策略表 | `src/platform/mod.rs` + `policy/` | policy 已拆；facade/`allow(dead_code)` 半迁移见 L3 |
| Win 主机 | `src/platform/adapters/windows/{frontend,remote_frontend}.rs` | remote 客户端；`remote_frontend` 巨石见 L2 |
| Unix 主机 | `src/platform/adapters/unix/frontend/` | embedded 状态机；`mod`/`render` 巨石见 L2 |
| 机制 crate | `crates/agenterm-platform/src/{selected,window,input,ipc,pty,process,shared_memory}.rs` | 无产品名 |
| 边界闸 | `src/platform/boundary_tests.rs` | 规则见 §8.2；**不**解析本文全文 |

---

## 4. 已知结构债务（勿当「已修好」）

摘自 `plan/archive/plan-v0.1.13.md` 审查；**修债务时更新本节与对应叶**。

| ID | 现状 | 目标 |
|----|------|------|
| L1 | ~~`frontend.rs` `#[path]` 虚树~~ | **已收**：`platform::adapters::{windows,unix}` 正规 mod；`frontend` 只 `use` |
| L1b | ~~`windows/frontend` 靠 sibling `#[path]`~~ | **已收**：同目录 `windows::{frontend,remote_frontend}` |
| L2 | Win remote vs Unix embedded 双主机（selection/focus/wheel/scrollbar-drag 已共享；`ui-action` 大 match 与巨石 adapter 仍双写；**interim set-diff gate**: `src/frontend/ui_action_catalog.rs`） | 共享交互语义单点；主机只 present/wake/IME；action 表驱动记版本 plan 讨论叶 |
| L3 | `platform/mod.rs` 策略过肥（input/paths/control_center/runtime/test_fixtures/workspace 已拆 `policy/`；FrontendHost 与 facade 是剩余薄层）+ `allow(dead_code)` | `policy/*` 全拆收口；禁新顶层 `is_windows_host` 蔓延；半迁移 facade 二选一（全接线或删） |
| L4 | **结构 SSOT 未机读双向**（本文 prose + 局部 `boundary_tests`；目录树/分层文案漂移靠人） | 见 §8.4；版本 plan **S 组**执行；本文只定契约 |
| D1 | shared_memory 名长 ≤31 | **本机已绿**：unit + `shared_memory_process` 名式 `apm-…` ≤31 |

已清理：`src/platform/services/frontend.rs` 孤儿 re-export（无人 `mod`）——删除；入口以 `src/frontend/` 为准。

---

## 5. 文档谁说了算

| 问题 | 看哪里 |
|------|--------|
| 代码现在怎么分层？ | **本文** |
| 结构如何被自动勾住 / 工具边界？ | **本文 §8** |
| 本版要修哪些叶？ | 当前版本 `plan/plan-v0.1.*.md`（结构机读化 → **S 组**） |
| 能力是否 shipped / 验收？ | owning `prd/PRD_*.md` + `prd/alignment-contract.json` + `scripts/rh/prd-alignment.rh`（**能力**对齐，**不是**结构树） |
| Win↔Unix 可见行为差距？ | `plan/plan-unix-gui-win-parity.md` + evidence matrix（**差距地图，不是结构 SSOT**） |
| Agent 操作纪律？ | `AGENTS.md` |
| 产品总树？ | `PRD.md` |
| 旧 boundary-tree 叙事？ | `plan/archive/platform-ui-ux-boundary-tree.md`（**superseded**） |

历史过程文若与本文冲突：**以本文 + 代码 + boundary_tests 为准**。

---

## 6. Agent 禁令（短）

1. 不要在 adapter 里新写产品策略 `if windows` / `if unix`；策略进共享管线或 `platform` 表。  
2. 不要静默把 `Failed`/`Unsupported` 改成 temp 路径或「假装可用」。  
3. 不要在 `agenterm-platform` 引入 `agenterm::` / `AGENTERM_` 产品耦合（已有测）。  
4. 不要新增第二套 GUI 启动解析或第二套 server autostart 决策。  
5. 不要把 net / WebView / 大 Control Center 内容写进「已 shipped」除非 owning PRD 已改。  
6. 结构变更：更新本文；版本 plan 只记叶与证据，不重画全树。  
7. 新 `ui-action` / 产品手势：**shared-first**（`src/frontend/*` + `ui_action_catalog.rs`）；单端落地须进 `WINDOWS_ONLY_*` / `UNIX_ONLY_*` 并写 `parity-gap:`，禁止默认同端双写后甩给另一平台 agent。  
8. 跨平台任务固定句式（机制 / 产品 / host present 判定 → 改对应层 → 证据）：见 [`plan-platform-encapsulation-gap.md`](plan-platform-encapsulation-gap.md) § Agent 执行句式。

### 6.2 `agenterm` / `agenterm-con` 协同边界

- UI/UX 可分化：主程序是 server/script/Fleet 工作台；`agenterm-con` 是随 GUI
  生命周期结束的轻量多终端，两者不共享产品导航、持久化或 authority policy。
- 底层机制应汇合：PTY 生命周期、VT/宽字符、字体与渲染缓存、选择/剪贴板、
  IME/focus、鼠标/滚轮、DPI geometry、背压/调度及黑盒观测接口优先形成纯函数或
  typed platform/frontend contract；host adapter 只 present/wake/接 OS 事件。
- Win32 host 重入机制也必须汇合：共享容量、FIFO 和借用失败合同；各 host 只保留
  typed message snapshot、default-processing 和生命周期政策，禁止线程全局裸
  `(WPARAM, LPARAM)` 队列跨 HWND 复用。
- PTY 输出传输由 `agenterm-platform::pty::BoundedOutputPipe` 提供跨平台固定容量
  字节环：一次原生 read 要么整体提交、要么等待容量，关闭会唤醒生产者且已提交
  字节仍可排空。消费者按字节预算直接读取环内连续切片，不为每次 read 分配
  `Vec`；产品层只决定容量、每轮预算、解析和重调度策略。
- Windows PTY 由 platform adapter 直接拥有 ConPTY、同步 output、可取消 overlapped
  input、`STARTUPINFOEXW`、进程等待和 `KILL_ON_JOB_CLOSE` Job Object；子进程以
  `CREATE_SUSPENDED` 创建，加入 Job 后才恢复，任一部分失败都终止未受保护的进程。
  旧版 Windows 的 `ClosePseudoConsole` 可能等待最终输出，因此独立 output pump 会先
  切换为 drain/discard，再关闭唯一 HPCON；resize 在同一 HPCON lock 内完成，不得与
  close 使用已释放句柄。PowerShell DSR 分片、Windows build-gated passthrough fallback、
  PATH/PATHEXT、环境块、cwd、命令行 quoting 和 breakaway retry 都属于 adapter 合同。
  控制台附着、分离、进程级串行化和按键 `INPUT_RECORD` 仍由同一 `ConsoleGuard`
  持有，通过 `WriteConsoleInputW` 精确提交 press/release 对。真实 `cmd.exe`、CJK、
  alternate-screen `less`、输入、缩放、截图和异常进程的 18 项黑盒及 1 项多标签控制门
  通过；Windows 正常生产图不再含 `rmux-pty` / `rmux-types` / `tracing`。同一
  unwind/trace-only release-fast PE 从 791,552 B 降至 761,856 B，净减 29,696 B。
- Platform feature 边界按机制而非历史聚合划分：`pty` 和 `clipboard` 不再隐式启用
  完整 `process`；GUI launcher 的父控制台输出与目标 shell/locale 默认值分别由
  `parent-console`、`runtime` 拥有。完整 `process` 仅为兼容聚合窄机制，con 必须显式
  声明所需 feature，不得因此获得进程枚举、控制、指标、安全或 spawn 面。
- 同一规则适用于正交文件机制：`screenshot`、`font` 不携带完整 `filesystem`，`ipc`
  不携带未使用的 `locking`。截图落盘由产品显式选择 `filesystem-publish`；IPC adapter
  只拥有 endpoint、transport 与调用者 identity，不能借 feature 依赖隐式扩大文件面。
- con 的公开自动化面是 `agenterm-con cli`，不是其进程内 wire。CLI 与 JSON 输出保持
  稳定；GUI-lifetime client/server 之间使用 `ATC1` 长度前缀 typed frame，只编码命令
  实际字段并拒绝未知 opcode、非法 tag/范围/UTF-8、超限长度和尾随字节。该层不得演化
  为 mux/server 协议，也不得重新引入通用 DOM envelope。同一 unwind/trace-only
  release-fast PE 从 760,832 B 降至 737,280 B（-23,552 B）：`.text -17,664 B`、
  `.rdata -4,808 B`、`.pdata -1,572 B`，`.rsrc` 不变；脚本 decode 同时用显式循环
  取代异常膨胀的泛型 `collect<Result<...>>`。`cargo-bloat` 等会改变 rustc flags 的分析
  构建必须使用隔离 `--target-dir`；与 build-std 官方图共用 target 会留下不匹配的
  core/compiler_builtins fingerprint，profile clean 不能可靠回收，禁止再次污染交付图。
  后续隔离 bloat 证明 size profile 仍把 config、参数、脚本和 CLI codec 过度内联进
  `main` / offline 入口；这些既有可测边界显式禁止内联，并以固定 8-byte 数组装配
  `ATC1` header，避免通用可变尾切片。官方同 profile PE 从 737,280 B 降至
  733,184 B（-4,096 B），其中 `.text -2,896 B`、`.rdata -536 B`，`.rsrc` 不变。
  隔离 bloat 随后定位到控制线程为请求队列和每请求回复分别单态化两套通用
  `std::sync::mpsc`。con 现以互斥保护的 FIFO 和一次性 Condvar 回复槽表达实际协议：
  每请求线程仍可并发，`wait-text` 不会阻塞后续客户端；队列关闭在同一临界区原子
  拒绝新请求并释放待处理回复，发送端丢弃会立即唤醒等待者。隔离 PE 从 716 KiB
  降至 698 KiB、`.text` 从 473.5 KiB 降至 459.0 KiB；官方 release-fast PE 从
  733,184 B 降至 714,752 B（-18,432 B）。普通 profile 与 build-std con profile
  也不得共用默认 target：即使包/profile 名相同，不同 rustc flags 仍会污染
  core/compiler_builtins fingerprint；诊断构建同样必须使用隔离 `--target-dir`。
  生产线程入口统一经过无 feature 的 `agenterm-platform::threading::spawn_named`：
  产品和 adapter 先将任务收敛为 `Box<dyn FnOnce() + Send>`，platform 内一个禁止
  内联的 trampoline 才调用 `std::thread::Builder`。线程名、spawn 错误和 Rust
  unwind/JoinHandle containment 不变，con reader/waiter、控制 listener/request、
  ConPTY output pump 以及通用 child reaper 不再按闭包类型重复生成 std 线程启动和
  `catch_unwind` 胶水。隔离 PE 从 698 KiB 降至 682.5 KiB、`.text` 从 459.0 KiB
  降至 447.5 KiB；官方 release-fast PE 从 714,752 B 降至 698,880 B
  （-15,872 B）。该边界是跨平台机制复用，不包含终端、进程或产品调度策略。
  Windows adapter 随后将这个真实 detached 语义下沉到 raw system FFI：一个
  `CreateThread` 入口接收 boxed 上下文，成功后立即 `CloseHandle`，线程内先用
  `SetThreadDescription` 发布 OS 可见名称，再以显式 `catch_unwind` 执行任务；
  创建失败在调用线程回收上下文，panic 绝不越过 `extern "system"` ABI。Linux/
  macOS 保持同一 `spawn_named_detached` contract 上的 std adapter，直到 pthread
  方案具备同等可移植性证据。Windows 单测从系统读取线程描述并证明 panic 析构，
  con 的真实 PTY/control/child 路径继续通过。隔离 PE 从 682.5 KiB 降至
  672.0 KiB、`.text` 从 447.5 KiB 降至 441.5 KiB；官方 release-fast PE 从
  698,880 B 降至 688,128 B（-10,752 B），未增加 crate 或 platform feature。
  child waiter 的完成通知不再为单个 `()` 实例化最后一套生产 `mpsc`。每个 session
  持有一个共享 `AtomicBool`：waiter 在写入退出状态后以 Release 发布并沿用既有
  window wake，GUI 以 AcqRel `swap(false)` 恰好消费一次；原子位只拥有状态，wake
  仍拥有调度，因此 ConPTY child 退出与 output pipe EOF 的独立语义不变。隔离 PE
  从 672 KiB 降至 652 KiB、`.text` 从 441.5 KiB 降至 429.0 KiB；官方
  release-fast PE 从 688,128 B 降至 667,648 B（-20,480 B）。正常/失败/快速命令
  退出和多标签控制仍由 90 unit、18 black-box 与 1 control journey 覆盖。
- `agenterm-con` 的 session ownership 不再使用通用 `BTreeMap`。树顺序、父子关系和
  stable `TabId` 的权威仍完全属于 `Workspace`；产品专用 `SessionStore` 只以小型
  `Vec<(TabId, ConTerminal)>` 做线性路由，并在关闭时用不可观察顺序的 swap remove，
  因而不会把节点分配、平衡和有序删除代码链接进迷你客户端。隔离 PE 从 652.0 KiB
  降至 638.5 KiB、`.text` 从 429.0 KiB 降至 419.0 KiB；官方 release-fast PE 从
  667,648 B 降至 653,824 B（-13,824 B）。多标签行为继续由同一 90 unit、18
  black-box 与 1 control journey 覆盖。这个结果也固化尺寸工作的选择准则：先删除
  不需要的通用机制，只有反汇编证据指向的窄叶子才进入汇编或原生 FFI。
- 像素热循环由 `agenterm-ui-core::pixel` 持有标量真值与 ISA dispatch；产品不得复制
  CPU 探测。Windows 截图不再打包 XRGB 或自行计算 PNG checksum：platform adapter
  将已校验 clip 的首像素指针和原 framebuffer stride 直接交给 GDI+
  `GdipCreateBitmapFromScan0` / `GdipSaveImageToFile`。Linux/macOS 继续由 portable adapter
  生成 RGBA 并调用 Rust PNG encoder。`agenterm-platform::checksum` 的 IEEE CRC-32 与
  Adler-32 仍是通用校验合同，但不再进入 Windows con 截图生产图；不得误用 x86
  SSE4.2/Arm CRC32C 冒充 PNG polynomial。无测量证据时不得为 `slice::fill` 等成熟原语
  维护 ISA 分叉。
- 原子文件发布由 `agenterm-platform::filesystem_publish` 的 `write_file_atomic` 与
  `write_path_atomic` 统一持有：前者供 Rust writer，后者供只能接收路径的系统 codec；
  两者均使用同目录独占临时文件、文件 sync、原生替换、父目录 durability barrier 和
  失败清理，path callback 返回后还会重验 regular/non-link entry。Windows adapter 使用
  `MoveFileExW(REPLACE_EXISTING | WRITE_THROUGH)`
  并有界重试共享冲突；Linux/macOS 使用同目录 `rename` 后 fsync 父目录。错误必须
  区分未发布与“完整文件已发布但 durability 未确认”，产品不得复制 `.tmp` 规则。
- 提升顺序固定为：先在 owning 产品以单测和公开黑盒证据证明规则，再抽取无产品
  authority 的最小契约，再让两个产品消费；不得为了“复用”把 server、脚本、Fleet
  或 con 的 GUI-lifetime local-control 策略下沉到 platform。
- `src/bin/agenterm-con/ui.rs` 当前是局部孵化层，只容纳无窗口后端/PTY 依赖的
  geometry、命中与视口规则；规则被主程序实际需要且证据稳定后，迁入
  `src/frontend/*` 或 `src/ui_geometry.rs`，不长期复制双份实现。
- 体积与构建隔离是两个问题：Windows 原生 pixel host、独立
  `crates/agenterm-con` package、platform-owned native PNG/font adapter 及
  native font rasterizer 及 bounded schema-specific JSON codec 已把 release PE 从
  1,046,528 B 降到 585,216 B；当前比 512 KiB x86_64 目标高 60,928 B，主要增量是
  后续加入的可靠 PTY 固定环、同步语义和通用原子文件发布状态机，不以回退关闭、
  背压或覆盖/durability 正确性换体积；
- tree depth 已下沉为 UI-core 的迭代 O(n) typed kernel，替代 con 每节点重复扫描 parent；20,000 深链、缺父、重复 ID、自环和多节点环均有单测；
- UI-core dirty region/row kernel 描述保守 raster candidate；vendored `vt100` 在 mutation 层输出无分配 row/cursor/model/viewport damage，以逐 Cell 精确比较作为无碰撞测试 oracle，未知 callback、resize、alternate screen 与 viewport 变化保守升级 full；con 在 PTY Wake 阶段先 drain 再按 changed rows 与旧/新 cursor 请求 redraw，公开 perf stats 同时记录 candidate、host direct/copy 和 platform-owned native present 证据；pixel-window frame 合同声明 backing 为 retained 或 transient，并要求提交 `None`/`Full`/bounded partial。Windows con 直接 raster 到 native retained XRGB buffer，allocation/resize/DPI 失效后强制 full；Unix/macOS 继续由产品 retained frame 向 transient softbuffer 完整复制。Windows adapter 将 typed physical rect 映射为 `InvalidateRect`，以 `PAINTSTRUCT.rcPaint` 驱动 top-down `StretchDIBits` partial present，并以 RAII 保证 `BeginPaint`/`EndPaint` exactly once、拒绝短 scanline 和 renderer error；Unix/macOS 当前 full present fallback，并在 event-loop 边界将 application panic 收敛为 typed failure；配对 Windows release 探针中 idle 平均 render 895 us -> 360 us（-59.8%），50-step send/wait 为 1,310 us -> 992 us（-24.3%），新版 250/250 direct、0 copy frame/pixel；post-row-damage Windows release 探针为 33/33 partial raster candidate、dirty/frame 约 0.40%、33/33 native present 成功，平台 ledger 仍诚实区分一次 529,584-pixel OS full expose 与 70,560 partial pixels；旧版 2/5、2/13 candidate 与约 6.7%/12.1% render 降幅仅为方向性历史证据，不作为发布资格基准；Win32 host 的 userdata owner、dispatch phase 和 bounded deferred queue 阻止同步 User32/IMM FFI 在 application/frame borrow 内重建 `&mut HostState`，复制 DPI/IME 等消息数据后再重放，nested paint validate 后重新 invalidate，overflow/nonconvergence typed-fail；随后将窗口回调和 deferred item 各收敛为一个 panic boundary，并以单一 typed message class 替代重复的 stateless/stateful matcher；原 abort 配置下同 profile release-fast PE 从 622,080 B 降至 621,568 B，512 B 收益全部落在 `.text` raw，`.rsrc` 不变；但该 profile 使 `catch_unwind` 在交付版直接 abort，测试默认 unwind 曾掩盖这一合同破坏。现由 `con-dev` / `con-release-fast` / `con-release` 独立构建完整 unwind 依赖图，Rh build 将其 `agenterm-con` 精确覆盖进原 staging 目录而不改变主程序 abort profile；三处 staged bytes SHA-256 相同，release-fast unwind PE 当前为 849,920 B，87 个单测、16 个黑盒（2 个既有 ignore）、1 个多标签控制面测试及专门的 release-profile panic containment test 通过。官方 con 构建现以显式 target、固定 `rust-src` 和局部 `RUSTC_BOOTSTRAP` 使用 Rust 1.97 `backtrace-trace-only + panic-unwind` 自建 std；自建 std 基线为 790,016 B，GDI+ 共享截图后当前为 790,528 B；精确 profile 的 87 个单测、16 个黑盒（2 个既有 ignore）、1 个控制面测试、x64 Clippy 与 Windows aarch64 编译通过；六平台 Candidate 与 sealed-byte 可复现性仍由发布门最终证明，512 KiB 仍是目标，不以恢复 abort 换体积；100-title OSC 公共压力为 883/883 direct、0 copy、0 present failure；
  con 的 resolved normal graph 为 59 行且不含 winit、softbuffer、Rhai、HTTP/TLS 或
  任一脚本 engine。拆包主要消除冷构建污染并允许 Windows con 默认选 native host，
  完整 native IME/capture/DPI 机制相对首个独立包基线增加 3,072 B；证明未使用根
  依赖原本已被 linker 裁掉，也证明关键系统交互无需引入大型框架。Windows 截图现由
  platform GDI+ adapter 直接编码 caller-owned XRGB/stride，不再维护 con 私有
  stored-DEFLATE、Adler-32、IEEE CRC-32、64 KiB block buffer 或全帧 RGB 副本；主程序
  和 con 复用同一 `write_xrgb_png` 合同。快照和截图分别通过 writer/path 两种 platform
  原子发布覆盖已有目标，不共享固定 `.tmp` 名；第三方 PNG decoder、原子覆盖和 GUI
  black-box 测试拥有格式/发布互操作证据。替换后 unwind+trace-only release-fast PE
  由 790,016 B 变为 790,528 B，净增的 512 B 全在 `.text` raw 对齐块，`.rdata`、
  `.pdata`、`.rsrc` 不变；接受这 0.06% 交换以删除双写并获得系统压缩，不伪称 FFI
  必然缩小二进制。
  570,368 B 基线 PE 的 section 证据为 `.text` 420,864 B、`.rdata` 119,296 B、
  `.pdata` 16,896 B、`.rsrc` 8,704 B；full-copy 已落到 CRT memcpy/memmove/memset，
  字体与 PTY 已落到 GDI/ConPTY FFI，pixel packing/blend 已有 SSSE3/AVX2/NEON，
  不再为这些路径新增手写汇编。已退役的 con PNG checksum 实验拒绝了未独立证明 reflected IEEE
  reduction constants/chunk combination 的 PCLMULQDQ/PMULL folding，也禁止用 CRC32C
  指令冒充 PNG polynomial；最终采用 1 KiB IEEE byte table 和共享 Adler-32 state，
  x86_64 SSSE3、aarch64 NEON、其余 scalar fallback；以下数字仅保留为被替换方案的
  历史证据。101 对公开 `screenshot-pane`
  交替样本中，scalar+nibble p95 31.215 ms、byte-table+SSSE3 p95 24.887 ms，改善
  20.27%，平均改善 23.94%，相同 PNG 字节数，release PE 只增加 2,048 B；同 byte
  table 下 SSSE3 相对 scalar Adler 两次正反序样本均改善约 5% average / 8-10% p95。
  Windows 字形路径通过 neutral `RasterGlyph` contract 调用 GDI
  `GetGlyphIndicesW`/`GetGlyphOutlineW`，con 不再读/解析字体文件，Windows 生产图也
  不含 ab_glyph/ttf_parser；Linux/macOS 的现有 file-font 实现下沉到 platform 的共享
  portable adapter。GDI 当前只接受单 UTF-16 unit，补充平面安全返回缺字而不拆
  surrogate；完整 emoji fallback 是否值得引入 DirectWrite 必须由后续体积/体验证据决定。
  con 的配置、script、snapshot 与 local-control wire 共用一个完整 JSON grammar：UTF-8、
  escape/surrogate pair、严格 number 均受 4 MiB 输入、32 层、65,536 nodes、256 object
  fields 和 1 MiB string 预算约束；重复 key、孤立 surrogate、非有限数和尾随数据
  fail closed。serde_json 仅作为 dev-only 独立 decoder oracle，Windows 生产图不再链接
  serde_json/derive；control 原有 newline framing 与 1/2 MiB request/response 预算不变。
  Windows resource 复用现有图标的 16/32/64 PNG frames，compact ICO 为 7,658 B，
  `.rsrc` 从 90,112 B 降到 8,704 B；build script 强制 16 KiB source-icon budget，
  Windows shell 已成功提取 32×32 associated icon。release-fast 的 con-only one-CGU
  override把默认 staged PE 保持接近 release；无 LTO 快版不得冒充 release 发布证据。
  后续体积工作必须继续归因实际链接段，不以 strip 或 package 拆分冒充进展。

### 6.1 跨平台任务固定执行句式

1. 判定：platform **机制** / frontend **产品语义** / host **present**？  
2. 机制 → 改 `crates/agenterm-platform`，feature 与 typed Unsupported **诚实**更新。  
3. 产品 → 改 `src/frontend/*` + `ui_action_catalog`，再改 **两端** adapter。  
4. 仅 host → 只动对应 adapter，并登记 catalog allowlist 或 gap 表。  
5. 证据：相关 `cargo test -p agenterm-platform` + `cargo test --lib ui_action_catalog` + 直接单测；**无证据不宣称三端手感已齐**。  

7. 不要把 rust-analyzer / 通用 LSP 当成「结构 SSOT 已对齐」的证据；LSP 不消费本文。  
8. 不要新开第二份「现行结构图」md；扩展对齐能力只加闸/机读清单并回写 **本节/§8**。  
9. **文档脱敏**：仓内 → 仓库相对；各平台用户主目录的展开形式 → **`~/...`**（详见 [`Agents.md`](../Agents.md) Home conversion table；自检 [`scripts/doc-redact-check.sh`](../scripts/doc-redact-check.sh)）。

---

## 7. 验证入口（本地）

```text
.\check.cmd --quick          # lint + 主 crate 单测（含 boundary_tests）
cargo test -p agenterm --lib platform::boundary_tests   # 结构红线闸（路径以实际 module 为准）
cargo test -p agenterm-platform --all-features   # 含跨进程；shm 名长已知红见 D1
```

Quick 绿 ≠ 六平台 CI / Candidate。  
Quick 绿 **≠** 「ARCHITECTURE.md 与目录树全文一致」（见 §8）。

---

## 8. 结构如何被勾住（对齐机制 · 工具边界 · 升级路径）

> 沉淀自 2026-08-05 结构 review / 工具澄清。**契约在本文**；实现排期在版本 plan **S 组**。

### 8.1 三角关系（今日真相）

```text
plan/ARCHITECTURE.md     人读结构 SSOT（分层/禁令/债务）—— 权威叙述
        │
        │  人维护；无解析器读全文
        ▼
src/** + crates/**       真实模块树与所有权
        │
        │  cargo test 跑局部规则
        ▼
boundary_tests.rs        结构红线闸（不是全文 diff 引擎）
```

| 组件 | 角色 | 是否「双向」 |
|------|------|----------------|
| 本文 | 现行结构叙述 SSOT | 否（人手） |
| `boundary_tests` | 代码侧可机检红线 | **单向：代码规则** |
| `prd-alignment.rh` | PRD 能力/证据/命令目录 | **另一轴**，非结构树 |
| rust-analyzer (LSP) | 跳转/补全/重命名 | **编辑助手**，不校验分层 |

**结论**：已有「钩」，但是 **局部自动 + 全局靠纪律**；**未能**做到「改 md 自动约束代码 / 改目录自动改 md」的全自动双向对齐。

### 8.2 `boundary_tests` 今日覆盖（勾住了什么）

| 测项（概念） | 勾住的结构意图 |
|--------------|----------------|
| 产品 `src/**` 禁原生 marker / `cfg(target_*)` | 原生边界只在 `crates/agenterm-platform` |
| platform crate 禁产品耦合 marker | 机制 crate 无 AgenTerm 产品名/路径 |
| adapters 同契约 declaration | 三 OS adapter 合同形状一致 |
| `services/*` 无 orphan 源文件 | 防再长已删的 `services/frontend` 类 |
| `frontend` `#[path]` 预算 = 0 | L1 债务不回潮 |

**未覆盖（故会漂）**：§1 目录/分层 prose、§2 bins 表与 `src/bin/*` 一致性、巨石文件行数、Win/Unix `ui-action` 表是否同一 ActionId 集、policy/services 半迁移是否收口、本文债务表 L* 是否过时。

### 8.3 工具地图（别用错层）

| 层级 | 代表工具 | 与结构 SSOT 的关系 |
|------|----------|-------------------|
| LSP | rust-analyzer | 写代码顺手；**不**消费本文、**不**当对齐证据 |
| 构建 | `cargo check` / `cargo test` | 模块能编过；orphan `mod` 会红 |
| **本仓结构闸** | `boundary_tests` | **唯一官方结构红线机闸** |
| 能力对齐 | `prd-alignment.rh` + alignment-contract | shipped/证据，**非**分层树 |
| 静态分析 | clippy / 可选 semgrep·ast-grep | 可补模式禁令；非 SSOT |
| 依赖图 | `cargo-modules` / depgraph 等 | 发现巨石与环；**辅助**，不替代本文 |
| 文档生成 | 自写 tree 脚本 / rustdoc | 可做 **代码→文档片段** |

结构工作 = **约定文档（本文）+ 测试/脚本闸 +（可选）依赖图**；不是「装个 LSP 插件」。

### 8.4 升级路径（要真·双向时）

自由 prose MD ↔ 任意 Rust **无法**可靠全文双向。可机读路径：

```text
A 扩 boundary_tests（单向规则）     必存在/禁路径、软行数预算、ActionId 完备性…
B 代码→文档围栏（半自动）           扫树生成 ```structure 块；CI diff 本文围栏
C manifest 真源（推荐长期）         architecture.manifest.{toml,json}
                                    → 生成 ARCHITECTURE 可机读块 + 同一清单喂测试
```

| 档 | 做到什么 | 仍靠人 |
|----|----------|--------|
| A | 红线不破 | 叙事/分层解释 |
| A+B | 目录树不静默漂 | 禁令语义措辞 |
| C | 改清单驱动文档+闸 | 清单本身的产品决策 |

**禁止**：再立第二棵「现行结构」md 冒充双向；扩展只加闸/机读清单并回写本文。

### 8.5 与封装/复用 review 的关系

巨石拆分、`ui-action` 表驱动、client 切分等 **改进建议** 不写入本文执行清单（防第二现实）。  
债务钩子：**L2**（双主机/巨石）、**L3**（policy/facade）、**L4**（SSOT 机读）。  
执行叶：当前版本 plan（如 `plan-v0.1.15` **S 组** + **§九 预备树**）；落地后 **同批** 更新本文 §1/§3/§4。  
**HOLD**：多 agent 并行时 S 泳道不写主树；用户通知复审后再按 §九 刀序开工。不必等 S3 全文双向才微重构。

### Shared terminal interaction geometry

`crates/agenterm-ui-core` is the allocation-free, host-neutral boundary for
scrollbar geometry, hit testing, and drag mapping. Product hosts retain layout,
palette, viewport state, capture, rendering, and OS event adaptation. The pixel
window contract exposes typed pointer cursors; Windows implements them through
Win32 cursor FFI and Unix/macOS through the native winit adapter.

The crate also owns bit-exact XRGB alpha-mask row composition and RGB8 packing.
It selects AVX2/SSE2 or SSSE3 once on x86_64, uses NEON on aarch64, and retains
scalar references for other architectures and parity tests. Rectangle fill
shares safe clipping/stride/full-frame collapse but deliberately retains
`slice::fill`; emitted-code inspection found no reason to own another ISA fork.
Architecture kernels never own terminal cells, fonts, layout, or frame lifecycle.
