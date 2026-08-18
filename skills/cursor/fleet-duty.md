# 舰队值班 / Auto-Dream

周期性「做梦」：在无人盯梢时唤醒一个 Cloud Agent，主动扫态势、催交付、
准备合流，只把**需要人类拍板**的事留给用户。

这不是 Cloudflare Worker 巡检，也不是第二套 LLM-judge。  
**定时器在 Cursor Automations**；**脑在本 skill + git SSOT**。

相关：[fleet-awareness.md](fleet-awareness.md) · [inter-agent-comms.md](inter-agent-comms.md) ·
[hand-off-controller.md](hand-off-controller.md)

## 谁跑

| 角色 | 何时 |
|------|------|
| **主控**（优先） | 用户会话仍活着时，每轮可主动跑 duty；或 Automations 唤醒**同一显示名/新主控梦境会话** |
| **Dream 会话** | Automations cron 开的短命 agent（建议 `name`: `舰队梦境`），跑完写 mailbox 交接日志后 IDLE |

同一时刻只应有一个 duty 执行者（mailbox 里写 `duty.lock` 逻辑见下）。

## 本地工具

```bash
# 只读摘要（不发 API 消息）
scripts/cursor_agent_fleet_duty.sh

# 打印建议脉冲后，对「该催」的席位实际 chat（需 CURSOR_API）
scripts/cursor_agent_fleet_duty.sh --apply --from 主控2
```

## Duty 清单（按序）

1. `git fetch && git pull --ff-only origin main`
2. 读 `session-registry.md` + `mailbox.md`；跑 `cursor_agent_fleet_pulse.sh`
3. **未合分支**：`origin/cursor/*` 相对 `origin/main` 有 tip → 记「待审合」；若 mailbox 称 DONE/IDLE 且 tip 超龄 → **催主控审或催分身回报**
4. **席位超时**：
   - `RUNNING` / `ACTIVE` 但 mailbox 心跳过旧（默认 ≥ 4h）→ 探活短讯
   - `BLOCKED` / 请示空回复 ≥ 2h → 催主控填回复或升级用户
   - 分身 `IDLE` 且指令区仍有未消化主控指令 → 再 wake 一次
5. **依赖解锁**：设计已合 main、工程仍写「等设计」→ wake 工程开下一 Phase
6. **写交接日志**一行 + 刷新主控/梦境席位；无动作则写 `duty: noop`
7. **对用户**：仅当存在「需人类授权」项（发版、破所有权、删数据等）才 @；日常催办不打扰

## 禁止

- 在 dream 里大范围改产品代码（除非主控已授权且所有权清晰的小修）
- 互相同步 wake 造成风暴（每个 peer 每轮最多一条短讯）
- 把密钥写入 git / 聊天
- 用 CF Worker / 外站 LLM-judge 代替本 duty

## `duty.lock`（防并发）

mailbox「共享事实」可临时写：

| 键 | 值 |
|----|-----|
| `duty.lock` | `持有者 bcId / 直到 ISO 时间` |

新 dream 若见未过期 lock → 只读 pulse 后退出。持有者结束时清 lock。

## Cursor Automations 配置（需人类在 UI 创建）

入口：[cursor.com/automations](https://cursor.com/automations)

| 字段 | 建议 |
|------|------|
| Name | `AgenTerm 舰队梦境` |
| Trigger | Scheduled · cron 例：`0 */4 * * *`（每 4 小时 UTC）或 `0 2,8,14,20 * * *` |
| Repository | `partnernetsoftware/agenterm` · `main` |
| Environment | **必须与日常主控同一 Cloud Environment**（当前主控2：`7ef6e5b0-8a35-11f1-b532-320a589b8025`）。绑到另一个 environmentPublicId 时，定时可能看似创建但舰队列表里 `source=automations` 为空 |
| Tools | 允许改仓库 / 发 Cloud Agent 消息（按你租户权限勾选） |
| Prompt | 使用下一节**整段**（可按频率改 cron） |
| Enabled | 必须打开；创建后先 **Run now** 验证，再依赖 cron |

创建后把 automation id（dashboard URL 里的 UUID）记进 `session-registry.md`，便于主控核对。

## Automation 提示词（整段粘贴）

```text
你是【舰队梦境】短命值班会话（auto-dream）。不是替换主控的日常对话权，只跑一轮 duty。

立即：
1. git fetch && git checkout main && git pull --ff-only origin main
2. 读 skills/cursor/fleet-duty.md（全文照做）与 fleet-awareness.md / mailbox.md / session-registry.md
3. 若 mailbox 共享事实里 duty.lock 未过期且持有者不是你 → 写交接日志 duty: skipped-lock 后结束
4. 否则写入 duty.lock（你=bcId，直到现在+50分钟），跑：
   scripts/cursor_agent_fleet_duty.sh
   若存在明确可安全催办的对象，再（--from 必须等于本会话 Agents 显示名，
   当前 spawn 名为「舰队值班会话」，不是 Automation 名「主控 造梦」）：
   scripts/cursor_agent_fleet_duty.sh --apply --from '舰队值班会话'
5. 小步 commit+push：只允许改 skills/cursor/mailbox.md（席位/交接日志/lock）以及 duty 脚本若你发现显式 bug 的最小修复；勿借机做产品功能
6. 清掉自己的 duty.lock；席位写 IDLE；交接日志一行：duty: <摘要>
7. 若有必须人类拍板的项：在 mailbox 请示队列追加，并 chat 当前主控（registry 里「当前主控」）一条短讯

约束：遵循 AGENTS.md；密钥不进 git；不要开无必要 PR；不要 wake 已 ARCHIVED 的旧分身除非 registry 仍标活跃。
结束条件：一轮 duty 完成即停，勿自我重新 schedule。
```

## 主控日常（无 Automations 时）

用户一说话或你空闲时，主动跑：

```bash
scripts/cursor_agent_fleet_duty.sh
```

有「待审合 / 超时」则先处理再问用户下一指令——用户不应再说「主动探一下」。
