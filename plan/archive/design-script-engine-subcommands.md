# ⚠️ 已归档：脚本引擎子命令化设计

> **归档于 2026-08-10。** SUB-M1～M4 已落地，公开入口与退役工作由当前 PRD、
> `plan/archive/plan-v0.1.17.md` E5 和黑盒合同拥有。本文保留实现前设计与历史决策，不再派工。

# 脚本引擎子命令化：`agenterm {rh|lua|qjs|sql}` 设计（历史原稿）

> 2026-08-09。用户指示：「准备安排把几种后端（rh,lua,qjs,sql）做成 agenterm 的子命令」。
> 本文档是执行前设计；分期见 §5。与 [`design-agenterm-cli-merge.md`](design-agenterm-cli-merge.md)
> 同族——复用同一套 GUI-子系统控制台接管机制，不重复发明。
>
> **后续决策更新（2026-08-09，同日）：** SUB-M1～M4（commit `234b2f87`）已落地
> 子命令别名。用户随后决定**废弃**四个独立 exe（`agenterm-rh.exe` /
> `agenterm-lua.exe` / `agenterm-qjs.exe` / `agenterm-sql.exe`）——这推翻了
> §6「不废弃独立 bin」非目标；根 `Cargo.toml` 的四个 `[[bin]]` 条目在后续
> wave 中删除，CI/打包/安装脚本/文档全部改指向 `agenterm {rh,lua,qjs,sql}`
> 子命令形式。§0～§5 的机制设计和分期记录本身保持不变（历史准确）。

---

## 0. 现状与目标

```text
现状：
  agenterm-rh.exe  check x.rh     → 独立 PE（根包 [[bin]]，console 子系统）
  agenterm-lua.exe check x.lua    → 同上
  agenterm-qjs.exe check x.js     → 同上
  agenterm-sql.exe check x.sql    → 同上
  agenterm.exe cli <args>         → 已实现：GUI PE 自我 re-exec 跑 CLI（见 §1）

目标：
  agenterm.exe rh  <args>         → 与 agenterm-rh.exe <args> 完全等价（argv 透传别名）
  agenterm.exe lua <args>         → 同上
  agenterm.exe qjs <args>         → 同上
  agenterm.exe sql <args>         → 同上
  agenterm-{rh,lua,qjs,sql}.exe   → 保留为薄转发（兼容期），未来是否废弃是发布决策
```

**等价性定义**：退出码、stdout/stderr 内容、对 stdin/管道的行为逐字节一致——
`tests/script_cli_verb_parity.rs` 已经把四引擎的 CLI 契约钉住了，别名路径必须
通过同一套断言（见 §4）。

## 1. 机制：复用已实现的 `agenterm cli` 通道

`src/bin/agenterm.rs` 已经解决了本设计最难的问题——GUI 子系统 PE
（`windows_subsystem = "windows"`）如何跑控制台型子命令：

- `agenterm cli <args>`（`src/bin/agenterm.rs`，`run_cli_from_gui_subsystem`）：
  attach 父控制台 → `duplicated_std_handles()` 复制真实 stdin/stdout/stderr
  （console/pipe/file 都对）→ 用 `__agenterm-internal-cli` 内部标记自我 re-exec，
  显式接线复制的句柄 → 子进程在 Rust 缓存 std 状态**之前**就持有有效句柄，
  走普通 CLI 入口。
- 这套机制已在产（`agenterm cli list-windows` 等），不是提案。

引擎子命令 = 同一通道加一个「入口选择 token」：

```text
agenterm rh check x.rh
  → run_cli_from_gui_subsystem 变体：re-exec 自身
    `__agenterm-internal-engine rh check x.rh`（句柄接线同 cli 路径）
  → 子进程分发：token "rh" → rh CLI 入口(["check","x.rh"])
```

Unix 侧无子系统问题，直接进程内调用入口函数即可（不需要 re-exec，
但为实现单一性也可以统一走 re-exec——分期时用最小实现，见 §5 SUB-M3）。

## 2. 入口函数抽取（前提工程）

子命令分发需要「从 agenterm 主 bin 可调用的引擎 CLI 入口函数」。现状是四个
`main.rs`（都编译为根包 [[bin]]，但 **bin target 的代码对 lib 不可见**）：

| 引擎 | main.rs 依赖 | 入口去处 |
|------|-------------|---------|
| lua  | 仅 `agenterm_lua::*` + script-common | 抽到 `agenterm_lua::cli::run(args) -> u8`（crate 内新模块）|
| qjs  | 仅 `agenterm_qjs::*` + script-common | 抽到 `agenterm_qjs::cli::run(args) -> u8` |
| sql  | 仅 `agenterm_sql::*` + script-common | 抽到 `agenterm_sql::cli::run(args) -> u8` |
| rh   | **用了 `agenterm::` 内部**（incremental_wrapper、`run_legacy_worker_stdio`、`run_framed_worker_stdio`、task dispatch）| 抽到根 lib 新模块 `agenterm::script_rh_cli_main::run(args) -> ExitCode 等价物`（不能进 agenterm-rh crate——依赖方向反了）|

抽取后各 `main.rs` 变成 3-5 行薄壳（收集 argv → 调入口 → exit）。
**行为零变化**——这一步不碰任何 cmd_* 逻辑，纯搬家 + 可见性。

