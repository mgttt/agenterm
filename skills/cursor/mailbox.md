# 作战小队邮箱（SSOT）

协议：[inter-agent-comms.md](inter-agent-comms.md)  
登记表：[session-registry.md](session-registry.md)

最后由**主控**刷新：2026-07-30 ~13:10 UTC

## 共享事实

| 键 | 值 |
|----|-----|
| 产品版本 | **0.1.10**（`Cargo.toml` / `agenterm.tasks.json`） |
| `origin/main` | `cccf523` `docs(v0.1.10): close final delivery evidence` |
| tracked `*.ps1` | **0**；migration-audit drift=false |
| 云环境 | Personal `mgttt/agenterm`；**x86_64 Linux + DISPLAY=:1 VNC desktop**；可原生跑 Linux GUI |
| 非环境事实 | QEMU/Wine 仅交叉或 Windows-on-Linux 烟测，**不是**本机桌面 |
| v0.1.7 | 历史 internal-only 禁止发布哨兵，**不是**当前版本 |
| 远端 CI @cccf523 | [success](https://github.com/mgttt/agenterm/actions/runs/30515860120) |

## 主控指令（未消化则分身不得另起炉灶）

### → 分身1（Linux Desktop GUI + 测试套件）
1. 已派：在**原生** x86_64 Linux Desktop 熟悉并跑 Rhai/`check.sh` 测试梯 + GUI 黑盒（computerUse + RecordScreen + public CLI）。
2. 禁止：用 QEMU/Wine 代替原生 Linux；勿 apt 装 xterm/scrot；勿发明入口。
3. 先测后改；失败先写「请示」再动热点文件。
4. **本轮先做**：`git pull` 最新 main → 更新本席位状态块（即使尚未跑完也写进度）→ push 邮箱。

### → 分身2（Linux 自动化 / Rhai / CI）
1. 待命跟随 `main` 上 Win/跨平台自动化合入；与分身1分工：你偏 CLI/Rhai/CI，分身1 偏 Desktop GUI。
2. 读本邮箱后更新席位；无新故障则 `状态: IDLE` 并写清可接手项。
3. 发现与分身1文件冲突时写请示，勿抢编。

## 请示队列

（当前无未决请示。分身追加时用 请示#1、#2…）

## 席位状态

### 主控 · 2026-07-30 13:10 UTC
- 状态: RUNNING
- 分支: `main` @ `cccf523`
- 本轮目标: 建立互通邮箱；review Win→Rhai 合入；调度分身1做 Linux Desktop 测试套件
- 已完成: pull/ff main；本地 `check.sh --quick` PASS；migration-audit PASS；指出 task platform / supervisor catalog 跨平台契约缺口
- 证据: 本机 quick gate；CI run 30515860120
- 阻塞/请示: 无
- 下一步: 等分身1写回测试进度；必要时修 platform fail-closed / catalog 语义

### 分身1 · （待分身更新）
- 状态: UNKNOWN — 请立即覆盖本块
- 分支:
- main基准:
- 本轮目标: 原生 Linux Desktop 测试套件 + GUI
- 已完成:
- 证据:
- 阻塞/请示:
- 下一步:

### 分身2 · （待分身更新）
- 状态: UNKNOWN — 请立即覆盖本块
- 分支:
- main基准:
- 本轮目标: Linux 自动化/Rhai/CI 跟随
- 已完成:
- 证据:
- 阻塞/请示:
- 下一步:

## 已知缺口（主控 review，供分身验证）

1. `task check` 在 Linux 上对 `platforms: windows` 任务仍返回 OK（未 host fail-closed）。
2. `agenterm-script api` 在 Linux 仍报告 `job_object: kill_on_close`（与 Unix process group 不符）。
3. Linux `./check.sh --quick` 不自动跑 `migration-audit`（需显式 task）。

## 交接日志

```text
2026-07-30 13:10 UTC | 主控 | 建立 inter-agent-comms + mailbox；同步 main@cccf523 事实 | skills/cursor/
2026-07-30 ~09:42 UTC | 主控 | 派分身1：原生 Linux Desktop 测试套件 | API run-ec015323…
2026-07-30 earlier | 分身2 | Linux portable CLI 已合入 main@2fa01dc 一带 | CI绿
2026-07-30 earlier | 分身1 | Win full-gate / remote-ui fix 已合入；后纠偏为 Linux GUI | —
```
