# Cloud Agent 会话登记表（主控 / 分身）

**显示名**在 [cursor.com/agents](https://cursor.com/agents) 里改（与「主控」相同操作）。
REST API **不能**改 `name`；下面登记表供主控与分身对齐身份与职责。

**互通**：状态 / 请示 / 共享事实 → [mailbox.md](mailbox.md)；协议 → [inter-agent-comms.md](inter-agent-comms.md)。

最后更新：2026-07-30

| 显示名 | bcId | 来源 | 当前职责 | 注释 |
|--------|------|------|----------|------|
| **主控** | `bc-019fadf1-32a1-76ac-8b2c-086f8a4059a1` | mobile | 统筹、合流、开分身、邮箱裁决 | 本对话 |
| **分身1** | `bc-5a9c83b4-3a39-42e4-9d33-cb705d848f8f` | api | **原生 Linux Desktop** GUI + 测试套件黑盒 | 非 QEMU；computerUse + RecordScreen |
| **分身2** | `bc-26005f17-af78-4f63-bded-328cd1356396` | api | Linux Rhai / 自动化 / CI 跟随 | 与分身1 文件所有权隔离 |

链接：

- 主控：https://cursor.com/agents/bc-019fadf1-32a1-76ac-8b2c-086f8a4059a1
- 分身1：https://cursor.com/agents/bc-5a9c83b4-3a39-42e4-9d33-cb705d848f8f
- 分身2：https://cursor.com/agents/bc-26005f17-af78-4f63-bded-328cd1356396

## 命名约定

- **主控**：唯一统筹会话（产品、合流、派活、邮箱裁决）。
- **分身N**：API 或 UI 创建的执行会话；创建时 `name` 尽量用 `分身N`。
- 新开分身：递增 N，更新本表 **并** 在邮箱加席位块（勿写 API 密钥）。

## Agent 间通信（推荐）

用仓库脚本发带信封的 Cloud Agent 消息（脱敏，不打印 `CURSOR_API`）：

```bash
scripts/cursor_agent_chat.sh --list
scripts/cursor_agent_chat.sh --from 主控 --to 分身1 '指令正文'
scripts/cursor_agent_chat.sh --from 分身1 --to 主控 --stdin <<'EOF'
回报：…
EOF
```

静默前缀：`<from::主控><to::分身1>`。收件人 `RUNNING` 时 API 可能 `409`；`--dry-run` 可预览信封。
