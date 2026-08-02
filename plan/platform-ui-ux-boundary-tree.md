# 跨平台 UI/UX 边界树（执行版）

- [x] 目标：让 `agenterm-platform` 只承载「能力」与「机制」；主进程 `src/` 承载「产品语义」。
- [x] [已完成] 启动参数解析已抽离到 `src/frontend.rs` 的共享策略器，Windows/Unix 前端入口复用同一 `parse_gui_launch_arguments` 规则，仅保留平台能力差异（地址校验/ui-client 开关）。启动/唤醒分发已收敛到统一能力入口。
- [x] [已完成] 客户端控制入口的服务端自动拉起也统一到 `src/frontend_server.rs`：`src/client/mod.rs` 的 `start_server_process` 仅委托 `start_frontend_server_process`，不再重复构造端点参数与 autostart 决策。

```text
src/platform
├─ services
│  ├─ frontend.rs
│  │  ├─ run_gui_entry(): only dispatch by PlatformKind
│  │  ├─ request_gui_wake(): only dispatch by PlatformKind
│  │  └─ 当前已是能力路由起点，下一步目标是统一启动/唤醒失败语义
│  ├─ ipc.rs / paths.rs / control_center.rs / process.rs / supervisor_audit.rs
│  │  ├─ 保留：跨平台能力拼装（端点/路径/截图/进程/审计）
│  │  └─ 风险点：避免继续在此层引入 UI 语义条件分支
│  └─ control_center_shell.rs / script_*：保持协议与脚本宿主语义
│
├─ adapters/windows
│  ├─ frontend.rs
│  │  ├─ Windows GUI launcher（参数规范化、handoff、server handoff）
│  │  └─ 仅承载 Win32 启动方式与错误文案
│  ├─ remote_frontend.rs
│  │  ├─ Win32 控件 + 交互主循环 + 远程替代客户端渲染层
│  │  ├─ 负责：native hwnd/消息、输入底稿、raw 鼠标等主机行为
│  │  ├─ 不应持有产品分支策略；仅消费 control_dispatch/ui_bridge 快照语义
│  │  └─ 当前差距：产品语义仍需持续用树结构核对
│  ├─ wake.rs
│  ├─ font.rs / input.rs / clipboard.rs / window.rs / toolbar.rs / integration.rs
│  └─ 具体机制均通过 crate `agenterm-platform` 内部能力化对齐
│
├─ adapters/unix/frontend
│  ├─ mod.rs
│  │  ├─ Unix 嵌入式窗口生命周期（open、render、focus、输入循环）
│  │  ├─ 产品状态机（侧栏/terminal/compose/setting/screenshot/paste）在主仓保留
│  │  └─ 与 win 对齐目标：统一点击/滚轮/选区/焦点语义而非平台条件分支
│  ├─ input.rs
│  │  ├─ winit/输入事件归一化的主机入口
│  │  └─ 与 product policy 的快捷键/行为映射应保持契约一致
│  ├─ font.rs / render.rs / wake.rs
│  └─ 统一目标：平台差异留在能力层（字体/缩放/激活/截图）

agenterm-platform crate（复用层）
├─ window/input/clipboard/font/screenshot/pty/ipc/process/integration
│  ├─ 提供跨平台能力定义与错误码
│  ├─ selected.rs 统一宿主适配器选择
│  └─ product 只消费能力，不持有 winit/windows-sys/libc/直接 OS 条件分支
```

## 统一语义分支清单（当前 vs 目标）

```text
行为分支
├─ 启动入口（run_gui_entry）
│  ├─ 当前：run_gui_entry 做 OS dispatch
│  └─ 目标：dispatch 后返回统一失败类语义（supported/unsupported/failed）
│
├─ 启动唤醒（request_gui_wake）
│  ├─ 当前：Windows 发 HWND wake；Unix 发 proxy/通道信号
│  └─ 目标：统一返回与回退策略，避免产品层猜测 host 语义
│
├─ 焦点与恢复行为
│  ├─ 当前：Windows 和 Unix 分别通过各自窗口主机实现
│  └─ 目标：在共享快照/契约层可比较差异，只在能力缺口上分歧
│
└─ 输入/选择/滚轮
   ├─ 当前：事件源归一化在 host 后进入同一产品处理管线
   └─ 目标：抽象测试场景与证据，用“行为不一致=能力缺口”记录
```

## 下一步动作树（建议并行）

