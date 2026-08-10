# ⚠️ 已归档：CLI 输入面与跨平台封装收口 goal

> **归档于 2026-08-10。** 已完成与残余 parity 工作由
> [`../agent-human-parity-audit.md`](../agent-human-parity-audit.md)、UI action catalog 和 owning PRD
> 接管。本文是历史 goal 快照，不得再作为活跃 `/goal` 执行。

# /goal — CLI 输入面与跨平台封装收口（历史快照）

> 用法：把下面 `--- GOAL ---` 之间的内容整段发给 agent 即可。
> 目标是可以「执行到底」的，不需要中途回答问题；遇到产品口径分歧时按文末规则处理。

--- GOAL ---

在 agenterm **仓库根**继续推进 v0.1.15 的 **CLI 输入面** 与 **跨平台封装** 两条线，自主执行到底。

## 背景（已验证，不要重新论证）

- **观察面已经很完整**：`agenterm-cli ui-snapshot` 在 `projection: "embedded_gui"` 下已经输出几乎所有可点元素的像素 bounds（toolbar 各按钮、tab 行、close/new_child、disclosure_hit、滚动条 thumb/track、sidebar resize_grip、composer input），并带 `focus`/`caret`/`anchor`/`selection`。
- **动作面基本是空的**：`send-mouse` 在 `commands.rs:707`、`control_authority.rs:251`、`client/mod.rs:4927/5034` 四处声明，但 `control_dispatch.rs` 里**没有任何 dispatch 分支**；实测 `agenterm-cli send-mouse -x 5 -y 5 --button left --action press` 返回 `Unix GUI does not implement 'send-mouse' yet`（`unix/frontend/mod.rs:4087`）。而且它的参数是 `-x col -y row`（终端单元格），本来就是 tmux 的「往 PTY 写鼠标上报」，**不可能点到工具栏按钮**。
- `ui-action` 是约 25 个手工登记的语义动词（`src/operations.rs`），每加一个人类手势就要手写一个动词，永远追不平。
- `composer-send` / `select-tab` / `new-child` / `toggle-tree` 在 `control_dispatch.rs` 有实现，但**没登记进 `operations.rs`**。

## 任务

### T1（主线）像素级输入原语
新增 CLI/控制协议命令，坐标用**像素**、与 `ui-snapshot` 的 bounds 同一坐标系：

```
ui-input pointer --x PX --y PX --button left|right|middle --action press|release|move [--count 1|2|3] [--mods shift,ctrl,alt,meta]
ui-input wheel   --x PX --y PX --delta-y N
ui-input key     --key NAME [--mods ...]
```

**硬性约束：必须合成 `PixelWindowEvent` 喂给现有的 GUI 事件入口**（`unix/frontend/mod.rs` 里 `PointerMoved` / `PointerButton` / `MouseWheel` 的同一条路径），**不允许另写一份 hit-test**。理由：Windows 曾把 composer 外包给原生 EDIT 控件、Unix 自己画，两套选区实现各走各的，导致 Unix 长期没有鼠标选区没人发现。第二条实现必然再次漂移。

验收：能跑通 perceive→act 闭环 —— 读 `ui-snapshot` 拿到 `tabs[0].actions.close.bounds` → 点它的中心 → 再读 snapshot 确认标签关闭。并且能用 press/move/move/release 驱动 composer 拖拽选区（这是 `c5b31ee` 那批选区功能目前唯一可机器验证的方式）。

### T2 目录漂移
把 `composer-send` / `select-tab` / `new-child` / `toggle-tree` 补登记进 `src/operations.rs`；`ui-input` 也要同步登记。
**注意**：`tests/rhai_migration.rs` 的 `prd_alignment_task_matches_public_catalogs_and_fails_closed` 会 fail-closed，加公开命令必须同时更新 `prd/` 目录与该测试里 pin 的计数串。

### T3 跨平台封装（每次改动都问一次）
改任何 `src/platform/adapters/unix/**` 的代码时，先判断这是 **Unix 共性** 还是 **OSX 个性**：
- 共性 → 留在 unix 共享层；
- 个性 → 下沉到 `src/platform/policy/**` 或 `crates/agenterm-platform/**`，并在 lnx/win 对应位置留 `TODO(linux)` / `TODO(windows)`，写清楚**该读哪个系统 API**，不要只写「待实现」。

已建立的样板：`src/platform/policy/input.rs` 的 `multi_click_interval_ms()` / `caret_blink_interval_ms()`。

### T4 顺手的 review 注释
在 win/lnx 分支发现问题时，用 `REVIEW(macos → windows owner):` / `TODO(linux):` 注释写进对应文件，不要替对方改实现。

## 已知未决（不要自己拍板，做完在报告里列出来）

1. Windows 独有的 **多服务器顶栏 strip** 与 **instance picker**（Unix 明确返回 `"instance picker is Windows-first in this build"`）要不要补到 Unix —— 这是产品排期问题。
2. headless 的 `ui-snapshot` 不带几何（`server_app.rs:1908` 硬编码 `composer.visible:false`、`focus.surface:null`），且 macOS/Linux 的 `screenshot` 需要活着的渲染进程。所以 **T1 的闭环目前只能在有窗口的会话里跑，CI headless 跑不了**。要不要让 headless 也供几何，是架构取舍。

## 已知的既有红灯（不是你引入的，别当成回归）

`cargo test --test rhai_migration` 目前 2 个 fail，**实测**在 `5db830f~1` 的干净 worktree 上就已经是红的（当时是 4 个，其中 `prd_alignment` 和 `preflight_benchmark` 已修）：

- `artifact_manifest_task_accepts_canonical_contract_and_rejects_invalid_fields`
- `child_id_remains_public_after_process_completion`（`tests/rhai_migration.rs:959`，`Bool(true)` vs `Bool(false)`）

这两个没人认领。要么顺手修掉，要么在报告里点名，别默默走过。

## 工作纪律（这个仓库有并发 agent）

- 提交必须精确 pathspec，**禁止 `git add -A` / `-u` / `.`**；`cargo fmt` 要带 `-p` 限定包，改完 `git status --short` 检查有没有扫到别人的文件，扫到就 `git checkout --` 还原。
- 每轮结束前跑 `cargo test --lib` + `cargo clippy --all-targets`；**整合类改动还要跑 `cargo test --test rhai_migration`**（`--lib` 绿不代表集成绿，`kernel32` 那次事故就是这么漏的）。
- 测试失败先归因再下结论：「不是我引入的」≠「随机 flake」。按 baseline 全量 / 带改动 / baseline 单跑 三种测量区分，不要凭印象。
- GUI 改动**必须 `./install.sh --local-build target/release` 装进去再验**，跑 `target/release` 的二进制不等于用户在用的那个。
- 做完 `git pull --rebase` → 重跑测试 → `git push`。

--- END GOAL ---

## 备注

上面第 1、2 条是**决策项**，交给你拍板；agent 应当把它们做完其余部分后列出来，而不是自己选一个方向做掉。