注意 rh 的 worker 模式（`--worker`/`--framed-worker`/`--internal-incremental-finalize`）
也要透传：`agenterm rh --worker` 必须和 `agenterm-rh.exe --worker` 等价，
因为 worker spawner 未来可能只认主 PE。入口函数签名要能承载这些模式
（rh main.rs 现在 main() 顶部就分流了，抽取时整块搬）。

## 3. 分发点

`src/bin/agenterm.rs` 的 main() 早期 argv 检查（现有 `cli`/`server` 同级）：

```rust
const ENGINE_SUBCOMMANDS: &[&str] = &["rh", "lua", "qjs", "sql"];
// args.first() 命中 → Windows: re-exec 通道（§1）；Unix: 直接调入口
```

冲突检查：`rh`/`lua`/`qjs`/`sql` 与现有 first-arg 语义（`cli`、`server`、
`--server`、`--version`、`--help`、GUI 默认路径）无碰撞——现在这些 token
落到 GUI 启动路径里当无效参数，没人依赖。

内部标记沿用 cli 的命名法：`__agenterm-internal-engine`（或直接扩展
`__agenterm-internal-cli` 携带 engine token——实现时选改动最小的，
倾向后者：一个内部标记 + 第一个参数是入口选择器，`cli` 也是选择器之一，
统一「agenterm 内部 re-exec 多路入口」）。

## 4. 测试与守护

- `tests/script_cli_verb_parity.rs` 增加别名等价性维度：现有每个场景
  （version/check valid/check broken/check-many/未知动词/sql 保留动词/
  `--project-root` foreign-CWD）在 `agenterm <engine> <args>` 路径上复跑，
  断言退出码 + 关键输出与独立 bin 逐场景一致。实现上给 `Engine` 结构加一个
  「调用方式」轴（standalone bin / agenterm subcommand），场景循环两轴笛卡尔积。
- Windows 句柄接线的坑（管道 vs 控制台）：cli 通道已经蹚过（`Stdio::inherit`
  不可信、必须显式复制句柄的注释就在代码里），子命令直接继承这些教训；
  parity 测试本身用管道捕获输出，天然覆盖 pipe 场景。
- 独立 bin 薄壳化后，各引擎自己的 bin 测试（如 `cargo test --bin agenterm-qjs`
  的 flag 回归测试）留在原处不动——它们测的逻辑跟着入口函数搬进 lib，
  测试引用路径需同步（搬到 crate 的 cli 模块测试里，或保持 bin 测试引用
  lib 入口）。

## 5. 分期

| 期 | 内容 | 前置 | 验收 |
|----|------|------|------|
| **SUB-M1** | lua/qjs/sql 三个 main.rs 的 CLI 逻辑抽到各自 crate 的 `cli` 模块，main.rs 薄壳化。行为零变化 | round-8 退出码 agent 落地（正在编辑 qjs/sql main.rs，先等合流） | 三 crate lib 测试不变全绿；`cargo test --bin` 各引擎不变；cli-parity 7/7 不变 |
| **SUB-M2** | rh main.rs 抽到根 lib `script_rh_cli_main`（含 worker 模式整块），main.rs 薄壳化 | 无（rh main 当前无人在编辑） | rh_backend 11/11、rh_framed_worker 2/3（既有失败不变）、rh 独立 bin 全动词 smoke |
| **SUB-M3** | `src/bin/agenterm.rs` 分发：Unix 直调 / Windows 走 re-exec 通道（扩展现有内部标记为多路入口） | SUB-M1+M2 | 手工 smoke：`agenterm rh version` 等四引擎、管道捕获、退出码 |
| **SUB-M4** | cli-parity 测试加别名轴（两轴笛卡尔积）；plan/PRD 记录；独立 bin 标注「兼容保留」 | SUB-M3 | parity 全绿（场景数约翻倍） |

## 6. 非目标

- **不废弃独立 bin**（本设计只加别名路径；废弃与否是发布/分发决策，
  涉及 dist 布局和用户脚本兼容，留给版本 plan）。
- **不动 `agenterm cli` 通道的语义**（只复用/扩展其内部标记机制）。
- **不在本设计内统一 rh/lua 的退出码 quirk**（usage 错误 exit 1，
  cli-parity 已钉住，是各自轨道的债）。
- **不做 shell completion / 帮助文本大改**（`agenterm --help` 提及四个
  子命令即可，各引擎自己的 --help 原样透传）。

## 7. 风险

- **共享 checkout 并发**：qjs/sql main.rs 正被 round-8 agent 编辑（退出码
  分类）；`src/bin/agenterm.rs` 与 cli-merge 轨道相邻。SUB-M1 必须等
  round-8 合流；SUB-M3 动 agenterm.rs 前先 `git status` 确认无人在编辑。
- **rh worker 模式透传**：worker 协议对 argv 形状敏感（`--framed-worker`
  是精确匹配单参数），透传实现必须保持「skip(1) 后原样」，不做任何
  规范化。SUB-M2 的 framed-worker 测试是守门员。
- **Windows 句柄边界**：新入口忘记走 re-exec 通道、直接进程内 println! 会
  静默丢输出（GUI 子系统）。SUB-M4 的管道断言能抓到。