```text
- O1A（Platform）：统一 startup/wake 结果为可归并失败码（已完成）
  - owner: Platform services
  - output: 运行时错误码（supported/unsupported/failed）文档；本轮要求 `request_gui_wake` 调用都通过 `request_gui_wake_best_effort` 上报失败语义
  - acceptance: 无策略分支猜测、证据可归集
  - 当前收敛: `replaceable_ui_client` 投影名与共享系统菜单/投影常量从 `src/ui_snapshot.rs` 统一引用到 `server_app`/`client`/Windows 远端前端，减少平台/模块内重复。
  - 已完成: 启动/恢复 server 的决策逻辑从 `src/platform/adapters/windows/remote_frontend.rs` 提取为独立产品域 `src/frontend_server.rs`，并由 `src/platform/adapters/windows/remote_frontend.rs` 仅消费该域接口（`connect_or_start_frontend_gui_client` / `FrontendServerRecovery*`），减少平台适配器中的生命周期分支。
  - 本轮补充: 控制平面 CLI 的 `start_server_process` 也已委托给 `frontend_server::start_frontend_server_process`，避免服务端启动策略在 `src/client/mod.rs` 和 `remote_frontend` 中重复定义。
  - 本轮收口: 新增 `FrontendContractState`，并由 `GuiLaunchResult` / `GuiWakeResult` / `FrontendServerRecovery` 映射统一状态（Supported/Unsupported/Failed）；便于 evidence 统一归档与分支阻断。
  - 本轮补充: 把 `autostart_server` 的平台分支下沉到 `src/platform/process.rs` 的编译期分支（Windows 真正执行、非 Windows 返回 `Ok(false)`），去掉运行时 `platform_kind` 条件，提高平台能力边界清晰度。
  - 本轮补充: 客户端启动链路因此不再依赖 `platform::is_windows_host()` 做 `autostart_server` 的运行时分支判断。
  - 本轮补充: `src/frontend.rs` 不再直接维护平台 host 分流，改为消费 `platform::frontend_host()`，统一 host 判定入口。
  - 本轮补充（已合并）：把 `platform/services` 的平台策略集中收口到 `src/platform/mod.rs`，删除 `control_center/script_http/ipc/paths/supervisor_audit` 在服务层的直接 `platform_kind` 分支，并将 `agenterm-script` 名称残留统一清理为 `agenterm-rhai`。

- O1B（UI）：统一输入-选择-滚轮场景模板（已完成）
  - owner: Human Workspace + ui_bridge
  - output: Given/When/Then 场景清单 + 对应快照锚点
  - acceptance: windows/unix 场景映射同名字段一致
  - 本轮动作：`unix` 前端渲染层中，移除 `platform_kind` 直接分支，`NewShellChoice::label` 改为复用 `platform::runtime::primary_terminal_shell().label`，让“主终端命名”由平台能力层提供，前端仅消费显示字段。
  - 已完成：把 `unix` 输入单测中的 `platform_kind` 断言改为走平台策略能力（`primary_text_field_shortcut_modifiers` / `is_primary_shortcut_via_meta` / `terminal_shortcut_empty_copy_action_is_suppressed`），减少测试层面的平台分支直接感知。
  - 已完成：增加 `platform` 侧输入策略一致性测试（`primary_shortcut_policy_is_internal_consistent` / `primary_shortcut_policy_matches_runtime_kind`），把“平台策略边界”收敛为可验证产物。
  - 本轮补充：`scripts/rhai/platform-ux-parity-smoke.rhai` 改为按已上报证据逐条计算状态，避免一次失败掩盖已通过场景的 `Supported`。 

- O1C（验收）：并发回归与阻断
  - owner: QA
  - output: 平台缺口矩阵（Unsupported/Failed/Supported）+ 统一脚本入口
  - handoff: 更新 `plan/platform-ux-parity-evidence-matrix.md` 与 `plan/plan-unix-gui-win-parity.md`
  - acceptance: 平台证据树完整，分支失败可快速定位
```

## 首轮能力缺口矩阵（可执行）

```text
能力面（UI/UX 启动与唤醒层）
├─ GUI 入口路由（run_gui_entry）
│  ├─ Windows：Supported
│  │  ├─ 方式：`src/platform/adapters/windows/frontend.rs`
│  │  ├─ 参数：`--no-activate`, `--not-foreground`, `--ui-client`, `--endpoint`, `--address`, `--instance`
│  │  └─ 回退：参数错误 => 含具体错误码的 stderr + 2，handoff 被拒绝 => 1（无静默失败）
│  ├─ Linux/macOS：Supported
│  │  ├─ 方式：`src/platform/adapters/unix/frontend/mod.rs`
│  │  ├─ 参数：`--no-activate`, `--not-foreground`, `--endpoint`, `--address`, `--instance`
│  │  └─ 回退：参数错误 => 含具体错误码的 stderr + 2（无静默失败）
│  └─ 其他OS：Unsupported（`frontend` 返回 1）
├─ 前台唤醒（request_gui_wake）
│  ├─ Windows：Supported（`post_application_wake`）
│  ├─ Linux/macOS：Supported（`unix` wake proxy）
│  └─ 其他OS：Unsupported（已去 panic，no-op）
├─ 入口分发层
│  ├─ 目标状态：统一能力选择，不内嵌 UI 语义分支
│  ├─ 当前状态：`src/platform/services/frontend.rs` 已收敛到统一 `frontend_host()`
│  └─ 下一动作：把失败语义映射为 `supported/unsupported/failed` 文本标签供证据采集
```

