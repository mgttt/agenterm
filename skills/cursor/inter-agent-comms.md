# 主控 ↔ 分身互通协议

API 单向推送（`POST /v1/agents/{id}/runs`）在分身 `RUNNING` 时常
`409 agent_busy`，且分身之间没有共享聊天线程。因此：

- **邮箱（git）= durable 态势 SSOT**
- **聊天 = 唤醒 / 改向脉冲**（默认附带舰队脉搏）
- **有机感知规范** → [fleet-awareness.md](fleet-awareness.md)

## 角色

| 角色 | 责任 |
|------|------|
| **主控** | 写任务简报、裁决冲突、合流 `main`、更新登记表；把用户意图翻译成有所有权边界的脉冲 |
| **分身** | 每轮 pull + 读全态势；更新本席位心跳；按独占文件集交付；IDLE 也留痕 |

## 文件

| 路径 | 用途 |
|------|------|
| [fleet-awareness.md](fleet-awareness.md) | **有机舰队感知**（生命周期、脉搏、自适应） |
| [session-registry.md](session-registry.md) | 身份、bcId、当前职责（低频） |
| [mailbox.md](mailbox.md) | 共享事实、席位状态、请示队列、交接日志（高频） |
| [hand-off-controller.md](hand-off-controller.md) | 主控换防 |
| [spawn-sibling-cloud-agent.md](spawn-sibling-cloud-agent.md) | 开分身 |
| 本文件 | 互通动作清单 |

勿把密钥、token、完整 `CURSOR_*` 值写入邮箱。

## 开工清单（每个席位每一轮）

1. `git fetch && git pull --ff-only origin main`（或当前工作分支并 rebase main）。
2. 读 `session-registry.md` + `mailbox.md` **全文**（获得同伴态势，不只读自己）。
3. 若有指向自己的 **主控指令** 或未决 **请示回复**，先执行再开新话题。
4. 覆盖更新本席位块：`状态 / 分支 / tip / 下一步 / 阻塞`（见模板）。
5. 小步 `commit` + `push`（邮箱可与代码同提交）。
6. 需要立刻唤醒谁：`scripts/cursor_agent_chat.sh --from … --to …`  
   默认：`--wait` + **注入 `<fleet-pulse>`**。短超时 `--wait-timeout 60`；`--no-wait`；`--no-fleet-context` 可关脉搏。

## 主控清单

1. 派活前写清：目标、禁止项、文件所有权、成功证据、是否允许合 `main`。
2. Spawn prompt 植入 [fleet-awareness.md](fleet-awareness.md) 骨架（同伴感知，而非孤岛任务）。
3. API 推送走 `cursor_agent_chat.sh`；观测用 MCP `list-cloud-agents` / `batch-fetch-details`。
4. 合流后更新邮箱「共享事实」的 `main` tip；打回用 Critical/Major 清单。
5. 用户只对主控说话时，由主控广播态势，勿让用户手工转发长文给分身。

## 席位状态模板

每个席位只保留**最新一块**（覆盖旧块）：

```markdown
### 分身N · YYYY-MM-DD HH:MM UTC
- 状态: RUNNING|IDLE|BLOCKED|DONE
- 分支: cursor/…
- tip: <sha7>
- main基准: <sha7>
- 本轮目标: …
- 已完成: …
- 证据: 测试命令 / CI / 路径
- 阻塞/请示: 无 | 见请示#K
- 下一步: …（IDLE 写「无新指令不开工」）
```

## 请示模板

```markdown
### 请示#K · 分身N → 主控 · YYYY-MM-DD HH:MM UTC
- 问题: …
- 选项: A) … B) …
- 建议: …
- 影响文件: …
- 主控回复: （空着等主控填）
```

## 与 API / MCP 的关系

| 通道 | 用途 | 限制 |
|------|------|------|
| 邮箱（git） | 状态、请示、事实、所有权 | 需 pull/push；冲突时主控裁决邮箱 |
| `cursor_agent_chat.sh` | 唤醒；信封 + 默认 fleet-pulse | 409 时默认等待；无回执线程 |
| `cursor_agent_fleet_pulse.sh` | 本地打印脉搏 | 不发 API；chat 内部会调用 |
| MCP list/batch-fetch | 主控巡检 | 只读 |
| MCP `mem_*` | 租户偏好（可选） | **不是**工程 SSOT |

## 禁止

- 只靠聊天记忆传递「当前 main / 职责 / 是否可合」。
- 结束任务却不更新邮箱；或只读自己的席位而忽略同伴。
- 两个分身同时编辑热点文件而不经主控分配所有权。
- 把密钥写入 git / 聊天明文。
