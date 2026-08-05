# Cloud Agent 会话登记表（主控 / 分身）

**显示名**在 [cursor.com/agents](https://cursor.com/agents) 里改（与「主控」相同操作）。
REST API **不能**改 `name`；下面登记表供主控与分身对齐身份与职责。

**互通**：有机感知 → [fleet-awareness.md](fleet-awareness.md)；状态 / 请示 → [mailbox.md](mailbox.md)；
动作清单 → [inter-agent-comms.md](inter-agent-comms.md)；换防 → [hand-off-controller.md](hand-off-controller.md)。
Agent chat：`scripts/cursor_agent_chat.sh`（默认 `--wait` + `<fleet-pulse>`）。

最后更新：2026-08-05（造梦未触发：环境 ID 不一致）

| 显示名 | bcId | 来源 | 当前职责 | 注释 |
|--------|------|------|----------|------|
| **主控2** | `bc-05b7c357-d712-440d-b140-8774bfa90e2a` | api | **当前主控**：统筹、合流、编排 | env=`7ef6e5b0-8a35-11f1-b532-320a589b8025` |
| **主控 造梦** | _(每次 Automations 新开 bcId)_ | automation | **auto-dream 值班**：只跑 `fleet-duty` 一轮 | **待修**：勿绑 `4f22874a-…`；须绑主控2 同环境 |
| **主控** | `bc-019fadf1-32a1-76ac-8b2c-086f8a4059a1` | mobile | 已换防 / 待命 | 勿再当唯一统筹 |
| **分身1** | `bc-5a9c83b4-3a39-42e4-9d33-cb705d848f8f` | api | 待命（旧 Linux platform） | 皮肤任务不复用 |
| **分身2** | `bc-26005f17-af78-4f63-bded-328cd1356396` | api | 待命（旧 Rhai/CI） | 皮肤任务不复用 |
| **分身3** | `bc-f2d5f6f1-41c0-44b1-bd74-1b80f036013b` | api | **皮肤设计**：`assets/skins/**` | classic/fancy tokens |
| **分身4** | `bc-c3e01145-b870-4e74-b18c-f3aea06ea800` | api | **皮肤工程**：theme/settings/snapshot/smoke | 与分身3 文件隔离 |

### Automations

| Name | cron (UTC) | environmentPublicId | 备注 |
|------|------------|---------------------|------|
| `主控 造梦` | `21 * * * *` | **必须** `7ef6e5b0-8a35-11f1-b532-320a589b8025` | 旧登记 `4f22874a-8a90-…` 与主控2 环境不一致；2026-08-05 查：本环境 `source=automations` agent **0** 个（含 archived）。改绑后点 Run now 验证；把 automation UUID URL 回填本表 |

链接：

- 主控2：https://cursor.com/agents/bc-05b7c357-d712-440d-b140-8774bfa90e2a
- 旧主控：https://cursor.com/agents/bc-019fadf1-32a1-76ac-8b2c-086f8a4059a1
- 分身1：https://cursor.com/agents/bc-5a9c83b4-3a39-42e4-9d33-cb705d848f8f
- 分身2：https://cursor.com/agents/bc-26005f17-af78-4f63-bded-328cd1356396
- 分身3：https://cursor.com/agents/bc-f2d5f6f1-41c0-44b1-bd74-1b80f036013b
- 分身4：https://cursor.com/agents/bc-c3e01145-b870-4e74-b18c-f3aea06ea800

## 命名约定

- **主控 / 主控N**：统筹会话；同时只应有一个「当前主控」。
- **分身N**：执行会话；创建时 `name` 尽量用 `分身N`。
- 换防：更新本表 + 给新主控完整 prompt；合完即删多余分支。

## Agent 间通信（推荐）

```bash
scripts/cursor_agent_chat.sh --list
scripts/cursor_agent_chat.sh --from 主控2 --to 分身3 '指令正文'
scripts/cursor_agent_chat.sh --from 主控2 --to 分身4 '指令正文'
```

静默前缀：`<from::主控2><to::分身3>`。默认等待 `409 agent_busy`。
