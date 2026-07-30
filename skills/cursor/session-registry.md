# Cloud Agent 会话登记表（主控 / 分身）

**显示名**在 [cursor.com/agents](https://cursor.com/agents) 里改（与「主控」相同操作）。
REST API **不能**改 `name`；下面登记表供主控与分身对齐身份与职责。

**互通**：状态 / 请示 / 共享事实 → [mailbox.md](mailbox.md)；协议 → [inter-agent-comms.md](inter-agent-comms.md)。
Agent chat：`scripts/cursor_agent_chat.sh`（默认 `--wait`）。

最后更新：2026-07-30（**换防：主控 → 主控2**）

| 显示名 | bcId | 来源 | 当前职责 | 注释 |
|--------|------|------|----------|------|
| **主控2** | `bc-05b7c357-d712-440d-b140-8774bfa90e2a` | api | **当前主控**：统筹、合流、跟进；用户暂不派分身 | 接防自旧主控 |
| **主控** | `bc-019fadf1-32a1-76ac-8b2c-086f8a4059a1` | mobile | 已换防 / 待命 | 勿再当唯一统筹 |
| **分身1** | `bc-5a9c83b4-3a39-42e4-9d33-cb705d848f8f` | api | Linux agent（待命） | 用户指示暂时不用分身 |
| **分身2** | `bc-26005f17-af78-4f63-bded-328cd1356396` | api | Linux Rhai/CI（待命） | 同上 |

链接：

- 主控2：https://cursor.com/agents/bc-05b7c357-d712-440d-b140-8774bfa90e2a
- 旧主控：https://cursor.com/agents/bc-019fadf1-32a1-76ac-8b2c-086f8a4059a1
- 分身1：https://cursor.com/agents/bc-5a9c83b4-3a39-42e4-9d33-cb705d848f8f
- 分身2：https://cursor.com/agents/bc-26005f17-af78-4f63-bded-328cd1356396

## 命名约定

- **主控 / 主控N**：统筹会话；同时只应有一个「当前主控」。
- **分身N**：执行会话；创建时 `name` 尽量用 `分身N`。
- 换防：更新本表 + 给新主控完整 prompt；合完即删多余分支。

## Agent 间通信（推荐）

```bash
scripts/cursor_agent_chat.sh --list
scripts/cursor_agent_chat.sh --from 主控2 --to 分身1 '指令正文'
```

静默前缀：`<from::主控2><to::分身1>`。默认等待 `409 agent_busy`。
