# /goal：v0.1.15 S′ 多 Server / Instance 可达

> 权威细节与验收条文：`plan/plan-v0.1.15.md` **§一·五 S′**。  
> 本文是可执行交接；冲突以 plan 正文为准。

## 目标

关窗后再开**找得回、认得出、能开另一 instance**。  
实现 **S1 + S2 + S3**。Windows 优先；Unix 能顺带则顺带，否则诚实记 gap。

## 禁止

- **主终端顶栏横向 server tab**（用户原话诉求的形态；plan 明确不做）
- **S4** 同窗热切换权威（后置，非本 goal）
- 第二套发现协议 / 第二权威；在同窗静默换 endpoint
- 把 instance 选择器画成第二套 PTY tab 条
- 为「修桌面」启动 `explorer.exe`（用户可能用 Cairo 等壳）
- 回退已有 P0 附着 live peer / 拒双 main

## 交付（顺序）

| 叶 | 做什么 | 验收一句话 |
|----|--------|------------|
| **S2** | 主窗身份常显：instance 名 + 可选短 pid（标题和/或状态栏） | 两窗分挂 main/work 时 `ui-snapshot`/标题可区分 |
| **S1** | 冷启动或未附着时：live/stale instance 列表，点选附着；无 live 诚实空态 + 启动默认 | 只留 server 杀 GUI 再开，列表点回同一 `server_pid`/tabs；stale 不可当 live |
| **S3** | 工具栏/菜单 `Open instance…` → 复用 S1 列表 → **新窗/新进程**附着 | 从 main 开 work 得第二窗；原窗 PTY/lease 不变 |

数据源：现有 `list-instances` / `server-list` 注册表（`src/instances.rs` 等）。  
GUI 附着：`src/frontend_server.rs` + Windows `remote_frontend`（及相关）。  
Windows 起窗：`skills/agenterm-windows-gui-ops/SKILL.md`（Job breakaway；勿信死后的 `ui-snapshot`）。

## 工程约束

- 共享 checkout 的 `main`；精确 pathspec 提交；勿 `git add -A`
- 纯列表/快照逻辑可单测；行为黑盒走 CLI
- 改完回写 `plan/plan-v0.1.15.md` S′ 勾选 + 一行 journal；能力面变则改 owning PRD
- 验证梯子：unit/fmt → build → 必要 smoke；勿每叶 full release gate

## 完成定义

S1+S2+S3 在 Windows 上可演示；无 dual-main 回退；plan 已勾；报告：改动文件、三条试用命令、未做项（Unix/S4）。
