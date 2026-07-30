# Cloud Agent 会话登记表（主控 / 分身）

**显示名**在 [cursor.com/agents](https://cursor.com/agents) 里改（与「主控」相同操作）。
REST API **不能**改 `name`；下面登记表供主控与分身对齐身份与职责。

最后更新：2026-07-30

| 显示名 | bcId | 来源 | 职责 | 注释 |
|--------|------|------|------|------|
| **主控** | `bc-019fadf1-32a1-76ac-8b2c-086f8a4059a1` | mobile | 统筹、合 `main`、开分身、v0.2.0 规划 | 本对话；不抢 Win 全门禁实现 |
| **分身1** | `bc-5a9c83b4-3a39-42e4-9d33-cb705d848f8f` | api | v0.1.10 Windows 全门禁 | `check.cmd` 含 smoke；分支 `cursor/v0-1-10-win-full-gate-b30f`；`autoCreatePR` 关闭 |

链接：

- 主控：https://cursor.com/agents/bc-019fadf1-32a1-76ac-8b2c-086f8a4059a1
- 分身1：https://cursor.com/agents/bc-5a9c83b4-3a39-42e4-9d33-cb705d848f8f

## 命名约定

- **主控**：唯一统筹会话（产品、合流、派活）。
- **分身N**：API 或 UI 创建的执行会话；创建时 `name` 尽量用 `分身N` 或任务简称。
- 新开分身：递增 N，并更新本表（勿写 API 密钥）。

## 主控给分身发话（API）

分身 RUNNING 时 `POST /v1/agents/{id}/runs` 可能返回 `409 agent_busy`；等当前 run 结束再发，或直接在 cursor.com 该会话里聊天。

```bash
curl -sS -X POST \
  --url "https://api.cursor.com/v1/agents/<bcId>/runs" \
  -u "${CURSOR_API}:" \
  -H 'Content-Type: application/json' \
  -d '{"prompt":{"text":"主控指令：…"}}'
```