```text
能力面（snapshot 与交互可观测层）
├─ 窗口快照（`ui-snapshot` 形态）
│  ├─ 对齐目标：`projection`、`layout.sidebar`、`selection` 字段在 Win/Unix 语义等价
│  ├─ 当前状态：P0/P1/P2 已有多个已完成项；跨场景回放矩阵尚待闭环
│  └─ 回退：能力缺口先记录为 `Unsupported` 而不是字段重映射
├─ 选择/滚轮/焦点
│  ├─ 对齐目标：同场景同字段差异最小化，差异只发生在能力缺口
│  ├─ 当前状态：行为主逻辑趋同，残差收敛交由 O1B/O1C 场景脚本
│  └─ 回退：以回归脚本证据为唯一判定，禁止“口头对齐”放行

## 本轮平台边界巡检（2026-08-02）

- 目标：确认 UI/UX 体验分支是否真正集中在 `src/platform` 能力层。
- 结果：
- `src/platform/adapters/unix/frontend/input.rs`/`render.rs`/`new_terminal.rs` 已以 `platform` 能力 API 驱动快捷键与主终端 shell 命名，不再出现新的 OS/主机策略分支外泄。
  - `src/platform/adapters/windows/remote_frontend.rs` 继续承担 Win32 交互宿主职责，但未新增产品语义分支。
- 全仓库 `src`（排除平台crate）里，非测试的 `#[cfg(windows)]`/`#[cfg(unix)]` 直接分支已降到可接受边界；本轮已完成 `instances.rs`、`workspace.rs`、`working_context.rs`、`script_stdlib.rs`、`script_process.rs` 的测试与兼容性路径适配收口，全部改为基于 `platform` 能力契约分支。
- `script_process.rs` 测试辅助中与 shell/超时进程相关的 host 分支已从编译期 `#[cfg(windows)]`/`#[cfg(unix)]` 改为统一 `crate::platform::is_windows_host()` 的运行时能力分支，避免在通用 API 上重复注入主机策略。
- 本次同步进度（2026-08-02）：再向下沉 3 个测试分支（`script_process` 的 runtime 分支、`script_stdlib`/`working_context`/`workspace` 的 OS 条件），确保非平台层的运行时 `host` 判断继续收敛。
- 本次同步进度（2026-08-02）：`remote-ui-smoke.rhai` 已加 `window_control` 空对象防护，回归脚本对平台差异改为“控制项缺失=明确证据项”而非直接 `()` 属性取值 panic；`src/workspace.rs` 的平台分支测试已改为调用 `platform` 能力语义。
- 追加（2026-08-02）：`platform/mod.rs` 增加 `workspace_layout_kind()` 语义类型，`workspace.rs` 测试改以 `WorkspaceLayoutKind::WindowsFlat` 显式表达“路径形态能力”，减少 bool 语义泄露。
- 追加（2026-08-02）：`script_process` 与 `script_stdlib` 已将剩余 OS 分支下沉到 `platform` 能力函数（长任务命令、超时命令、shell 选择、Windows 长路径语义），减少上层测试路径对主机分支的直接判断。
- 下一步：
  - O1C 补强：
    - 将行为差异记录到 `plan/platform-ux-parity-evidence-matrix.md` 的 P0/P1/P2 回归条目，并补齐 GUI/CLI 双端最小冒烟证据脚本的证据汇聚策略。
    - 对 `script_process` 中的运行时主机判定场景，优先统一为能力入口返回值语义（supported/unsupported/failed）而非直接 `cfg!(windows)` 语义分支。

- 实测验收（src 非平台层）：
  - 截止 `2026-08-02`，`src` 非平台目录未发现剩余 `#[cfg(windows)]`/`#[cfg(unix)]` 或 `cfg!(windows)`/`cfg!(unix)` 直接分支；仅保留与行为可见性相关的测试辅助路径分支（如 `script_process` 的兼容性路径）并通过 `platform` 能力入口落地。
```

